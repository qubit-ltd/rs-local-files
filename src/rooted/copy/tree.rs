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
use crate::local::LocalCopyDirError as Error;
use crate::local::LocalCopyDirOptions as Options;
use crate::local::LocalCopyDirStage as Stage;
use crate::local::LocalCopyDirStats as Statistics;
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
    let mut frames = vec![CopyFrame {
        source: source.clone(),
        destination: destination.clone(),
        metadata: source_metadata,
        depth: 0,
        reader,
        directory_permit: root_permit,
    }];
    let mut active_sources = vec![source.clone()];

    while !frames.is_empty() {
        let (source, destination, depth) = {
            let frame = frames.last().expect("non-empty frame stack");
            (frame.source.clone(), frame.destination.clone(), frame.depth)
        };
        if let Err(source_error) = budget.check_deadline() {
            return Err(error(
                Stage::ReadSourceDirectory,
                &source,
                &destination,
                statistics,
                source_error,
            ));
        }
        let next = frames.last_mut().expect("non-empty frame stack").reader.next_entry();
        let entry = match next {
            Ok(Some(entry)) => entry,
            Ok(None) => {
                let frame = frames.pop().expect("non-empty frame stack");
                preserve_permissions(
                    root,
                    &frame.source,
                    &frame.destination,
                    frame.metadata,
                    options,
                    statistics,
                )?;
                active_sources.pop();
                drop(frame.directory_permit);
                continue;
            }
            Err(source_error) => {
                return Err(error(
                    Stage::ReadSourceDirectory,
                    &source,
                    &destination,
                    statistics,
                    source_error,
                ));
            }
        };
        let source_child = source
            .join_component(entry.name())
            .expect("root directory entry names are normal components");
        let destination_child = destination
            .join_component(entry.name())
            .expect("root directory entry names are normal components");
        let child_depth = depth.saturating_add(1);
        if let Err(source_error) = budget.check_depth(child_depth) {
            return Err(error(
                Stage::InspectSourceEntry,
                &source_child,
                &destination_child,
                statistics,
                source_error,
            ));
        }
        if let Err(source_error) = budget.charge_entry() {
            return Err(error(
                Stage::UpdateStatistics,
                &source_child,
                &destination_child,
                statistics,
                source_error,
            ));
        }

        match entry.metadata().kind() {
            EntryKind::File => {
                statistics = copy_file(
                    root,
                    &source_child,
                    &destination_child,
                    options,
                    durability,
                    statistics,
                    budget,
                )?;
            }
            EntryKind::Directory => {
                if prepare_directory(root, &source_child, &destination_child, options, &mut statistics)? {
                    push_directory(
                        root,
                        &source_child,
                        &destination_child,
                        entry.metadata(),
                        child_depth,
                        &mut frames,
                        &mut active_sources,
                        budget,
                        statistics,
                    )?;
                }
            }
            EntryKind::Symlink => {
                if options.symlink_policy().follows() {
                    let resolved = match crate::rooted_local_file_system::resolve_rooted_path(
                        root,
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
                                statistics,
                                copy_error.into_io_error(),
                            ));
                        }
                    };
                    let resolved_metadata = match root.symlink_metadata(&resolved) {
                        Ok(metadata) => metadata,
                        Err(source_error) => {
                            return Err(error(
                                Stage::InspectSourceEntry,
                                &source_child,
                                &destination_child,
                                statistics,
                                source_error,
                            ));
                        }
                    };
                    if resolved_metadata.kind() == EntryKind::Directory {
                        if prepare_directory(root, &resolved, &destination_child, options, &mut statistics)? {
                            push_directory(
                                root,
                                &resolved,
                                &destination_child,
                                resolved_metadata,
                                child_depth,
                                &mut frames,
                                &mut active_sources,
                                budget,
                                statistics,
                            )?;
                        }
                    } else {
                        statistics =
                            copy_symlink(root, &source_child, &destination_child, options, statistics, budget)?;
                    }
                } else {
                    statistics = copy_symlink(root, &source_child, &destination_child, options, statistics, budget)?;
                }
            }
            EntryKind::Other => {
                return Err(error(
                    Stage::InspectSourceEntry,
                    &source_child,
                    &destination_child,
                    statistics,
                    unsupported_source_error(),
                ));
            }
            #[cfg(unix)]
            EntryKind::Fifo | EntryKind::Socket | EntryKind::BlockDevice | EntryKind::CharDevice => {
                return Err(error(
                    Stage::InspectSourceEntry,
                    &source_child,
                    &destination_child,
                    statistics,
                    unsupported_source_error(),
                ));
            }
        }
    }
    Ok(statistics)
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
