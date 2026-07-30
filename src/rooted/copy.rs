// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
// qubit-style: allow coverage-cfg
//! Descriptor-relative file and directory copying.

use std::io::{
    self,
    ErrorKind,
};

use crate::copy::{
    ConflictPolicy,
    Error,
    Options,
    Stage,
    Statistics,
    TypeConflictPolicy,
};
use crate::{
    LocalCopyDirError,
    LocalDurabilityRequirement,
    atomic,
    read,
};

use super::{
    EntryKind,
    Metadata,
    Path,
    Root,
};

/// Deferred work for iterative rooted directory copying.
enum Work {
    /// Copies the children of one directory.
    Enter {
        /// Validated source directory.
        source: Path,
        /// Validated destination directory.
        destination: Path,
        /// Source metadata applied after all children are installed.
        metadata: Metadata,
    },
    /// Applies source permissions after a directory's children are installed.
    Finish {
        /// Source directory retained for error context.
        source: Path,
        /// Destination directory whose permissions are finalized.
        destination: Path,
        /// Source metadata supplying portable permissions.
        metadata: Metadata,
    },
}

/// Copies one rooted entry beneath the same opened root.
///
/// # Parameters
///
/// * `root` - Open root authorizing both source and destination.
/// * `source` - Existing source entry.
/// * `destination` - Destination entry beneath the same root.
/// * `options` - Explicit copy policies.
///
/// # Returns
///
/// Exact statistics accumulated by the completed copy.
///
/// # Errors
///
/// Returns a structured copy error when the source is unsupported, the
/// destination conflicts with the selected policies, traversal fails, or a
/// staged file cannot be installed.
pub(super) fn copy(
    root: &Root,
    source: &Path,
    destination: &Path,
    options: Options,
    durability: LocalDurabilityRequirement,
) -> Result<Statistics, Error> {
    if options.follows_symlinks() {
        return Err(error(
            Stage::InspectSource,
            source,
            destination,
            Statistics::default(),
            io::Error::new(
                ErrorKind::Unsupported,
                "rooted copy never follows symbolic links",
            ),
        ));
    }
    if source == destination {
        return Err(error(
            Stage::InspectSource,
            source,
            destination,
            Statistics::default(),
            io::Error::new(
                ErrorKind::InvalidInput,
                "rooted copy source and destination must differ",
            ),
        ));
    }

    let source_metadata =
        root.symlink_metadata(source).map_err(|source_error| {
            error(
                Stage::InspectSource,
                source,
                destination,
                Statistics::default(),
                source_error,
            )
        })?;
    match source_metadata.kind() {
        EntryKind::File => copy_file(
            root,
            source,
            destination,
            &options,
            durability,
            Statistics::default(),
        ),
        EntryKind::Directory => {
            if destination.as_path().starts_with(source.as_path()) {
                return Err(error(
                    Stage::InspectSource,
                    source,
                    destination,
                    Statistics::default(),
                    io::Error::new(
                        ErrorKind::InvalidInput,
                        "rooted tree destination must not be inside the source",
                    ),
                ));
            }
            copy_tree(
                root,
                source,
                destination,
                source_metadata,
                &options,
                durability,
            )
        }
        EntryKind::Symlink | EntryKind::Other => Err(error(
            Stage::InspectSource,
            source,
            destination,
            Statistics::default(),
            unsupported_source_error(),
        )),
    }
}

/// Copies a rooted directory tree with an explicit work stack.
fn copy_tree(
    root: &Root,
    source: &Path,
    destination: &Path,
    source_metadata: Metadata,
    options: &Options,
    durability: LocalDurabilityRequirement,
) -> Result<Statistics, Error> {
    let mut statistics = Statistics::default();
    if !prepare_directory(root, source, destination, options, &mut statistics)?
    {
        return Ok(statistics);
    }
    let mut work = vec![Work::Enter {
        source: source.clone(),
        destination: destination.clone(),
        metadata: source_metadata,
    }];
    while let Some(item) = work.pop() {
        match item {
            Work::Enter {
                source,
                destination,
                metadata,
            } => {
                #[cfg(coverage)]
                if crate::local::coverage_fault_enabled(
                    "rooted-copy-directory-read",
                ) {
                    return Err(error(
                        Stage::ReadSourceDirectory,
                        &source,
                        &destination,
                        statistics,
                        io::Error::from(ErrorKind::PermissionDenied),
                    ));
                }
                let entries =
                    root.read_dir(&source).map_err(|source_error| {
                        error(
                            Stage::ReadSourceDirectory,
                            &source,
                            &destination,
                            statistics,
                            source_error,
                        )
                    })?;
                work.push(Work::Finish {
                    source: source.clone(),
                    destination: destination.clone(),
                    metadata,
                });
                for entry in entries.into_iter().rev() {
                    // `Root::read_dir` constructs entries only from native
                    // directory names, which are guaranteed normal relative
                    // components. Revalidating them cannot fail.
                    let source_child =
                        source.join_component(entry.name()).expect(
                            "root directory entry names are normal components",
                        );
                    let destination_child =
                        destination.join_component(entry.name()).expect(
                            "root directory entry names are normal components",
                        );
                    match entry.metadata().kind() {
                        EntryKind::File => {
                            statistics = copy_file(
                                root,
                                &source_child,
                                &destination_child,
                                options,
                                durability,
                                statistics,
                            )?;
                        }
                        EntryKind::Directory => {
                            if prepare_directory(
                                root,
                                &source_child,
                                &destination_child,
                                options,
                                &mut statistics,
                            )? {
                                work.push(Work::Enter {
                                    source: source_child,
                                    destination: destination_child,
                                    metadata: entry.metadata(),
                                });
                            }
                        }
                        EntryKind::Symlink | EntryKind::Other => {
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
            } => preserve_permissions(
                root,
                &source,
                &destination,
                metadata,
                options,
                statistics,
            )?,
        }
    }
    Ok(statistics)
}

/// Prepares one destination directory according to copy policies.
fn prepare_directory(
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
            #[cfg(coverage)]
            {
                if crate::local::coverage_fault_enabled(
                    "rooted-copy-directory-create",
                ) {
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
            statistics.directories = checked_add(
                statistics.directories,
                1,
                source,
                destination,
                *statistics,
            )?;
            Ok(true)
        }
        Some(metadata) if metadata.kind() == EntryKind::Directory => {
            match options.conflict_policy() {
                ConflictPolicy::Fail => Err(error(
                    Stage::PrepareDestination,
                    source,
                    destination,
                    *statistics,
                    io::Error::new(
                        ErrorKind::AlreadyExists,
                        "rooted copy destination directory already exists",
                    ),
                )),
                ConflictPolicy::Skip => {
                    statistics.skipped = checked_add(
                        statistics.skipped,
                        1,
                        source,
                        destination,
                        *statistics,
                    )?;
                    Ok(false)
                }
                ConflictPolicy::Overwrite => {
                    statistics.overwritten = checked_add(
                        statistics.overwritten,
                        1,
                        source,
                        destination,
                        *statistics,
                    )?;
                    Ok(true)
                }
            }
        }
        Some(_)
            if options.type_conflict_policy()
                == TypeConflictPolicy::Replace =>
        {
            root.remove_file(destination).map_err(|source_error| {
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
            statistics.directories = checked_add(
                statistics.directories,
                1,
                source,
                destination,
                *statistics,
            )?;
            statistics.overwritten = checked_add(
                statistics.overwritten,
                1,
                source,
                destination,
                *statistics,
            )?;
            Ok(true)
        }
        Some(_) => Err(error(
            Stage::PrepareDestination,
            source,
            destination,
            *statistics,
            io::Error::new(
                ErrorKind::AlreadyExists,
                "rooted copy destination has a different entry type",
            ),
        )),
    }
}

/// Copies one regular file with handle-authoritative source metadata.
fn copy_file(
    root: &Root,
    source: &Path,
    destination: &Path,
    options: &Options,
    durability: LocalDurabilityRequirement,
    mut statistics: Statistics,
) -> Result<Statistics, Error> {
    #[cfg(coverage)]
    if crate::local::take_coverage_fault_on_nth("rooted-copy-file-second", 2) {
        return Err(error(
            Stage::CopyFileContents,
            source,
            destination,
            statistics,
            io::Error::from_raw_os_error(libc::EIO),
        ));
    }
    #[cfg(coverage)]
    if crate::local::coverage_fault_enabled("rooted-copy-source-open") {
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
        .map_err(|source_error| {
            error(
                Stage::InspectSourceEntry,
                source,
                destination,
                statistics,
                source_error,
            )
        })?;
    #[cfg(coverage)]
    let source_metadata_result = if crate::local::coverage_fault_enabled(
        "rooted-copy-source-metadata-native",
    ) {
        Err(io::Error::from_raw_os_error(libc::EIO))
    } else {
        Metadata::from_open_file(&reader)
    };
    #[cfg(not(coverage))]
    let source_metadata_result = Metadata::from_open_file(&reader);
    let source_metadata = source_metadata_result.map_err(|source_error| {
        error(
            Stage::InspectSourceEntry,
            source,
            destination,
            statistics,
            source_error,
        )
    })?;
    #[cfg(coverage)]
    {
        if crate::local::coverage_fault_enabled(
            "rooted-copy-destination-metadata",
        ) {
            return Err(error(
                Stage::PrepareDestination,
                source,
                destination,
                statistics,
                io::Error::from(ErrorKind::PermissionDenied),
            ));
        }
    }
    let destination_metadata =
        optional_metadata(root, destination).map_err(|source_error| {
            error(
                Stage::PrepareDestination,
                source,
                destination,
                statistics,
                source_error,
            )
        })?;
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
    if let Some(metadata) = destination_metadata {
        if metadata.kind() != EntryKind::File {
            if options.type_conflict_policy() != TypeConflictPolicy::Replace {
                return Err(error(
                    Stage::PrepareDestination,
                    source,
                    destination,
                    statistics,
                    io::Error::new(
                        ErrorKind::AlreadyExists,
                        "rooted copy destination has a different entry type",
                    ),
                ));
            }
            let remove_result = if metadata.kind() == EntryKind::Directory {
                root.remove_tree(destination)
            } else {
                root.remove_file(destination)
            };
            remove_result.map_err(|source_error| {
                error(
                    Stage::PrepareDestination,
                    source,
                    destination,
                    statistics,
                    source_error,
                )
            })?;
        } else {
            match options.conflict_policy() {
                ConflictPolicy::Fail => {
                    return Err(error(
                        Stage::PrepareDestination,
                        source,
                        destination,
                        statistics,
                        io::Error::new(
                            ErrorKind::AlreadyExists,
                            "rooted copy destination file already exists",
                        ),
                    ));
                }
                ConflictPolicy::Skip => {
                    statistics.skipped = checked_add(
                        statistics.skipped,
                        1,
                        source,
                        destination,
                        statistics,
                    )?;
                    return Ok(statistics);
                }
                ConflictPolicy::Overwrite => {}
            }
        }
    }
    #[cfg(coverage)]
    {
        if crate::local::coverage_fault_enabled("rooted-copy-writer-open") {
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
        .begin_atomic_write_with_options(
            destination,
            atomic::Options::new().with_durability(durability),
        )
        .map_err(|source_error| {
            let source_error =
                io::Error::new(source_error.kind(), source_error);
            error(
                Stage::PrepareDestination,
                source,
                destination,
                statistics,
                source_error,
            )
        })?;
    #[cfg(coverage)]
    let copy_result = if crate::local::coverage_fault_enabled(
        "rooted-copy-file-contents-native",
    ) {
        Err(io::Error::from_raw_os_error(libc::EIO))
    } else {
        io::copy(&mut reader, &mut writer)
    };
    #[cfg(not(coverage))]
    let copy_result = io::copy(&mut reader, &mut writer);
    let bytes = copy_result.map_err(|source_error| {
        error(
            Stage::CopyFileContents,
            source,
            destination,
            statistics,
            source_error,
        )
    })?;
    #[cfg(coverage)]
    let commit_result = if crate::local::coverage_fault_enabled(
        "rooted-copy-file-commit-native",
    ) {
        Err(crate::LocalAtomicWriteError::new(
            crate::LocalAtomicWriteStage::ReplaceDestination,
            destination.as_path().to_path_buf(),
            None,
            crate::LocalAtomicDestinationState::Unchanged,
            io::Error::from_raw_os_error(libc::EIO),
        ))
    } else {
        writer.commit()
    };
    #[cfg(not(coverage))]
    let commit_result = writer.commit();
    commit_result.map_err(|source_error| {
        rooted_commit_error(source, destination, statistics, source_error)
    })?;
    statistics.files =
        checked_add(statistics.files, 1, source, destination, statistics)?;
    statistics.bytes =
        checked_add(statistics.bytes, bytes, source, destination, statistics)?;
    if destination_metadata.is_some() {
        statistics.overwritten = checked_add(
            statistics.overwritten,
            1,
            source,
            destination,
            statistics,
        )?;
    }
    preserve_permissions(
        root,
        source,
        destination,
        source_metadata,
        options,
        statistics,
    )?;
    Ok(statistics)
}

/// Applies source permissions when requested by the caller.
fn preserve_permissions(
    root: &Root,
    source: &Path,
    destination: &Path,
    metadata: Metadata,
    options: &Options,
    statistics: Statistics,
) -> Result<(), Error> {
    if options.preserves_permissions() {
        #[cfg(coverage)]
        if crate::local::coverage_fault_enabled("rooted-copy-set-permissions") {
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
    let (temporary_path, cleanup_error, source_error) =
        source_error.into_staging_parts();
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

/// Reads optional destination metadata without following the final link.
fn optional_metadata(root: &Root, path: &Path) -> io::Result<Option<Metadata>> {
    match root.symlink_metadata(path) {
        Ok(metadata) => Ok(Some(metadata)),
        Err(source_error) if source_error.kind() == ErrorKind::NotFound => {
            Ok(None)
        }
        Err(source_error) => Err(source_error),
    }
}

/// Adds an exact statistic or reports overflow.
fn checked_add(
    value: u64,
    addition: u64,
    source: &Path,
    destination: &Path,
    statistics: Statistics,
) -> Result<u64, Error> {
    #[cfg(coverage)]
    let result = if crate::local::coverage_fault_enabled(
        "rooted-copy-statistics-overflow",
    ) {
        None
    } else {
        value.checked_add(addition)
    };
    #[cfg(not(coverage))]
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
fn error(
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
fn unsupported_source_error() -> io::Error {
    io::Error::new(
        ErrorKind::Unsupported,
        "rooted copy supports only regular files and directories",
    )
}
