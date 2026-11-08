// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Rooted directory-tree copy traversal.

use std::io;
use std::io::ErrorKind;

use super::EntryKind;
use super::Metadata;
use super::Path;
use super::Root;
use super::copy_file;
use super::copy_frame::CopyFrame;
use super::copy_symlink;
use super::destination::error;
use super::destination::prepare_directory;
use super::destination::unsupported_source_error;
use super::file::preserve_permissions;
use crate::LocalDurabilityRequirement;
use crate::local::CopyBudget;
use crate::local::CopyTreeBackend;
use crate::local::CopyTreeFrameContext;
use crate::local::LocalCopyDirError as Error;
use crate::local::LocalCopyDirOptions as Options;
use crate::local::LocalCopyDirStage as Stage;
use crate::local::LocalCopyDirStats as Statistics;
use crate::local::copy_tree as run_copy_tree;
use crate::rooted::DirectoryReader;

/// Copies a rooted directory tree with a lazy depth-first work stack.
///
/// # Parameters
///
/// - `root`: Opened authority used for every source and destination operation.
/// - `source`: Rooted source directory to enumerate.
/// - `destination`: Rooted destination directory to populate.
/// - `source_metadata`: Metadata already inspected for the source root.
/// - `options`: Copy policy controlling conflicts, links, and preservation.
/// - `durability`: Required durability for destination publication.
/// - `budget`: Mutable resource limits shared with the traversal scheduler.
///
/// # Returns
///
/// Statistics for completed copy effects.
///
/// # Errors
///
/// Returns the failing stage, coordinates, partial effects, and source error.
pub(super) fn copy_tree(
    root: &Root,
    source: &Path,
    destination: &Path,
    source_metadata: Metadata,
    options: &Options,
    durability: LocalDurabilityRequirement,
    budget: &mut CopyBudget,
) -> Result<Statistics, Error> {
    let mut statistics = Statistics::default();
    validate_source_directory(source, destination, destination, &[], statistics)?;
    let root_permit = budget.acquire_directory().map_err(|source_error| {
        error(
            Stage::ReadSourceDirectory,
            source,
            destination,
            statistics,
            source_error,
        )
    })?;
    if !prepare_directory(root, source, destination, options, &mut statistics, budget)? {
        drop(root_permit);
        return Ok(statistics);
    }
    let reader = open_root_reader(root, source, destination, statistics)?;
    let root_frame = CopyFrame {
        source: source.clone(),
        destination: destination.clone(),
        metadata: source_metadata,
        depth: 0,
        reader,
        directory_permit: root_permit,
    };
    let mut backend = RootedCopyBackend {
        root,
        options,
        durability,
        destination_root: destination.clone(),
        active_sources: vec![source.clone()],
    };
    run_copy_tree(&mut backend, root_frame, &mut statistics, budget)?;
    Ok(statistics)
}

/// Rooted native operations bound to one immutable authority.
struct RootedCopyBackend<'a> {
    /// Opened authority shared by all native operations.
    root: &'a Root,
    /// Fixed policy for this copy.
    options: &'a Options,
    /// Selected publication durability requirement.
    durability: LocalDurabilityRequirement,
    /// Original destination root for source-containment checks.
    destination_root: Path,
    /// Resolved source paths for active-ancestor cycle detection.
    active_sources: Vec<Path>,
}

impl CopyTreeBackend for RootedCopyBackend<'_> {
    /// Owns the active directory reader and its managed-resource permit.
    type Frame = CopyFrame;
    /// One lazily enumerated child before scheduler budget checks.
    type Entry = super::super::Entry;

    /// Clones frame coordinates for diagnostics without opening native handles.
    ///
    /// # Parameters
    ///
    /// - `frame`: Active frame whose paths and depth are copied.
    ///
    /// # Returns
    ///
    /// The frame's source and destination coordinates and logical depth.
    #[inline]
    fn frame_context(&self, frame: &Self::Frame) -> CopyTreeFrameContext {
        CopyTreeFrameContext {
            source: frame.source.as_path().to_path_buf(),
            destination: frame.destination.as_path().to_path_buf(),
            depth: frame.depth,
        }
    }

    /// Reads one child lazily; `None` means the directory is exhausted.
    ///
    /// # Parameters
    ///
    /// - `frame`: Active directory reader advanced by this operation.
    /// - `stats`: Completed copy effects included in any read error.
    ///
    /// # Returns
    ///
    /// The next lazy entry, or `None` when the directory is exhausted.
    ///
    /// # Errors
    ///
    /// Preserves current statistics and directory coordinates on reader
    /// failure.
    fn next_entry(&mut self, frame: &mut Self::Frame, stats: &Statistics) -> Result<Option<Self::Entry>, Error> {
        match frame.reader.next_entry() {
            Ok(entry) => Ok(entry),
            Err(source_error) => Err(error(
                Stage::ReadSourceDirectory,
                &frame.source,
                &frame.destination,
                *stats,
                source_error,
            )),
        }
    }

    /// Constructs child coordinates once, at one level below the active frame.
    ///
    /// # Parameters
    ///
    /// - `frame`: Parent coordinates used to derive child paths and depth.
    /// - `entry`: Native entry whose child coordinates are constructed.
    ///
    /// # Returns
    ///
    /// The child's source and destination coordinates and logical depth.
    #[inline]
    fn child_context(&self, frame: &CopyTreeFrameContext, entry: &Self::Entry) -> CopyTreeFrameContext {
        CopyTreeFrameContext {
            source: frame.source.join(entry.name()),
            destination: frame.destination.join(entry.name()),
            depth: frame.depth.saturating_add(1),
        }
    }

    /// Publishes one admitted child or returns its owned directory frame.
    ///
    /// The scheduler has already charged the entry and checked its depth. This
    /// backend charges bytes and acquired handles, and records destination
    /// mutations as they occur. `None` means a leaf was copied or skipped;
    /// `Some` transfers a child reader and permit to the scheduler.
    ///
    /// # Parameters
    ///
    /// - `entry`: Child entry already charged by the scheduler.
    /// - `frame`: Precomputed child coordinates and depth.
    /// - `stats`: Mutable counters updated as destination effects complete.
    /// - `budget`: Shared byte and open-directory resource accounting state.
    ///
    /// # Returns
    ///
    /// `Some(frame)` when descent is required, otherwise `None`.
    ///
    /// # Errors
    ///
    /// Returns native or policy failures with all recorded partial effects.
    ///
    /// # Panics
    ///
    /// Panics if the scheduler supplies paths that are not valid rooted paths.
    fn process_entry(
        &mut self,
        entry: Self::Entry,
        frame: &CopyTreeFrameContext,
        stats: &mut Statistics,
        budget: &mut CopyBudget,
    ) -> Result<Option<Self::Frame>, Error> {
        let source_child = Path::new(&frame.source).expect("scheduler preserves rooted source paths");
        let destination_child = Path::new(&frame.destination).expect("scheduler preserves rooted destination paths");
        let mut child_frame = None;
        match entry.metadata().kind() {
            EntryKind::File => {
                *stats = copy_file(
                    self.root,
                    &source_child,
                    &destination_child,
                    self.options,
                    self.durability,
                    *stats,
                    budget,
                )?;
            }
            EntryKind::Directory => {
                validate_source_directory(
                    &source_child,
                    &destination_child,
                    &self.destination_root,
                    &self.active_sources,
                    *stats,
                )?;
                if prepare_directory(
                    self.root,
                    &source_child,
                    &destination_child,
                    self.options,
                    stats,
                    budget,
                )? {
                    let current = *stats;
                    child_frame = Some(enter_directory(
                        self.root,
                        &source_child,
                        &destination_child,
                        entry.metadata(),
                        frame.depth,
                        &mut self.active_sources,
                        budget,
                        current,
                    )?);
                }
            }
            EntryKind::Symlink => {
                if self.options.symlink_policy().follows() {
                    let deadline = budget.absolute_deadline().map_err(|source_error| {
                        error(
                            Stage::InspectSourceEntry,
                            &source_child,
                            &destination_child,
                            *stats,
                            source_error,
                        )
                    })?;
                    let resolved = match crate::rooted_local_file_system::resolve_rooted_path_with_deadline(
                        self.root,
                        source_child.as_path(),
                        crate::LocalSymlinkPolicy::FollowWithinScope,
                        true,
                        crate::LocalFileOperation::Copy,
                        deadline,
                    ) {
                        Ok(resolved) => resolved,
                        Err(copy_error) if copy_error.kind() == crate::LocalFileErrorKind::NotFound => {
                            *stats = copy_symlink(
                                self.root,
                                &source_child,
                                &destination_child,
                                self.options,
                                *stats,
                                budget,
                            )?;
                            return Ok(None);
                        }
                        Err(copy_error) => {
                            return Err(error(
                                Stage::InspectSourceEntry,
                                &source_child,
                                &destination_child,
                                *stats,
                                copy_error.into_io_error(),
                            ));
                        }
                    };
                    #[cfg(test)]
                    crate::tests::rooted::support::resolution_observation_tests::reach_deadline_boundary(
                        crate::tests::rooted::support::resolution_observation_tests::DeadlineBoundary::FollowedLinkResolved,
                    );
                    budget.check_deadline().map_err(|source_error| {
                        error(
                            Stage::InspectSourceEntry,
                            &source_child,
                            &destination_child,
                            *stats,
                            source_error,
                        )
                    })?;
                    #[cfg(test)]
                    crate::tests::rooted::support::resolution_observation_tests::record_metadata();
                    let resolved_metadata = match self.root.symlink_metadata(&resolved) {
                        Ok(metadata) => metadata,
                        Err(source_error) if source_error.kind() == ErrorKind::NotFound => {
                            *stats = copy_symlink(
                                self.root,
                                &source_child,
                                &destination_child,
                                self.options,
                                *stats,
                                budget,
                            )?;
                            return Ok(None);
                        }
                        Err(source_error) => {
                            return Err(error(
                                Stage::InspectSourceEntry,
                                &source_child,
                                &destination_child,
                                *stats,
                                source_error,
                            ));
                        }
                    };
                    if resolved_metadata.kind() == EntryKind::Directory {
                        validate_source_directory(
                            &resolved,
                            &destination_child,
                            &self.destination_root,
                            &self.active_sources,
                            *stats,
                        )?;
                        if prepare_directory(self.root, &resolved, &destination_child, self.options, stats, budget)? {
                            let current = *stats;
                            child_frame = Some(enter_directory(
                                self.root,
                                &resolved,
                                &destination_child,
                                resolved_metadata,
                                frame.depth,
                                &mut self.active_sources,
                                budget,
                                current,
                            )?);
                        }
                    } else {
                        *stats = copy_symlink(
                            self.root,
                            &source_child,
                            &destination_child,
                            self.options,
                            *stats,
                            budget,
                        )?;
                    }
                } else {
                    *stats = copy_symlink(
                        self.root,
                        &source_child,
                        &destination_child,
                        self.options,
                        *stats,
                        budget,
                    )?;
                }
            }
            EntryKind::Other => {
                return Err(error(
                    Stage::InspectSourceEntry,
                    &source_child,
                    &destination_child,
                    *stats,
                    unsupported_source_error(),
                ));
            }
            #[cfg(unix)]
            EntryKind::Fifo | EntryKind::Socket | EntryKind::BlockDevice | EntryKind::CharDevice => {
                return Err(error(
                    Stage::InspectSourceEntry,
                    &source_child,
                    &destination_child,
                    *stats,
                    unsupported_source_error(),
                ));
            }
        }
        Ok(child_frame)
    }

    /// Applies final directory permissions after all descendants are processed.
    ///
    /// Consumes the frame, releasing its reader and permit on success or error.
    ///
    /// # Parameters
    ///
    /// - `frame`: Completed directory frame consumed while finalizing it.
    /// - `stats`: Completed effects retained if permission preservation fails.
    ///
    /// # Returns
    ///
    /// `Ok(())` after final permissions have been applied.
    ///
    /// # Errors
    ///
    /// Reports permission preservation failures with current copy statistics.
    fn finish_frame(&mut self, frame: Self::Frame, stats: &mut Statistics) -> Result<(), Error> {
        preserve_permissions(
            self.root,
            &frame.source,
            &frame.destination,
            frame.metadata,
            self.options,
            stats,
        )?;
        self.active_sources.pop();
        // Close the reader before making its capacity available again.
        drop(frame.reader);
        drop(frame.directory_permit);
        Ok(())
    }

    /// Attaches scheduler failure coordinates and a snapshot of partial
    /// effects.
    ///
    /// # Parameters
    ///
    /// - `stage`: Scheduler stage at which the failure occurred.
    /// - `context`: Source/destination coordinates and depth for the failure.
    /// - `stats`: Completed copy effects included in the error.
    /// - `source_error`: Underlying scheduler or I/O failure.
    ///
    /// # Returns
    ///
    /// A copy error carrying the stage, coordinates, statistics, and cause.
    fn error(
        &self,
        stage: Stage,
        context: &CopyTreeFrameContext,
        stats: &Statistics,
        source_error: io::Error,
    ) -> Error {
        let source = Path::new(&context.source).expect("scheduler preserves rooted source paths");
        let destination = Path::new(&context.destination).expect("scheduler preserves rooted destination paths");
        error(stage, &source, &destination, *stats, source_error)
    }
}

/// Opens the initial lazy reader or reports its path and partial statistics.
///
/// # Parameters
///
/// - `root`: Opened authority from which the reader is created.
/// - `source`: Rooted source directory to open.
/// - `destination`: Destination coordinate attached to any open error.
/// - `statistics`: Completed effects included in any open error.
///
/// # Returns
///
/// A lazy reader for the source directory.
///
/// # Errors
///
/// Returns a read-stage error with the source and destination coordinates.
fn open_root_reader(
    root: &Root,
    source: &Path,
    destination: &Path,
    statistics: Statistics,
) -> Result<DirectoryReader, Error> {
    #[cfg(feature = "test-support")]
    if crate::local::test_support_enabled("rooted-copy-directory-read") {
        return Err(error(
            Stage::ReadSourceDirectory,
            source,
            destination,
            statistics,
            io::Error::from(ErrorKind::PermissionDenied),
        ));
    }
    let opened = if source.as_path().as_os_str().is_empty() {
        root.open_root_dir_reader()
    } else {
        root.open_dir_reader(source)
    };
    opened.map_err(|source_error| {
        error(
            Stage::ReadSourceDirectory,
            source,
            destination,
            statistics,
            source_error,
        )
    })
}

/// Opens one child reader after cycle and permit checks.
///
/// Returns an owned frame for the scheduler; errors release acquired capacity
/// and retain any destination changes already recorded by the caller.
///
/// # Parameters
///
/// - `root`: Opened authority used to create the child reader.
/// - `source`: Child source directory to open.
/// - `destination`: Child destination coordinate for diagnostics.
/// - `metadata`: Inspected metadata retained in the resulting frame.
/// - `depth`: Logical depth of the child relative to the requested root.
/// - `active_sources`: Active source ancestors updated after successful open.
/// - `budget`: Shared directory permit accounting.
/// - `statistics`: Completed effects included in any open error.
///
/// # Returns
///
/// An owned frame holding the reader and its directory permit.
///
/// # Errors
///
/// Returns a read-stage error; any acquired permit is released on failure.
#[allow(clippy::too_many_arguments)]
fn enter_directory(
    root: &Root,
    source: &Path,
    destination: &Path,
    metadata: Metadata,
    depth: usize,
    active_sources: &mut Vec<Path>,
    budget: &CopyBudget,
    statistics: Statistics,
) -> Result<CopyFrame, Error> {
    let directory_permit = budget.acquire_directory().map_err(|source_error| {
        error(
            Stage::ReadSourceDirectory,
            source,
            destination,
            statistics,
            source_error,
        )
    })?;
    #[cfg(feature = "test-support")]
    if crate::local::test_support_enabled("rooted-copy-directory-read") {
        drop(directory_permit);
        return Err(error(
            Stage::ReadSourceDirectory,
            source,
            destination,
            statistics,
            io::Error::from(ErrorKind::PermissionDenied),
        ));
    }
    let reader = match root.open_dir_reader(source) {
        Ok(reader) => reader,
        Err(source_error) => {
            drop(directory_permit);
            return Err(error(
                Stage::ReadSourceDirectory,
                source,
                destination,
                statistics,
                source_error,
            ));
        }
    };
    active_sources.push(source.clone());
    Ok(CopyFrame {
        source: source.clone(),
        destination: destination.clone(),
        metadata,
        depth,
        reader,
        directory_permit,
    })
}

/// Rejects a source directory that contains the fixed destination root or is
/// already an active ancestor before any child destination is prepared.
///
/// # Parameters
///
/// - `source`: Candidate source directory to validate.
/// - `destination`: Corresponding destination coordinate for diagnostics.
/// - `destination_root`: Fixed destination root for overlap checks.
/// - `active_sources`: Ancestor source directories checked for cycles.
/// - `statistics`: Completed effects included in any validation error.
///
/// # Returns
///
/// `Ok(())` when the source is disjoint from the destination and ancestors.
///
/// # Errors
///
/// Returns an inspect-source error for overlap or an active-source cycle.
fn validate_source_directory(
    source: &Path,
    destination: &Path,
    destination_root: &Path,
    active_sources: &[Path],
    statistics: Statistics,
) -> Result<(), Error> {
    if destination_root.as_path().starts_with(source.as_path())
        || source.as_path().starts_with(destination_root.as_path())
    {
        return Err(error(
            Stage::InspectSource,
            source,
            destination,
            statistics,
            io::Error::new(
                ErrorKind::InvalidInput,
                "rooted destination must not be inside the source or overlap its ancestor",
            ),
        ));
    }
    if active_sources.iter().any(|active| active == source) {
        return Err(error(
            Stage::InspectSource,
            source,
            destination,
            statistics,
            io::Error::new(ErrorKind::InvalidInput, "rooted copy source directory cycle detected"),
        ));
    }
    Ok(())
}
