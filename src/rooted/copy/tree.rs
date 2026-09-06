// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Rooted directory-tree copy traversal.
// qubit-style: allow source-test-pair

use std::io;
use std::io::ErrorKind;

use qubit_budget::ManagedResourcePermit;

use super::EntryKind;
use super::Metadata;
use super::Path;
use super::Root;
use super::copy_file;
use super::copy_symlink;
use super::destination::error;
use super::destination::prepare_directory;
use super::destination::unsupported_source_error;
use super::file::preserve_permissions;
use crate::LocalDurabilityRequirement;
use crate::LocalResourceKind;
use crate::local::CopyBudget;
use crate::local::CopyTreeBackend;
use crate::local::CopyTreeFrameContext;
use crate::local::LocalCopyDirError as Error;
use crate::local::LocalCopyDirOptions as Options;
use crate::local::LocalCopyDirStage as Stage;
use crate::local::LocalCopyDirStats as Statistics;
use crate::local::copy_tree as run_copy_tree;
use crate::rooted::DirectoryReader;

/// One active rooted directory reader retained until its children finish.
#[derive(Debug)]
struct CopyFrame {
    source: Path,
    destination: Path,
    metadata: Metadata,
    depth: usize,
    reader: DirectoryReader,
    directory_permit: Option<ManagedResourcePermit<LocalResourceKind, usize>>,
}

/// Copies a rooted directory tree with a lazy depth-first work stack.
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
    let root_permit = budget.acquire_directory().map_err(|source_error| {
        error(
            Stage::ReadSourceDirectory,
            source,
            destination,
            statistics,
            source_error,
        )
    })?;
    if !prepare_directory(root, source, destination, options, &mut statistics)? {
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
        active_sources: vec![source.clone()],
    };
    run_copy_tree(&mut backend, root_frame, &mut statistics, budget)?;
    Ok(statistics)
}

struct RootedCopyBackend<'a> {
    root: &'a Root,
    options: &'a Options,
    durability: LocalDurabilityRequirement,
    active_sources: Vec<Path>,
}

impl CopyTreeBackend for RootedCopyBackend<'_> {
    type Frame = CopyFrame;
    type Entry = super::super::Entry;

    fn frame_context(&self, frame: &Self::Frame) -> CopyTreeFrameContext {
        CopyTreeFrameContext {
            source: frame.source.as_path().to_path_buf(),
            destination: frame.destination.as_path().to_path_buf(),
            depth: frame.depth,
        }
    }

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

    fn child_context(&self, frame: &CopyTreeFrameContext, entry: &Self::Entry) -> CopyTreeFrameContext {
        CopyTreeFrameContext {
            source: frame.source.join(entry.name()),
            destination: frame.destination.join(entry.name()),
            depth: frame.depth.saturating_add(1),
        }
    }

    fn process_entry(
        &mut self,
        entry: Self::Entry,
        frame: &CopyTreeFrameContext,
        frames: &mut Vec<Self::Frame>,
        stats: &mut Statistics,
        budget: &mut CopyBudget,
    ) -> Result<(), Error> {
        let source_child = Path::new(frame.source.join(entry.name())).expect("scheduler preserves rooted source paths");
        let destination_child =
            Path::new(frame.destination.join(entry.name())).expect("scheduler preserves rooted destination paths");
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
                if prepare_directory(self.root, &source_child, &destination_child, self.options, stats)? {
                    let current = *stats;
                    push_directory(
                        self.root,
                        &source_child,
                        &destination_child,
                        entry.metadata(),
                        frame.depth.saturating_add(1),
                        frames,
                        &mut self.active_sources,
                        budget,
                        current,
                    )?;
                }
            }
            EntryKind::Symlink => {
                if self.options.symlink_policy().follows() {
                    let resolved = match crate::rooted_local_file_system::resolve_rooted_path(
                        self.root,
                        source_child.as_path(),
                        crate::LocalSymlinkPolicy::FollowWithinScope,
                        true,
                        crate::LocalFileOperation::Copy,
                    ) {
                        Ok(resolved) => resolved,
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
                    let resolved_metadata = match self.root.symlink_metadata(&resolved) {
                        Ok(metadata) => metadata,
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
                        if prepare_directory(self.root, &resolved, &destination_child, self.options, stats)? {
                            let current = *stats;
                            push_directory(
                                self.root,
                                &resolved,
                                &destination_child,
                                resolved_metadata,
                                frame.depth.saturating_add(1),
                                frames,
                                &mut self.active_sources,
                                budget,
                                current,
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
        Ok(())
    }

    fn finish_frame(&mut self, frame: Self::Frame, stats: &mut Statistics) -> Result<(), Error> {
        preserve_permissions(
            self.root,
            &frame.source,
            &frame.destination,
            frame.metadata,
            self.options,
            *stats,
        )?;
        self.active_sources.pop();
        drop(frame.directory_permit);
        Ok(())
    }

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

#[allow(clippy::too_many_arguments)]
fn push_directory(
    root: &Root,
    source: &Path,
    destination: &Path,
    metadata: Metadata,
    depth: usize,
    frames: &mut Vec<CopyFrame>,
    active_sources: &mut Vec<Path>,
    budget: &CopyBudget,
    statistics: Statistics,
) -> Result<(), Error> {
    if active_sources.iter().any(|active| active == source) {
        return Err(error(
            Stage::InspectSource,
            source,
            destination,
            statistics,
            io::Error::new(ErrorKind::InvalidInput, "rooted copy source directory cycle detected"),
        ));
    }
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
    frames.push(CopyFrame {
        source: source.clone(),
        destination: destination.clone(),
        metadata,
        depth,
        reader,
        directory_permit,
    });
    Ok(())
}
