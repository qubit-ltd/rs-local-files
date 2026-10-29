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
) -> Result<bool, Error> {
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
    match destination_metadata {
        None => {
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
                    if destination_is_directory {
                        root.remove_tree_typed(destination).map_err(|delete_error| {
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

/// Creates one structured rooted-copy error.
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

    use super::Options;
    use super::Path;
    use super::Root;
    use super::Stage;
    use super::Statistics;
    use super::prepare_directory;
    use crate::LocalCopyFailureState;
    use crate::local::LocalCopyTypeConflictPolicy;

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
            let error = prepare_directory(&root, &source, &destination, &options, &mut statistics)
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
                prepare_directory(&root, &source, &destination, &options, &mut Statistics::default())
                    .expect("retry should recognize the already prepared directory")
            );
        }
    }
}
