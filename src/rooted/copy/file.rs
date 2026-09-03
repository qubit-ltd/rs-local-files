// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Rooted regular-file copy and staged publication.
// qubit-style: allow source-test-pair

use std::io;
use std::io::ErrorKind;

use super::EntryKind;
use super::Metadata;
use super::Path;
use super::Root;
use super::destination::checked_add;
use super::destination::error;
use super::destination::optional_metadata;
use crate::LocalAtomicWriteOptions;
use crate::LocalDurabilityRequirement;
use crate::local::CopyBudget;
use crate::local::CopyDestinationAction;
use crate::local::LocalCopyDirError as Error;
use crate::local::LocalCopyDirOptions as Options;
use crate::local::LocalCopyDirStage as Stage;
use crate::local::LocalCopyDirStats as Statistics;
use crate::local::decide_copy_destination;
use crate::read;

/// Copies one regular file with handle-authoritative source metadata.
pub(super) fn copy_file(
    root: &Root,
    source: &Path,
    destination: &Path,
    options: &Options,
    durability: LocalDurabilityRequirement,
    mut statistics: Statistics,
    budget: &mut CopyBudget,
) -> Result<Statistics, Error> {
    budget
        .check_deadline()
        .map_err(|source_error| error(Stage::InspectSourceEntry, source, destination, statistics, source_error))?;
    #[cfg(feature = "test-support")]
    if crate::local::take_test_support_on_nth("rooted-copy-file-second", 2) {
        return Err(error(
            Stage::CopyFileContents,
            source,
            destination,
            statistics,
            crate::local::test_fault_error(),
        ));
    }
    #[cfg(feature = "test-support")]
    if crate::local::test_support_enabled("rooted-copy-source-open") {
        return Err(error(
            Stage::InspectSourceEntry,
            source,
            destination,
            statistics,
            io::Error::from(ErrorKind::PermissionDenied),
        ));
    }
    let mut reader = root
        .open_reader(source, &read::OpenOptions::default())
        .map_err(|source_error| error(Stage::InspectSourceEntry, source, destination, statistics, source_error))?;
    #[cfg(feature = "test-support")]
    let source_metadata_result = if crate::local::test_support_enabled("rooted-copy-source-metadata-native") {
        Err(crate::local::test_fault_error())
    } else {
        Metadata::from_open_file(&reader)
    };
    #[cfg(not(feature = "test-support"))]
    let source_metadata_result = Metadata::from_open_file(&reader);
    let source_metadata = source_metadata_result
        .map_err(|source_error| error(Stage::InspectSourceEntry, source, destination, statistics, source_error))?;
    #[cfg(feature = "test-support")]
    {
        if crate::local::test_support_enabled("rooted-copy-destination-metadata") {
            return Err(error(
                Stage::PrepareDestination,
                source,
                destination,
                statistics,
                io::Error::from(ErrorKind::PermissionDenied),
            ));
        }
    }
    let destination_metadata = optional_metadata(root, destination)
        .map_err(|source_error| error(Stage::PrepareDestination, source, destination, statistics, source_error))?;
    if destination_metadata
        .as_ref()
        .is_some_and(|metadata| source_metadata.is_same_file(metadata))
    {
        return Err(error(
            Stage::PrepareDestination,
            source,
            destination,
            statistics,
            io::Error::new(
                ErrorKind::InvalidInput,
                "rooted copy source and destination identify the same file",
            ),
        ));
    }
    let destination_directory_requires_removal = destination_metadata
        .as_ref()
        .is_some_and(|metadata| metadata.kind() == EntryKind::Directory);
    if let Some(metadata) = destination_metadata {
        let action = decide_copy_destination(
            false,
            Some(metadata.kind() == EntryKind::Directory),
            options.conflict_policy(),
            options.type_conflict_policy(),
        );
        match action {
            Some(CopyDestinationAction::Skip) => {
                statistics.skipped = checked_add(statistics.skipped, 1, source, destination, statistics)?;
                return Ok(statistics);
            }
            Some(CopyDestinationAction::Replace) => {
                if metadata.kind() == EntryKind::File {
                    // The staged writer replaces regular files at commit.
                } else {
                    let remove_result = if metadata.kind() == EntryKind::Directory {
                        root.remove_tree(destination)
                    } else {
                        root.remove_file(destination)
                    };
                    remove_result.map_err(|source_error| {
                        error(Stage::PrepareDestination, source, destination, statistics, source_error)
                    })?;
                }
            }
            None => {
                return Err(error(
                    Stage::PrepareDestination,
                    source,
                    destination,
                    statistics,
                    io::Error::new(
                        ErrorKind::AlreadyExists,
                        "rooted copy destination has a conflicting entry",
                    ),
                ));
            }
            Some(CopyDestinationAction::Create | CopyDestinationAction::Merge) => {
                unreachable!("a file destination cannot require create or merge")
            }
        }
    }
    #[cfg(feature = "test-support")]
    {
        if crate::local::test_support_enabled("rooted-copy-writer-open") {
            return Err(error(
                Stage::PrepareDestination,
                source,
                destination,
                statistics,
                io::Error::from(ErrorKind::PermissionDenied),
            ));
        }
    }
    let mut writer = root
        .begin_atomic_write_with_options(destination, LocalAtomicWriteOptions::new().with_durability(durability))
        .map_err(|source_error| {
            let source_error = io::Error::new(source_error.kind(), source_error);
            error(Stage::PrepareDestination, source, destination, statistics, source_error)
        })?;
    #[cfg(feature = "test-support")]
    let copy_result = if crate::local::test_support_enabled("rooted-copy-file-contents-native") {
        Err(crate::local::test_fault_error())
    } else {
        budget.copy(&mut reader, &mut writer)
    };
    #[cfg(not(feature = "test-support"))]
    let copy_result = budget.copy(&mut reader, &mut writer);
    let bytes = copy_result
        .map_err(|source_error| error(Stage::CopyFileContents, source, destination, statistics, source_error))?;
    #[cfg(feature = "test-support")]
    let commit_result = if crate::local::test_support_enabled("rooted-copy-file-commit-native") {
        Err(crate::LocalAtomicWriteError::new(
            crate::LocalAtomicWriteStage::ReplaceDestination,
            destination.as_path().to_path_buf(),
            None,
            crate::LocalAtomicDestinationState::Unchanged,
            crate::local::test_fault_error(),
        ))
    } else {
        writer.commit_with_durability()
    };
    #[cfg(not(feature = "test-support"))]
    let commit_result = writer.commit_with_durability();
    let file_durable =
        commit_result.map_err(|source_error| rooted_commit_error(source, destination, statistics, source_error))?;
    statistics.files_durable &= file_durable;
    statistics.files = checked_add(statistics.files, 1, source, destination, statistics)?;
    statistics.bytes = checked_add(statistics.bytes, bytes, source, destination, statistics)?;
    if destination_metadata.is_some() {
        statistics.overwritten = checked_add(statistics.overwritten, 1, source, destination, statistics)?;
    }
    preserve_permissions(root, source, destination, source_metadata, options, statistics)?;
    if destination_directory_requires_removal {
        statistics.non_atomic_publication = true;
    }
    Ok(statistics)
}

/// Applies source permissions when requested by the caller.
pub(super) fn preserve_permissions(
    root: &Root,
    source: &Path,
    destination: &Path,
    metadata: Metadata,
    options: &Options,
    statistics: Statistics,
) -> Result<(), Error> {
    if options.preserves_permissions() {
        #[cfg(feature = "test-support")]
        if crate::local::test_support_enabled("rooted-copy-set-permissions") {
            return Err(error(
                Stage::PreservePermissions,
                source,
                destination,
                statistics,
                io::Error::from(ErrorKind::PermissionDenied),
            ));
        }
        root.set_permissions(destination, metadata.permissions())
            .map_err(|source_error| {
                error(
                    Stage::PreservePermissions,
                    source,
                    destination,
                    statistics,
                    source_error,
                )
            })?;
    }
    Ok(())
}

/// Converts a rooted atomic commit failure without discarding cleanup details.
fn rooted_commit_error(
    source: &Path,
    destination: &Path,
    statistics: Statistics,
    source_error: crate::LocalAtomicWriteError,
) -> Error {
    let (temporary_path, cleanup_error, source_error) = source_error.into_staging_parts();
    let source_kind = source_error.kind();
    let copy_error = error(
        Stage::CommitFile,
        source,
        destination,
        statistics,
        io::Error::new(source_kind, source_error),
    );
    match (temporary_path, cleanup_error) {
        (Some(temporary_path), Some(cleanup_error)) => {
            copy_error.with_staging_context(temporary_path, Some(cleanup_error))
        }
        _ => copy_error,
    }
}
