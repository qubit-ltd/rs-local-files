// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Rooted copy destination policy and shared failure helpers.
// qubit-style: allow source-test-pair

use std::io;
use std::io::ErrorKind;

use super::EntryKind;
use super::Metadata;
use super::Path;
use super::Root;
use crate::LocalCopyDirError;
use crate::local::CopyDestinationAction;
use crate::local::LocalCopyConflictPolicy as ConflictPolicy;
use crate::local::LocalCopyDirError as Error;
use crate::local::LocalCopyDirOptions as Options;
use crate::local::LocalCopyDirStage as Stage;
use crate::local::LocalCopyDirStats as Statistics;
use crate::local::decide_copy_destination;

/// Copies one final symbolic-link entry without dereferencing it.
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
            #[cfg(feature = "internal-test-support")]
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
                    let remove_result = if destination_is_directory {
                        root.remove_tree(destination)
                    } else {
                        root.remove_file(destination)
                    };
                    remove_result.map_err(|source_error| {
                        error(
                            Stage::PrepareDestination,
                            source,
                            destination,
                            *statistics,
                            source_error,
                        )
                    })?;
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
                    statistics.overwritten = checked_add(statistics.overwritten, 1, source, destination, *statistics)?;
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
pub(super) fn optional_metadata(root: &Root, path: &Path) -> io::Result<Option<Metadata>> {
    match root.symlink_metadata(path) {
        Ok(metadata) => Ok(Some(metadata)),
        Err(source_error) if source_error.kind() == ErrorKind::NotFound => Ok(None),
        Err(source_error) => Err(source_error),
    }
}

/// Adds an exact statistic or reports overflow.
pub(super) fn checked_add(
    value: u64,
    addition: u64,
    source: &Path,
    destination: &Path,
    statistics: Statistics,
) -> Result<u64, Error> {
    #[cfg(feature = "internal-test-support")]
    let result = if crate::local::test_support_enabled("rooted-copy-statistics-overflow") {
        None
    } else {
        value.checked_add(addition)
    };
    #[cfg(not(feature = "internal-test-support"))]
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
#[inline(always)]
pub(super) fn unsupported_source_error() -> io::Error {
    io::Error::new(
        ErrorKind::Unsupported,
        "rooted copy supports only regular files, directories, and symbolic links",
    )
}
