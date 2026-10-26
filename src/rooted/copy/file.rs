// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Rooted regular-file copy and staged publication.

use std::io;
use std::io::ErrorKind;
#[cfg(feature = "test-support")]
use std::io::Write;

use super::EntryKind;
use super::Metadata;
use super::Path;
use super::Root;
use super::deletion_failure_hint;
use super::destination::checked_add;
use super::destination::error;
use super::destination::optional_metadata;
use super::mark_partial_after_removal;
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
///
/// Applies `options` to conflicts and permission preservation, charges copied
/// bytes and deadline checks to `budget`, and requests `durability` from the
/// staged writer. Returns updated statistics, including skipped entries.
/// Errors retain the statistics accumulated when the failure occurs and any
/// available staging cleanup error. Destination preparation can remove an
/// incompatible entry before a later failure, and permission preservation can
/// fail after the copied content has been published.
///
/// # Errors
///
/// Returns a structured error for deadline, source or destination inspection,
/// conflict policy, staging, copy, publication, permission, or statistics
/// failures. The error records the failing stage and accumulated statistics.
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
    let mut destination_removed = false;
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
                    if metadata.kind() == EntryKind::Directory {
                        root.remove_tree_typed(destination).map_err(|delete_error| {
                            let failure_hint = deletion_failure_hint(&delete_error);
                            let mut copy_error = error(
                                Stage::PrepareDestination,
                                source,
                                destination,
                                statistics,
                                delete_error.into_io_error(),
                            );
                            if let Some(failure_hint) = failure_hint {
                                copy_error = copy_error.with_failure_state(failure_hint);
                            }
                            copy_error
                        })?;
                    } else {
                        root.remove_file(destination).map_err(|source_error| {
                            error(Stage::PrepareDestination, source, destination, statistics, source_error)
                        })?;
                    }
                    destination_removed = true;
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
    inject_race_destination(root, destination, "rooted-copy-race-before-writer-open")
        .map_err(|source_error| error(Stage::PrepareDestination, source, destination, statistics, source_error))?;
    #[cfg(feature = "test-support")]
    {
        if crate::local::test_support_enabled("rooted-copy-writer-open") {
            return Err(mark_partial_after_removal(
                error(
                    Stage::PrepareDestination,
                    source,
                    destination,
                    statistics,
                    io::Error::from(ErrorKind::PermissionDenied),
                ),
                destination_removed,
            ));
        }
    }
    let mut writer_options = LocalAtomicWriteOptions::new().with_durability(durability);
    if options.conflict_policy() != crate::local::LocalCopyConflictPolicy::Overwrite {
        writer_options = writer_options.with_create_new();
    }
    let mut writer = match root.begin_atomic_write_with_options(destination, writer_options) {
        Ok(writer) => writer,
        Err(source_error) => {
            if !destination_removed
                && should_skip_conflict_race(
                    root,
                    destination,
                    options,
                    crate::LocalAtomicWriteStage::InspectDestination,
                    &source_error,
                )
            {
                statistics.skipped = checked_add(statistics.skipped, 1, source, destination, statistics)?;
                return Ok(statistics);
            }
            return Err(rooted_open_error(
                source,
                destination,
                statistics,
                destination_removed,
                source_error,
            ));
        }
    };
    if options.preserves_permissions() {
        writer.set_copy_permissions(source_metadata.permissions());
    }
    #[cfg(feature = "test-support")]
    let copy_result = if crate::local::test_support_enabled("rooted-copy-file-contents-native") {
        Err(crate::local::test_fault_error())
    } else {
        budget.copy(&mut reader, &mut writer)
    };
    #[cfg(not(feature = "test-support"))]
    let copy_result = budget.copy(&mut reader, &mut writer);
    let bytes = copy_result.map_err(|source_error| {
        mark_partial_after_removal(
            error(Stage::CopyFileContents, source, destination, statistics, source_error),
            destination_removed,
        )
    })?;
    #[cfg(feature = "test-support")]
    inject_race_destination(root, destination, "rooted-copy-race-before-writer-commit")
        .map_err(|source_error| error(Stage::CommitFile, source, destination, statistics, source_error))?;
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
    let file_durable = match commit_result {
        Ok(file_durable) => file_durable,
        Err(source_error) => {
            if !destination_removed
                && should_skip_conflict_race(
                    root,
                    destination,
                    options,
                    crate::LocalAtomicWriteStage::ReplaceDestination,
                    &source_error,
                )
            {
                statistics.skipped = checked_add(statistics.skipped, 1, source, destination, statistics)?;
                return Ok(statistics);
            }
            return Err(mark_partial_after_removal(
                rooted_commit_error(source, destination, statistics, source_error),
                destination_removed,
            ));
        }
    };
    statistics.files_durable &= file_durable;
    statistics.files = checked_add(statistics.files, 1, source, destination, statistics)?;
    statistics.bytes = checked_add(statistics.bytes, bytes, source, destination, statistics)?;
    if destination_metadata.is_some() {
        statistics.overwritten = checked_add(statistics.overwritten, 1, source, destination, statistics)?;
    }
    if destination_directory_requires_removal {
        statistics.non_atomic_publication = true;
    }
    Ok(statistics)
}

/// Creates a deterministic competing destination through the retained Root
/// authority at one selected copy boundary.
#[cfg(feature = "test-support")]
fn inject_race_destination(root: &Root, destination: &Path, selector: &str) -> io::Result<()> {
    let selected = crate::local::take_test_support(selector)
        || (selector == "rooted-copy-race-before-writer-commit"
            && crate::local::test_support_enabled("rooted-copy-install-cleanup"));
    if !selected {
        return Ok(());
    }
    if optional_metadata(root, destination)?.is_some() {
        return Ok(());
    }
    let options = crate::write::OpenOptions::new(crate::write::Mode::CreateNew);
    let mut sentinel = root.open_writer(destination, &options)?;
    sentinel.write_all(b"concurrent sentinel")
}

/// Returns true only when a staged-writer conflict proves an unchanged
/// destination that the configured copy policies explicitly allow skipping.
fn should_skip_conflict_race(
    root: &Root,
    destination: &Path,
    options: &Options,
    expected_stage: crate::LocalAtomicWriteStage,
    source_error: &crate::LocalAtomicWriteError,
) -> bool {
    if source_error.stage() != expected_stage
        || source_error.kind() != ErrorKind::AlreadyExists
        || source_error.destination_state() != crate::LocalAtomicDestinationState::Unchanged
        || source_error.cleanup_error().is_some()
        || source_error.parent_sync_error().is_some()
    {
        return false;
    }
    let Ok(Some(metadata)) = optional_metadata(root, destination) else {
        return false;
    };
    decide_copy_destination(
        false,
        Some(metadata.kind() == EntryKind::Directory),
        options.conflict_policy(),
        options.type_conflict_policy(),
    ) == Some(CopyDestinationAction::Skip)
}

/// Converts a rooted atomic-writer initialization failure without discarding
/// staging cleanup details.
fn rooted_open_error(
    source: &Path,
    destination: &Path,
    statistics: Statistics,
    destination_removed: bool,
    source_error: crate::LocalAtomicWriteError,
) -> Error {
    let (temporary_path, cleanup_error, source_error) = source_error.into_staging_parts();
    let source_kind = source_error.kind();
    let copy_error = error(
        Stage::PrepareDestination,
        source,
        destination,
        statistics,
        io::Error::new(source_kind, source_error),
    );
    let copy_error = match temporary_path {
        Some(temporary_path) => copy_error.with_staging_context(temporary_path, cleanup_error),
        None => match cleanup_error {
            Some(cleanup_error) => copy_error.with_cleanup_error(cleanup_error),
            None => copy_error,
        },
    };
    mark_partial_after_removal(copy_error, destination_removed)
}

/// Applies source permissions when requested by the caller.
///
/// Does nothing when preservation is disabled. A native permission failure
/// returns the `PreservePermissions` stage and supplied statistics; it does
/// not undo content already copied to the destination.
///
/// # Errors
///
/// Returns a structured error if updating destination permissions fails.
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
///
/// Retains the primary I/O error and attaches the staging path and cleanup
/// error when the failed commit reports them.
fn rooted_commit_error(
    source: &Path,
    destination: &Path,
    statistics: Statistics,
    source_error: crate::LocalAtomicWriteError,
) -> Error {
    let prepublication_metadata_failure = source_error.stage()
        == crate::LocalAtomicWriteStage::ApplyDestinationMetadata
        && source_error.destination_state() == crate::LocalAtomicDestinationState::Unchanged;
    let prepublication_destination_conflict = source_error.stage() == crate::LocalAtomicWriteStage::ReplaceDestination
        && source_error.kind() == ErrorKind::AlreadyExists
        && source_error.destination_state() == crate::LocalAtomicDestinationState::Unchanged
        && source_error.cleanup_error().is_none()
        && source_error.parent_sync_error().is_none();
    let (temporary_path, cleanup_error, source_error) = source_error.into_staging_parts();
    let source_kind = source_error.kind();
    let copy_error = error(
        if prepublication_metadata_failure || prepublication_destination_conflict {
            Stage::PrepareDestination
        } else {
            Stage::CommitFile
        },
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

#[cfg(test)]
mod tests {
    use std::io;
    use std::path::Path;
    use std::path::PathBuf;
    use std::time::Duration;

    use super::copy_file;
    use super::rooted_commit_error;
    use super::should_skip_conflict_race;

    #[test]
    fn conflict_race_is_skipped_only_for_a_clean_unchanged_allowed_conflict() {
        let directory = tempfile::tempdir().expect("temporary root should be created");
        let root = super::super::Root::open(directory.path()).expect("root authority should open");
        let destination =
            crate::local::LocalRelativePath::new(Path::new("destination")).expect("valid relative destination");
        std::fs::write(directory.path().join("destination"), b"sentinel").expect("concurrent destination should exist");
        let options =
            crate::local::LocalCopyDirOptions::new().with_conflict(crate::local::LocalCopyConflictPolicy::Skip);
        let conflict = || {
            crate::LocalAtomicWriteError::new(
                crate::LocalAtomicWriteStage::ReplaceDestination,
                PathBuf::from("destination"),
                None,
                crate::LocalAtomicDestinationState::Unchanged,
                io::Error::from(io::ErrorKind::AlreadyExists),
            )
        };

        assert!(should_skip_conflict_race(
            &root,
            &destination,
            &options,
            crate::LocalAtomicWriteStage::ReplaceDestination,
            &conflict(),
        ));
        assert!(!should_skip_conflict_race(
            &root,
            &destination,
            &options,
            crate::LocalAtomicWriteStage::InspectDestination,
            &conflict(),
        ));
        assert!(!should_skip_conflict_race(
            &root,
            &destination,
            &options,
            crate::LocalAtomicWriteStage::ReplaceDestination,
            &conflict().with_cleanup_error(Some(io::Error::from(io::ErrorKind::Other))),
        ));
        assert!(!should_skip_conflict_race(
            &root,
            &destination,
            &options,
            crate::LocalAtomicWriteStage::ReplaceDestination,
            &conflict().with_parent_sync_error(Some(io::Error::from(io::ErrorKind::Other))),
        ));
    }

    #[test]
    fn rooted_commit_error_maps_metadata_and_replacement_failures() {
        let source = crate::local::LocalRelativePath::new(Path::new("source")).expect("valid relative source");
        let destination =
            crate::local::LocalRelativePath::new(Path::new("destination")).expect("valid relative destination");
        let metadata_error = rooted_commit_error(
            &source,
            &destination,
            super::super::Statistics::default(),
            crate::LocalAtomicWriteError::new(
                crate::LocalAtomicWriteStage::ApplyDestinationMetadata,
                PathBuf::from("destination"),
                Some(PathBuf::from("staging")),
                crate::LocalAtomicDestinationState::Unchanged,
                io::Error::from(io::ErrorKind::PermissionDenied),
            )
            .with_cleanup_error(Some(io::Error::from(io::ErrorKind::Other))),
        );
        assert_eq!(super::super::Stage::PrepareDestination, metadata_error.stage());
        assert_eq!(Some(Path::new("staging")), metadata_error.temporary_path());
        assert_eq!(
            Some(io::ErrorKind::Other),
            metadata_error.cleanup_error().map(io::Error::kind)
        );
        assert_eq!(io::ErrorKind::PermissionDenied, metadata_error.error().kind());

        let replacement_error = rooted_commit_error(
            &source,
            &destination,
            super::super::Statistics::default(),
            crate::LocalAtomicWriteError::new(
                crate::LocalAtomicWriteStage::ReplaceDestination,
                PathBuf::from("destination"),
                None,
                crate::LocalAtomicDestinationState::Indeterminate,
                io::Error::from(io::ErrorKind::Other),
            ),
        );
        assert_eq!(super::super::Stage::CommitFile, replacement_error.stage());
        assert_eq!(io::ErrorKind::Other, replacement_error.error().kind());
    }

    #[test]
    fn rooted_file_copy_deadline_fails_before_source_inspection() {
        let directory = tempfile::tempdir().expect("temporary root should be created");
        let root = super::super::Root::open(directory.path()).expect("root authority should open");
        let source = crate::local::LocalRelativePath::new(Path::new("source")).expect("valid relative source");
        let destination =
            crate::local::LocalRelativePath::new(Path::new("destination")).expect("valid relative destination");
        let options = crate::local::LocalCopyDirOptions::new().with_deadline(Duration::ZERO);
        let mut budget = crate::local::CopyBudget::new(options);

        let error = copy_file(
            &root,
            &source,
            &destination,
            &options,
            crate::LocalDurabilityRequirement::NotRequired,
            super::super::Statistics::default(),
            &mut budget,
        )
        .expect_err("expired deadline must stop before source inspection");

        assert_eq!(super::super::Stage::InspectSourceEntry, error.stage());
        assert_eq!(io::ErrorKind::TimedOut, error.error().kind());
        assert!(!directory.path().join("destination").exists());
    }
}
