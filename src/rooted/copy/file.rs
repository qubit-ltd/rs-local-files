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
use crate::local::CopyMetadataEffect;
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
/// available staging cleanup error. Incompatible destinations remain in place
/// until staging completes; removal or later installation can still partially
/// publish the replacement.
///
/// # Parameters
///
/// - `root`: Supplies the filesystem authority used for all source and
///   destination operations.
/// - `source` and `destination`: Identify the rooted file paths to copy
///   between.
/// - `options`: Controls conflict handling and permission preservation.
/// - `durability`: Selects the durability requirement for staged publication.
/// - `statistics`: Carries counters accumulated before this file is copied.
/// - `budget`: Tracks byte limits and deadlines for this copy operation.
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
    let mut delayed_replacement = false;
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
                    delayed_replacement = true;
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
    let mut writer = match if delayed_replacement {
        root.begin_copy_replacement_staging(destination, writer_options)
    } else {
        root.begin_atomic_write_with_options(destination, writer_options)
    } {
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
    let copy_result = if crate::local::test_support_enabled("rooted-copy-file-contents-native")
        || crate::local::test_support_enabled("rooted-copy-file-contents-cleanup")
    {
        Err(crate::local::test_fault_error())
    } else {
        budget.copy(&mut reader, &mut writer)
    };
    #[cfg(not(feature = "test-support"))]
    let copy_result = budget.copy(&mut reader, &mut writer);
    let bytes = match copy_result {
        Ok(bytes) => bytes,
        Err(source_error) => {
            let mut failure = error(Stage::CopyFileContents, source, destination, statistics, source_error);
            if let Err(cleanup) = writer.abort() {
                failure = failure.with_staging_context(
                    writer.staging_path().to_path_buf(),
                    Some(cleanup.into_staging_parts().2),
                );
            }
            let failure = mark_partial_after_removal(failure, destination_removed);
            return Err(if delayed_replacement {
                failure.with_failure_state(crate::LocalCopyFailureState::Unchanged)
            } else {
                failure
            });
        }
    };
    let mut completed_statistics = match completed_file_statistics(
        statistics,
        bytes,
        destination_metadata.is_some(),
        destination_directory_requires_removal,
        source,
        destination,
    ) {
        Ok(statistics) => statistics,
        Err(mut failure) => {
            if let Err(cleanup) = writer.abort() {
                failure = failure.with_staging_context(
                    writer.staging_path().to_path_buf(),
                    Some(cleanup.into_staging_parts().2),
                );
            }
            return Err(mark_partial_after_removal(failure, destination_removed));
        }
    };
    if delayed_replacement {
        #[cfg(feature = "test-support")]
        if crate::local::take_test_support("rooted-copy-replacement-before-remove") {
            let displaced = Path::new("rooted-copy-displaced-target").expect("static rooted path is valid");
            let race_result = root.rename_without_replacing(destination, &displaced).and_then(|()| {
                let create = crate::write::OpenOptions::new(crate::write::Mode::CreateNew);
                let mut sentinel = root.open_writer(destination, &create)?;
                sentinel.write_all(b"replacement sentinel")
            });
            let source_error = race_result.err().unwrap_or_else(crate::local::test_fault_error);
            let mut failure = error(Stage::PrepareDestination, source, destination, statistics, source_error);
            if let Err(cleanup) = writer.abort() {
                failure = failure.with_staging_context(
                    writer.staging_path().to_path_buf(),
                    Some(cleanup.into_staging_parts().2),
                );
            }
            return Err(failure.with_failure_state(crate::LocalCopyFailureState::Unchanged));
        }
        match optional_metadata(root, destination) {
            Ok(Some(current))
                if destination_metadata
                    .is_some_and(|original| current.kind() == original.kind() && current.is_same_file(&original)) => {}
            Ok(None) => {}
            Ok(Some(_)) => {
                let mut failure = error(
                    Stage::PrepareDestination,
                    source,
                    destination,
                    statistics,
                    io::Error::new(
                        ErrorKind::AlreadyExists,
                        "rooted copy destination changed during staging",
                    ),
                );
                if let Err(cleanup) = writer.abort() {
                    failure = failure.with_staging_context(
                        writer.staging_path().to_path_buf(),
                        Some(cleanup.into_staging_parts().2),
                    );
                }
                return Err(failure.with_failure_state(crate::LocalCopyFailureState::Unchanged));
            }
            Err(source_error) => {
                let mut failure = error(Stage::PrepareDestination, source, destination, statistics, source_error);
                if let Err(cleanup) = writer.abort() {
                    failure = failure.with_staging_context(
                        writer.staging_path().to_path_buf(),
                        Some(cleanup.into_staging_parts().2),
                    );
                }
                return Err(failure.with_failure_state(crate::LocalCopyFailureState::Unchanged));
            }
        }
        if destination_metadata.is_some() {
            let removal = if destination_metadata.is_some_and(|metadata| metadata.kind() == EntryKind::Directory) {
                let (delete_options, started_at) = budget.deletion_options();
                root.remove_tree_typed_with_options(destination, delete_options, started_at)
                    .map_err(|delete_error| {
                        let hint = deletion_failure_hint(&delete_error);
                        (delete_error.into_io_error(), hint)
                    })
            } else {
                root.remove_file(destination)
                    .map_err(|source_error| (source_error, None))
            };
            if let Err((source_error, hint)) = removal {
                let mut failure = error(Stage::PrepareDestination, source, destination, statistics, source_error);
                if let Some(hint) = hint {
                    failure = failure.with_failure_state(hint);
                }
                if let Err(cleanup) = writer.abort() {
                    failure = failure.with_staging_context(
                        writer.staging_path().to_path_buf(),
                        Some(cleanup.into_staging_parts().2),
                    );
                }
                return Err(failure);
            }
            destination_removed = true;
        }
    }
    if let Err(source_error) = budget.check_deadline() {
        let mut failure = error(Stage::CommitFile, source, destination, statistics, source_error);
        if let Err(cleanup) = writer.abort() {
            failure = failure.with_staging_context(
                writer.staging_path().to_path_buf(),
                Some(cleanup.into_staging_parts().2),
            );
        }
        return Err(mark_partial_after_removal(failure, destination_removed));
    }
    #[cfg(feature = "test-support")]
    if let Err(source_error) = inject_race_destination(root, destination, "rooted-copy-race-before-writer-commit") {
        let mut failure = error(Stage::CommitFile, source, destination, statistics, source_error);
        if let Err(cleanup) = writer.abort() {
            failure = failure.with_staging_context(
                writer.staging_path().to_path_buf(),
                Some(cleanup.into_staging_parts().2),
            );
        }
        return Err(mark_partial_after_removal(failure, destination_removed));
    }
    #[cfg(feature = "test-support")]
    let commit_result = if crate::local::test_support_enabled("rooted-copy-file-commit-native") {
        let staging_path = writer.staging_path().to_path_buf();
        let mut error = crate::LocalAtomicWriteError::new(
            crate::LocalAtomicWriteStage::ReplaceDestination,
            destination.as_path().to_path_buf(),
            Some(staging_path),
            crate::LocalAtomicDestinationState::Unchanged,
            crate::local::test_fault_error(),
        );
        if let Err(cleanup) = writer.abort() {
            error = error.with_cleanup_error(Some(cleanup.into_staging_parts().2));
        }
        Err(error)
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
            let failure = rooted_commit_error(source, destination, statistics, completed_statistics, source_error);
            return Err(mark_partial_after_removal(failure, destination_removed));
        }
    };
    completed_statistics.files_durable &= file_durable;
    Ok(completed_statistics)
}

/// Computes counters for a completed file before publication is attempted.
///
/// # Parameters
///
/// - `statistics`: Counters accumulated before this file is published.
/// - `bytes`: Number of bytes copied from the source.
/// - `overwritten`: Whether a destination entry existed before the copy.
/// - `non_atomic_publication`: Whether replacement requires removing the old
///   destination before installation.
/// - `source` and `destination`: Paths used to report counter overflow.
fn completed_file_statistics(
    statistics: Statistics,
    bytes: u64,
    overwritten: bool,
    non_atomic_publication: bool,
    source: &Path,
    destination: &Path,
) -> Result<Statistics, Error> {
    let files = checked_add(statistics.files, 1, source, destination, statistics)?;
    let bytes = checked_add(statistics.bytes, bytes, source, destination, statistics)?;
    let overwritten = if overwritten {
        checked_add(statistics.overwritten, 1, source, destination, statistics)?
    } else {
        statistics.overwritten
    };
    Ok(Statistics {
        files,
        bytes,
        overwritten,
        non_atomic_publication: statistics.non_atomic_publication || non_atomic_publication,
        ..statistics
    })
}

/// Creates a deterministic competing destination through the retained Root
/// authority at one selected copy boundary.
///
/// # Parameters
///
/// - `root`: Supplies the rooted filesystem authority.
/// - `destination`: Identifies the path at which to create a competing entry.
/// - `selector`: Names the enabled test-support injection point.
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
///
/// # Parameters
///
/// - `root`: Supplies authority to inspect the destination safely.
/// - `destination`: Identifies the path whose current conflict is checked.
/// - `options`: Supplies the configured conflict policies.
/// - `expected_stage`: Restricts which writer failure stage may be skipped.
/// - `source_error`: Provides the staged writer's failure and cleanup state.
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
///
/// # Parameters
///
/// - `source` and `destination`: Paths associated with the failed copy.
/// - `statistics`: Counters accumulated before publication.
/// - `destination_removed`: Whether failure may have left the destination
///   absent.
/// - `source_error`: The writer initialization failure and staging context.
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
/// # Parameters
///
/// - `root`: Supplies authority to update destination metadata.
/// - `source` and `destination`: Identify the copied file paths.
/// - `metadata`: Supplies the source permissions to apply.
/// - `options`: Determines whether permission preservation is enabled.
/// - `statistics`: Tracks counters and metadata effects for the operation.
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
    statistics: &mut Statistics,
) -> Result<(), Error> {
    if options.preserves_permissions() {
        let previous_effect = statistics.metadata_effect;
        statistics.metadata_effect = CopyMetadataEffect::Indeterminate;
        #[cfg(feature = "test-support")]
        if crate::local::test_support_enabled("rooted-copy-set-permissions") {
            return Err(error(
                Stage::PreservePermissions,
                source,
                destination,
                *statistics,
                io::Error::from(ErrorKind::PermissionDenied),
            ));
        }
        if let Err(source_error) = root.set_permissions(destination, metadata.permissions()) {
            return Err(error(
                Stage::PreservePermissions,
                source,
                destination,
                *statistics,
                source_error,
            ));
        }
        if previous_effect != CopyMetadataEffect::Indeterminate {
            statistics.metadata_effect = CopyMetadataEffect::Applied;
        }
    }
    Ok(())
}

/// Converts a rooted atomic commit failure without discarding cleanup details.
///
/// Retains the primary I/O error and attaches the staging path and cleanup
/// error when the failed commit reports them.
///
/// # Parameters
///
/// - `source` and `destination`: Paths associated with the failed copy.
/// - `statistics`: Counters accumulated before publication.
/// - `completed_statistics`: Counters reflecting the file if installation
///   already occurred before a later failure.
/// - `source_error`: The commit failure and any staging cleanup context.
fn rooted_commit_error(
    source: &Path,
    destination: &Path,
    statistics: Statistics,
    completed_statistics: Statistics,
    source_error: crate::LocalAtomicWriteError,
) -> Error {
    let installed_before_sync_failure = source_error.stage() == crate::LocalAtomicWriteStage::SyncParent
        && source_error.destination_state() == crate::LocalAtomicDestinationState::Replaced;
    let atomic_destination_state = source_error.destination_state();
    let (temporary_path, cleanup_error, source_error) = source_error.into_staging_parts();
    let source_kind = source_error.kind();
    let copy_error = error(
        Stage::CommitFile,
        source,
        destination,
        if installed_before_sync_failure {
            completed_statistics
        } else {
            statistics
        },
        io::Error::new(source_kind, source_error),
    );
    let copy_error = match (temporary_path, cleanup_error) {
        (Some(temporary_path), Some(cleanup_error)) => {
            copy_error.with_staging_context(temporary_path, Some(cleanup_error))
        }
        (None, Some(cleanup_error)) => copy_error.with_cleanup_error(cleanup_error),
        _ => copy_error,
    };
    let copy_error = copy_error.with_atomic_destination_state(atomic_destination_state);
    if installed_before_sync_failure {
        copy_error.with_failure_state(crate::LocalCopyFailureState::Published)
    } else {
        copy_error
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
    fn test_conflict_race_is_skipped_only_for_a_clean_unchanged_allowed_conflict() {
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
    fn test_rooted_commit_error_maps_metadata_and_replacement_failures() {
        let source = crate::local::LocalRelativePath::new(Path::new("source")).expect("valid relative source");
        let destination =
            crate::local::LocalRelativePath::new(Path::new("destination")).expect("valid relative destination");
        let metadata_error = rooted_commit_error(
            &source,
            &destination,
            super::super::Statistics::default(),
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
        assert_eq!(super::super::Stage::CommitFile, metadata_error.stage());
        assert_eq!(None, metadata_error.failure_state());
        assert_eq!(
            Some(crate::LocalAtomicDestinationState::Unchanged),
            metadata_error.atomic_destination_state()
        );
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
        assert_eq!(None, replacement_error.failure_state());
        assert_eq!(io::ErrorKind::Other, replacement_error.error().kind());

        let installed_statistics = super::super::Statistics {
            files: 1,
            bytes: 7,
            overwritten: 1,
            ..super::super::Statistics::default()
        };
        let sync_error = rooted_commit_error(
            &source,
            &destination,
            super::super::Statistics::default(),
            installed_statistics,
            crate::LocalAtomicWriteError::new(
                crate::LocalAtomicWriteStage::SyncParent,
                PathBuf::from("destination"),
                None,
                crate::LocalAtomicDestinationState::Replaced,
                io::Error::from(io::ErrorKind::Other),
            ),
        );
        assert_eq!(super::super::Stage::CommitFile, sync_error.stage());
        assert_eq!(
            Some(crate::LocalCopyFailureState::Published),
            sync_error.failure_state()
        );
        assert_eq!(1, sync_error.stats().files);
        assert_eq!(7, sync_error.stats().bytes);
    }

    #[test]
    fn test_rooted_file_copy_deadline_fails_before_source_inspection() {
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
