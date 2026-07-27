// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
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
            copy_tree(root, source, destination, source_metadata, &options)
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
                    let source_child = source
                        .join_component(entry.name())
                        .map_err(|source_error| {
                            error(
                                Stage::InspectSourceEntry,
                                &source,
                                &destination,
                                statistics,
                                source_error,
                            )
                        })?;
                    let destination_child = destination
                        .join_component(entry.name())
                        .map_err(|source_error| {
                            error(
                                Stage::PrepareDestination,
                                &source_child,
                                &destination,
                                statistics,
                                source_error,
                            )
                        })?;
                    match entry.metadata().kind() {
                        EntryKind::File => {
                            statistics = copy_file(
                                root,
                                &source_child,
                                &destination_child,
                                options,
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
    match optional_metadata(root, destination).map_err(|source_error| {
        error(
            Stage::PrepareDestination,
            source,
            destination,
            *statistics,
            source_error,
        )
    })? {
        None => {
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
    mut statistics: Statistics,
) -> Result<Statistics, Error> {
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
    let source_metadata =
        Metadata::from_open_file(&reader).map_err(|source_error| {
            error(
                Stage::InspectSourceEntry,
                source,
                destination,
                statistics,
                source_error,
            )
        })?;
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
    let mut writer = root
        .begin_atomic_write_with_options(destination, atomic::Options::new())
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
    let bytes = io::copy(&mut reader, &mut writer).map_err(|source_error| {
        error(
            Stage::CopyFileContents,
            source,
            destination,
            statistics,
            source_error,
        )
    })?;
    writer.commit().map_err(|source_error| {
        let source_error = io::Error::new(source_error.kind(), source_error);
        error(
            Stage::CommitFile,
            source,
            destination,
            statistics,
            source_error,
        )
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
    value.checked_add(addition).ok_or_else(|| {
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
fn unsupported_source_error() -> io::Error {
    io::Error::new(
        ErrorKind::Unsupported,
        "rooted copy supports only regular files and directories",
    )
}
