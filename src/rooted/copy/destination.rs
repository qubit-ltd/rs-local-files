// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Rooted copy destination policy and shared failure helpers.

use std::io;
use std::io::ErrorKind;

use super::EntryKind;
use super::Metadata;
use super::Path;
use super::Root;
use super::deletion_failure_hint;
use crate::LocalCopyDirError;
use crate::local::CopyBudget;
use crate::local::CopyDestinationAction;
use crate::local::LocalCopyConflictPolicy as ConflictPolicy;
use crate::local::LocalCopyDirError as Error;
use crate::local::LocalCopyDirOptions as Options;
use crate::local::LocalCopyDirStage as Stage;
use crate::local::LocalCopyDirStats as Statistics;
use crate::local::decide_copy_destination;

/// Prepares a destination directory under the selected conflict policies.
///
/// Returns true to traverse a created or merged directory, or false after
/// counting a skipped entry. Updates creation and overwrite statistics only
/// after the corresponding namespace operation succeeds. Native inspection,
/// removal, creation, policy-conflict, and counter-overflow failures retain
/// the statistics accumulated so far; earlier mutations are not rolled back.
///
/// # Parameters
///
/// * `root` - Open authority for destination inspection and mutation.
/// * `source` - Source path recorded in any structured failure.
/// * `destination` - Destination directory to prepare beneath `root`.
/// * `options` - Conflict policies that control merging, skipping, and
///   replacement.
/// * `statistics` - Mutable progress counters updated after successful
///   operations.
/// * `budget` - Deadline and resource budget for destination operations.
///
/// # Errors
///
/// Returns a structured copy error when destination inspection or a namespace
/// operation fails, when the configured policies reject the existing entry,
/// or when a statistics counter overflows. The error includes the operation
/// stage, source and destination paths, accumulated statistics, and underlying
/// cause.
pub(super) fn prepare_directory(
    root: &Root,
    source: &Path,
    destination: &Path,
    options: &Options,
    statistics: &mut Statistics,
    budget: &CopyBudget,
) -> Result<bool, Error> {
    budget.check_deadline().map_err(|source_error| {
        error(
            Stage::PrepareDestination,
            source,
            destination,
            *statistics,
            source_error,
        )
    })?;
    let destination_metadata = match optional_metadata(root, destination) {
        Ok(metadata) => metadata,
        Err(source_error) => {
            return Err(error(
                Stage::PrepareDestination,
                source,
                destination,
                *statistics,
                source_error,
            ));
        }
    };
    budget.check_deadline().map_err(|source_error| {
        error(
            Stage::PrepareDestination,
            source,
            destination,
            *statistics,
            source_error,
        )
    })?;
    match destination_metadata {
        None => {
            budget.check_deadline().map_err(|source_error| {
                error(
                    Stage::PrepareDestination,
                    source,
                    destination,
                    *statistics,
                    source_error,
                )
            })?;
            #[cfg(feature = "test-support")]
            {
                if crate::local::test_support_enabled("rooted-copy-directory-create") {
                    return Err(error(
                        Stage::PrepareDestination,
                        source,
                        destination,
                        *statistics,
                        io::Error::from(ErrorKind::PermissionDenied),
                    ));
                }
            }
            root.create_dir(destination).map_err(|source_error| {
                error(
                    Stage::PrepareDestination,
                    source,
                    destination,
                    *statistics,
                    source_error,
                )
            })?;
            statistics.directories = checked_add(statistics.directories, 1, source, destination, *statistics)?;
            Ok(true)
        }
        Some(metadata) => {
            let destination_is_directory = metadata.kind() == EntryKind::Directory;
            match decide_copy_destination(
                true,
                Some(destination_is_directory),
                options.conflict_policy(),
                options.type_conflict_policy(),
            ) {
                Some(CopyDestinationAction::Merge) => {
                    if options.conflict_policy() == ConflictPolicy::Overwrite {
                        statistics.overwritten =
                            checked_add(statistics.overwritten, 1, source, destination, *statistics)?;
                    }
                    Ok(true)
                }
                Some(CopyDestinationAction::Skip) => {
                    statistics.skipped = checked_add(statistics.skipped, 1, source, destination, *statistics)?;
                    Ok(false)
                }
                Some(CopyDestinationAction::Replace) => {
                    budget.check_deadline().map_err(|source_error| {
                        error(
                            Stage::PrepareDestination,
                            source,
                            destination,
                            *statistics,
                            source_error,
                        )
                    })?;
                    if destination_is_directory {
                        let (delete_options, started_at) = budget.deletion_options();
                        root.remove_tree_typed_with_options(destination, delete_options, started_at)
                            .map_err(|delete_error| {
                                let failure_hint = deletion_failure_hint(&delete_error);
                                let mut copy_error = error(
                                    Stage::PrepareDestination,
                                    source,
                                    destination,
                                    *statistics,
                                    delete_error.into_io_error(),
                                );
                                if let Some(failure_hint) = failure_hint {
                                    copy_error = copy_error.with_failure_state(failure_hint);
                                }
                                copy_error
                            })?;
                    } else {
                        root.remove_file(destination).map_err(|source_error| {
                            error(
                                Stage::PrepareDestination,
                                source,
                                destination,
                                *statistics,
                                source_error,
                            )
                        })?;
                    }
                    #[cfg(test)]
                    crate::tests::rooted::support::resolution_observation_tests::reach_deadline_boundary(
                        crate::tests::rooted::support::resolution_observation_tests::DeadlineBoundary::DestinationRemoved,
                    );
                    budget.check_deadline().map_err(|source_error| {
                        error(
                            Stage::PrepareDestination,
                            source,
                            destination,
                            *statistics,
                            source_error,
                        )
                        .with_failure_state(crate::LocalCopyFailureState::PartiallyPublished)
                    })?;
                    #[cfg(feature = "test-support")]
                    if crate::local::test_support_enabled("rooted-copy-replacement-directory-create") {
                        return Err(error(
                            Stage::PrepareDestination,
                            source,
                            destination,
                            *statistics,
                            crate::local::test_fault_error(),
                        )
                        .with_failure_state(crate::LocalCopyFailureState::PartiallyPublished));
                    }
                    root.create_dir(destination).map_err(|source_error| {
                        error(
                            Stage::PrepareDestination,
                            source,
                            destination,
                            *statistics,
                            source_error,
                        )
                        .with_failure_state(crate::LocalCopyFailureState::PartiallyPublished)
                    })?;
                    statistics.directories = checked_add(statistics.directories, 1, source, destination, *statistics)
                        .map_err(|failure| {
                        failure.with_failure_state(crate::LocalCopyFailureState::PartiallyPublished)
                    })?;
                    statistics.overwritten = checked_add(statistics.overwritten, 1, source, destination, *statistics)
                        .map_err(|failure| {
                        failure.with_failure_state(crate::LocalCopyFailureState::PartiallyPublished)
                    })?;
                    Ok(true)
                }
                Some(CopyDestinationAction::Create) => {
                    unreachable!("an observed destination cannot require creation")
                }
                None => Err(error(
                    Stage::PrepareDestination,
                    source,
                    destination,
                    *statistics,
                    io::Error::new(
                        ErrorKind::AlreadyExists,
                        "rooted copy destination has a conflicting entry",
                    ),
                )),
            }
        }
    }
}

/// Reads optional destination metadata without following the final link.
///
/// Returns `None` for a missing entry and propagates other native failures.
///
/// # Parameters
///
/// * `root` - Open authority used for descriptor-relative inspection.
/// * `path` - Destination path whose final link is not followed.
///
/// # Returns
///
/// `Some(metadata)` when the entry exists, or `None` when it is absent.
///
/// # Errors
///
/// Returns the native I/O error when inspecting the path fails for a reason
/// other than the entry being absent.
pub(super) fn optional_metadata(root: &Root, path: &Path) -> io::Result<Option<Metadata>> {
    match root.symlink_metadata(path) {
        Ok(metadata) => Ok(Some(metadata)),
        Err(source_error) if source_error.kind() == ErrorKind::NotFound => Ok(None),
        Err(source_error) => Err(source_error),
    }
}

/// Adds an exact statistic or reports overflow.
///
/// # Parameters
///
/// * `value` - Current statistic value.
/// * `addition` - Amount to add using checked `u64` arithmetic.
/// * `source` - Source path recorded in an overflow error.
/// * `destination` - Destination path recorded in an overflow error.
/// * `statistics` - Progress snapshot retained in an overflow error.
///
/// # Returns
///
/// The sum when it fits in `u64`.
///
/// # Errors
///
/// Returns a structured copy error if adding the value would overflow `u64`.
pub(super) fn checked_add(
    value: u64,
    addition: u64,
    source: &Path,
    destination: &Path,
    statistics: Statistics,
) -> Result<u64, Error> {
    #[cfg(feature = "test-support")]
    let result = if crate::local::test_support_enabled("rooted-copy-statistics-overflow") {
        None
    } else {
        value.checked_add(addition)
    };
    #[cfg(not(feature = "test-support"))]
    let result = value.checked_add(addition);
    result.ok_or_else(|| {
        error(
            Stage::UpdateStatistics,
            source,
            destination,
            statistics,
            io::Error::other("rooted copy statistics overflowed"),
        )
    })
}

/// Creates a structured rooted-copy error with the operation context and
/// progress snapshot needed by callers to identify the failed stage.
///
/// # Parameters
///
/// * `stage` - Copy stage at which the operation failed.
/// * `source` - Source path associated with the operation.
/// * `destination` - Destination path associated with the operation.
/// * `statistics` - Progress accumulated before the failure.
/// * `source_error` - Underlying I/O cause retained by the structured error.
///
/// # Returns
///
/// A structured error containing the supplied context and cause.
#[inline]
pub(super) fn error(
    stage: Stage,
    source: &Path,
    destination: &Path,
    statistics: Statistics,
    source_error: io::Error,
) -> Error {
    LocalCopyDirError::new(
        stage,
        source.as_path().to_path_buf(),
        destination.as_path().to_path_buf(),
        statistics,
        source_error,
    )
}

/// Creates the stable error used for unsupported source entry types.
#[must_use]
#[inline]
pub(super) fn unsupported_source_error() -> io::Error {
    io::Error::new(
        ErrorKind::Unsupported,
        "rooted copy supports only regular files, directories, and symbolic links",
    )
}

#[cfg(all(test, unix))]
mod tests {
    use std::fs;
    use std::time::Duration;
    use std::time::Instant;

    use super::Options;
    use super::Path;
    use super::Root;
    use super::Stage;
    use super::Statistics;
    use super::prepare_directory;
    use crate::LocalCopyFailureState;
    use crate::local::CopyBudget;
    use crate::local::LocalCopyTypeConflictPolicy;

    #[test]
    fn test_prepare_directory_deadlines_at_each_destination_boundary() {
        use crate::tests::rooted::support::resolution_observation_tests::DeadlineBoundary;
        use crate::tests::rooted::support::resolution_observation_tests::DeadlineClockGuard;

        let scratch = tempfile::tempdir().expect("fixture should exist");
        let root = Root::open(scratch.path()).expect("root authority should open");
        let source = Path::new("source").expect("source path should be valid");
        let options = Options::new();

        for (destination_name, checks) in [("initial", 0), ("after-metadata", 1), ("before-create", 2)] {
            let destination = Path::new(destination_name).expect("destination path should be valid");
            let budget = CopyBudget::new(
                crate::LocalCopyDirOptions::new()
                    .with_started_at(Instant::now())
                    .with_deadline(Duration::from_secs(60)),
            );
            let _clock = DeadlineClockGuard::expire_after(checks);
            let error = prepare_directory(
                &root,
                &source,
                &destination,
                &options,
                &mut Statistics::default(),
                &budget,
            )
            .expect_err("an expired destination deadline must stop mutation");
            assert_eq!(std::io::ErrorKind::TimedOut, error.error().kind());
            assert!(!scratch.path().join(destination_name).exists());
            drop(_clock);
        }

        fs::write(scratch.path().join("replace-me"), b"old").expect("old target should be created");
        let destination = Path::new("replace-me").expect("destination path should be valid");
        let budget = CopyBudget::new(
            crate::LocalCopyDirOptions::new()
                .with_started_at(Instant::now())
                .with_deadline(Duration::from_secs(60)),
        );
        let _clock = DeadlineClockGuard::expire_after(2);
        let error = prepare_directory(
            &root,
            &source,
            &destination,
            &Options::new().with_type_conflict(LocalCopyTypeConflictPolicy::Replace),
            &mut Statistics::default(),
            &budget,
        )
        .expect_err("an expired replacement deadline must preserve the destination");
        assert_eq!(std::io::ErrorKind::TimedOut, error.error().kind());
        assert_eq!(
            b"old".as_slice(),
            fs::read(scratch.path().join("replace-me")).expect("target remains")
        );
        drop(_clock);

        let _clock = DeadlineClockGuard::expire_at(DeadlineBoundary::DestinationRemoved);
        let mut statistics = Statistics::default();
        let error = prepare_directory(
            &root,
            &source,
            &destination,
            &Options::new().with_type_conflict(LocalCopyTypeConflictPolicy::Replace),
            &mut statistics,
            &budget,
        )
        .expect_err("deadline after removal must report partial publication");
        assert_eq!(std::io::ErrorKind::TimedOut, error.error().kind());
        assert_eq!(
            Some(crate::LocalCopyFailureState::PartiallyPublished),
            error.failure_state()
        );
        assert!(!scratch.path().join("replace-me").exists());
    }

    #[test]
    fn test_prepare_directory_creates_missing_destination_and_updates_statistics() {
        let unsupported = super::unsupported_source_error();
        assert_eq!(std::io::ErrorKind::Unsupported, unsupported.kind());
        assert!(unsupported.to_string().contains("supports only regular files"));

        let scratch = tempfile::tempdir().expect("fixture should exist");
        let root = Root::open(scratch.path()).expect("root authority should open");
        let source = Path::new("source").expect("source path should be valid");
        let destination = Path::new("target").expect("destination path should be valid");
        let mut statistics = Statistics::default();
        let budget = CopyBudget::new(crate::LocalCopyDirOptions::new());

        assert!(
            prepare_directory(&root, &source, &destination, &Options::new(), &mut statistics, &budget,)
                .expect("missing destination should be created")
        );

        assert!(scratch.path().join("target").is_dir());
        assert_eq!(1, statistics.directories);
        assert_eq!(0, statistics.skipped);

        let faulted = Path::new("faulted").expect("faulted path should be valid");
        let guard = crate::tests::native_io_fault_tests::NativeIoFaultGuard::fail_on(
            crate::tests::native_io_fault_tests::NativeIoPoint::EntryMetadata,
            1,
        );
        let error = prepare_directory(
            &root,
            &source,
            &faulted,
            &Options::new(),
            &mut Statistics::default(),
            &budget,
        )
        .expect_err("metadata errors must be retained as destination preparation errors");
        assert!(guard.fired());
        assert_eq!(Stage::PrepareDestination, error.stage());
        assert_eq!(faulted.as_path(), error.destination_path());
        assert_eq!(std::io::ErrorKind::Other, error.kind());
        assert!(!scratch.path().join("faulted").exists());

        let missing_parent = Path::new("missing-parent/child").expect("nested path should be valid");
        let error = prepare_directory(
            &root,
            &source,
            &missing_parent,
            &Options::new(),
            &mut Statistics::default(),
            &budget,
        )
        .expect_err("destination creation must report a missing parent");
        assert_eq!(Stage::PrepareDestination, error.stage());
        assert_eq!(missing_parent.as_path(), error.destination_path());
        assert_eq!(std::io::ErrorKind::NotFound, error.kind());
    }

    #[test]
    fn test_prepare_directory_merges_skips_and_rejects_conflicts_by_policy() {
        let scratch = tempfile::tempdir().expect("fixture should exist");
        std::fs::create_dir(scratch.path().join("target")).expect("existing directory should be created");
        std::fs::write(scratch.path().join("file"), b"existing file").expect("file should be created");
        let root = Root::open(scratch.path()).expect("root authority should open");
        let source = Path::new("source").expect("source path should be valid");
        let destination = Path::new("target").expect("destination path should be valid");
        let budget = CopyBudget::new(crate::LocalCopyDirOptions::new());

        let mut merged = Statistics::default();
        assert!(
            prepare_directory(&root, &source, &destination, &Options::new(), &mut merged, &budget,)
                .expect("existing directory should merge")
        );
        assert_eq!(0, merged.directories);
        assert_eq!(0, merged.skipped);

        let mut skipped = Statistics::default();
        let file_destination = Path::new("file").expect("file destination should be valid");
        assert!(
            !prepare_directory(
                &root,
                &source,
                &file_destination,
                &Options::new().with_type_conflict(LocalCopyTypeConflictPolicy::Skip),
                &mut skipped,
                &budget,
            )
            .expect("type-conflict skip should retain the existing file")
        );
        assert_eq!(1, skipped.skipped);

        let error = prepare_directory(
            &root,
            &source,
            &file_destination,
            &Options::new().with_type_conflict(LocalCopyTypeConflictPolicy::Fail),
            &mut Statistics::default(),
            &budget,
        )
        .expect_err("directory and file type conflict should be rejected");
        assert_eq!(std::io::ErrorKind::AlreadyExists, error.kind());
        assert_eq!(Stage::PrepareDestination, error.stage());
        assert_eq!(file_destination.as_path(), error.destination_path());
    }

    #[test]
    fn test_replacement_statistics_overflow_retains_published_type_change_and_progress() {
        for overflow_directories in [true, false] {
            let scratch = tempfile::tempdir().expect("fixture should exist");
            fs::create_dir(scratch.path().join("source")).expect("source directory should exist");
            fs::write(scratch.path().join("source/payload"), b"protected source").expect("source payload should exist");
            fs::write(scratch.path().join("target"), b"old target").expect("conflicting file should exist");
            fs::write(scratch.path().join("user-data"), b"protected sibling").expect("user data should exist");
            let root = Root::open(scratch.path()).expect("root authority should open");
            let source = Path::new("source").expect("source should be a valid relative path");
            let destination = Path::new("target").expect("target should be a valid relative path");
            let options = Options::new().with_type_conflict(LocalCopyTypeConflictPolicy::Replace);
            let mut statistics = Statistics {
                directories: if overflow_directories { u64::MAX } else { 0 },
                overwritten: if overflow_directories { 0 } else { u64::MAX },
                ..Statistics::default()
            };
            let budget = CopyBudget::new(crate::LocalCopyDirOptions::new());
            let error = prepare_directory(&root, &source, &destination, &options, &mut statistics, &budget)
                .expect_err("counter overflow after replacement must report a partial publication");
            assert_eq!(error.stage(), Stage::UpdateStatistics);
            assert_eq!(error.source_path(), std::path::Path::new("source"));
            assert_eq!(error.destination_path(), std::path::Path::new("target"));
            assert_eq!(error.failure_state(), Some(LocalCopyFailureState::PartiallyPublished));
            assert_eq!(error.kind(), std::io::ErrorKind::Other);
            assert_eq!(error.error().to_string(), "rooted copy statistics overflowed");
            assert_eq!(error.stats(), &statistics);
            assert_eq!(statistics.directories, if overflow_directories { u64::MAX } else { 1 });
            assert_eq!(statistics.overwritten, if overflow_directories { 0 } else { u64::MAX });
            assert!(
                scratch.path().join("target").is_dir(),
                "the completed type change must remain visible"
            );
            assert_eq!(
                fs::read_dir(scratch.path().join("target"))
                    .expect("prepared target should be empty")
                    .count(),
                0
            );
            assert_eq!(
                fs::read(scratch.path().join("source/payload")).expect("counter failure must preserve source"),
                b"protected source"
            );
            assert_eq!(
                fs::read(scratch.path().join("user-data")).expect("counter failure must preserve siblings"),
                b"protected sibling"
            );
            assert!(
                prepare_directory(
                    &root,
                    &source,
                    &destination,
                    &options,
                    &mut Statistics::default(),
                    &budget,
                )
                .expect("retry should recognize the already prepared directory")
            );
        }
    }
}
