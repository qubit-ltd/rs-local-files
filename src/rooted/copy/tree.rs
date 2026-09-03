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
use crate::local::CopyBudget;
use crate::local::LocalCopyDirError as Error;
use crate::local::LocalCopyDirOptions as Options;
use crate::local::LocalCopyDirStage as Stage;
use crate::local::LocalCopyDirStats as Statistics;
use crate::rooted::work::Work;

/// Copies a rooted directory tree with an explicit work stack.
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
    let directory_permit = budget.acquire_directory().map_err(|source_error| {
        error(
            Stage::ReadSourceDirectory,
            source,
            destination,
            statistics,
            source_error,
        )
    })?;
    drop(directory_permit);
    if !prepare_directory(root, source, destination, options, &mut statistics)? {
        return Ok(statistics);
    }
    let mut work = vec![Work::Enter {
        source: source.clone(),
        destination: destination.clone(),
        metadata: source_metadata,
        depth: 0,
    }];
    let mut active_sources = Vec::new();
    while let Some(item) = work.pop() {
        match item {
            Work::Enter {
                source,
                destination,
                metadata,
                depth,
            } => {
                if let Err(source_error) = budget.check_deadline() {
                    return Err(error(
                        Stage::ReadSourceDirectory,
                        &source,
                        &destination,
                        statistics,
                        source_error,
                    ));
                }
                if active_sources.iter().any(|active| active == &source) {
                    return Err(error(
                        Stage::InspectSource,
                        &source,
                        &destination,
                        statistics,
                        io::Error::new(ErrorKind::InvalidInput, "rooted copy source directory cycle detected"),
                    ));
                }
                active_sources.push(source.clone());
                #[cfg(feature = "test-support")]
                if crate::local::test_support_enabled("rooted-copy-directory-read") {
                    return Err(error(
                        Stage::ReadSourceDirectory,
                        &source,
                        &destination,
                        statistics,
                        io::Error::from(ErrorKind::PermissionDenied),
                    ));
                }
                let directory_permit = match budget.acquire_directory() {
                    Ok(permit) => permit,
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
                let entries_result = root.read_dir(&source);
                drop(directory_permit);
                let entries = match entries_result {
                    Ok(entries) => entries,
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
                work.push(Work::Finish {
                    source: source.clone(),
                    destination: destination.clone(),
                    metadata,
                });
                for entry in entries.into_iter().rev() {
                    // `Root::read_dir` constructs entries only from native
                    // directory names, which are guaranteed normal relative
                    // components. Revalidating them cannot fail.
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
                                work.push(Work::Enter {
                                    source: source_child,
                                    destination: destination_child,
                                    metadata: entry.metadata(),
                                    depth: child_depth,
                                });
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
                                    if prepare_directory(root, &resolved, &destination_child, options, &mut statistics)?
                                    {
                                        work.push(Work::Enter {
                                            source: resolved,
                                            destination: destination_child,
                                            metadata: resolved_metadata,
                                            depth: child_depth,
                                        });
                                    }
                                    continue;
                                }
                                statistics =
                                    copy_symlink(root, &source_child, &destination_child, options, statistics, budget)?;
                                continue;
                            }
                            statistics =
                                copy_symlink(root, &source_child, &destination_child, options, statistics, budget)?;
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
            }
            Work::Finish {
                source,
                destination,
                metadata,
            } => {
                preserve_permissions(root, &source, &destination, metadata, options, statistics)?;
                active_sources.pop();
            }
        }
    }
    Ok(statistics)
}
