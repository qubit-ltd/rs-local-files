// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::{
    fs,
    io,
    path::Path,
};

use crate::{
    LocalAtomicityRequirement,
    LocalCopyMethod,
    LocalCopyOptions,
    LocalCopyOutcome,
    LocalCopyStats,
    LocalCreateDirectoryOptions,
    LocalCreateDirectoryOutcome,
    LocalDeleteOptions,
    LocalDeleteOutcome,
    LocalDirectoryWalker,
    LocalDurabilityRequirement,
    LocalFileError,
    LocalFileErrorKind,
    LocalFileMetadata,
    LocalFileOperation,
    LocalFileReader,
    LocalFileSystemCapabilities,
    LocalFileWriter,
    LocalListOptions,
    LocalMetadataPreservePolicy,
    LocalPaths,
    LocalReadOptions,
    LocalRenameOptions,
    LocalRenameOutcome,
    LocalResult,
    LocalSymlinkPolicy,
    LocalTempDirectory,
    LocalTempDirectoryOptions,
    LocalTempFile,
    LocalTempFileOptions,
    LocalWriteMode,
    LocalWriteOptions,
};

/// Host-wide native local filesystem namespace.
pub enum LocalFileSystem {}

impl LocalFileSystem {
    /// Returns a snapshot of capabilities for the current host platform.
    #[inline(always)]
    pub const fn capabilities() -> LocalFileSystemCapabilities {
        LocalFileSystemCapabilities::detect()
    }

    /// Reads metadata for the final directory entry without following a
    /// symlink.
    ///
    /// # Parameters
    ///
    /// - `path`: Native absolute or relative path.
    ///
    /// # Returns
    ///
    /// Normalized metadata for the final entry.
    ///
    /// # Errors
    ///
    /// Returns `LocalFileError` when the path cannot be inspected.
    #[inline]
    pub fn metadata(path: &Path) -> LocalResult<LocalFileMetadata> {
        fs::symlink_metadata(path)
            .map(|metadata| LocalFileMetadata::from_native(&metadata))
            .map_err(|source| {
                LocalFileError::from_io(
                    LocalFileOperation::Metadata,
                    Some(path.to_path_buf()),
                    None,
                    source,
                )
            })
    }

    /// Opens a synchronous reader for a native regular file.
    ///
    /// # Parameters
    ///
    /// - `path`: Native absolute or relative file path.
    /// - `options`: Reader open policy.
    ///
    /// # Returns
    ///
    /// An owned reader positioned at byte offset zero.
    ///
    /// # Errors
    ///
    /// Returns `LocalFileError` when the entry is not a regular file or cannot
    /// be opened.
    #[inline]
    pub fn open_reader(
        path: &Path,
        options: &LocalReadOptions,
    ) -> LocalResult<LocalFileReader> {
        let bound = LocalPaths::bind_host_path(path)?;
        let metadata = fs::symlink_metadata(&bound).map_err(|source| {
            LocalFileError::from_io(
                LocalFileOperation::OpenReader,
                Some(bound.clone()),
                None,
                source,
            )
        })?;
        if !metadata.file_type().is_file() {
            return Err(LocalFileError::new(
                LocalFileErrorKind::TypeConflict,
                LocalFileOperation::OpenReader,
            )
            .with_path(bound));
        }
        let native_options = options.open_retry_timeout().map_or_else(
            crate::read::OpenOptions::default,
            |timeout| {
                crate::read::OpenOptions::default()
                    .with_open_retry_timeout(timeout)
            },
        );
        crate::local::open_native_reader_path(&bound, &native_options)
            .map(LocalFileReader::new)
            .map_err(|source| {
                LocalFileError::from_io(
                    LocalFileOperation::OpenReader,
                    Some(bound),
                    None,
                    source,
                )
            })
    }

    /// Opens a native writer publication session.
    ///
    /// Create modes stage bytes in the destination directory. Append modifies
    /// an existing regular file directly and therefore rejects required
    /// atomicity.
    ///
    /// # Parameters
    ///
    /// - `path`: Native absolute or relative destination path.
    /// - `options`: Publication mode and guarantee policy.
    ///
    /// # Returns
    ///
    /// A stateful writer in the `Open` state.
    ///
    /// # Errors
    ///
    /// Returns `LocalFileError` for invalid mode/guarantee combinations,
    /// destination conflicts, invalid entry kinds, or native open failures.
    pub fn open_writer(
        path: &Path,
        options: &LocalWriteOptions,
    ) -> LocalResult<LocalFileWriter> {
        use crate::writer::internal::LocalFileWriterBackend;

        let bound = LocalPaths::bind_host_path(path)?;
        if options.mode() == LocalWriteMode::Append
            && options.atomicity() == LocalAtomicityRequirement::Required
        {
            return Err(LocalFileError::new(
                LocalFileErrorKind::RequirementNotMet,
                LocalFileOperation::OpenWriter,
            )
            .with_path(bound));
        }
        if options.mode() != LocalWriteMode::Append
            && options.durability() == LocalDurabilityRequirement::Required
            && !Self::capabilities().supports_directory_durability()
        {
            return Err(LocalFileError::new(
                LocalFileErrorKind::RequirementNotMet,
                LocalFileOperation::OpenWriter,
            )
            .with_path(bound));
        }
        if options.creates_parent()
            && let Some(parent) = bound.parent()
        {
            fs::create_dir_all(parent).map_err(|error| {
                LocalFileError::from_io(
                    LocalFileOperation::OpenWriter,
                    Some(bound.clone()),
                    None,
                    error,
                )
            })?;
        }
        let backend = match options.mode() {
            LocalWriteMode::CreateNew => LocalFileWriterBackend::Staged(
                open_staged_writer(&bound, options)?,
            ),
            LocalWriteMode::CreateOrReplace => LocalFileWriterBackend::Staged(
                open_staged_writer(&bound, options)?,
            ),
            LocalWriteMode::Append => {
                let metadata =
                    fs::symlink_metadata(&bound).map_err(|error| {
                        LocalFileError::from_io(
                            LocalFileOperation::OpenWriter,
                            Some(bound.clone()),
                            None,
                            error,
                        )
                    })?;
                if !metadata.file_type().is_file() {
                    return Err(LocalFileError::new(
                        LocalFileErrorKind::TypeConflict,
                        LocalFileOperation::OpenWriter,
                    )
                    .with_path(bound));
                }
                let file = fs::OpenOptions::new()
                    .append(true)
                    .open(&bound)
                    .map_err(|error| {
                        LocalFileError::from_io(
                            LocalFileOperation::OpenWriter,
                            Some(bound.clone()),
                            None,
                            error,
                        )
                    })?;
                LocalFileWriterBackend::Append(file)
            }
        };
        Ok(LocalFileWriter::new(bound, backend, *options))
    }

    /// Creates a lazy native directory walker.
    ///
    /// The root path is bound before the directory is opened, so later process
    /// working-directory changes cannot redirect traversal.
    ///
    /// # Parameters
    ///
    /// - `path`: Native absolute or relative directory path.
    /// - `options`: Traversal policy fixed for the walker lifetime.
    ///
    /// # Returns
    ///
    /// A lazy iterator yielding structured entries or path-specific errors.
    ///
    /// # Errors
    ///
    /// Returns `LocalFileError` when the root cannot be bound or opened.
    #[inline]
    pub fn list(
        path: &Path,
        options: &LocalListOptions,
    ) -> LocalResult<LocalDirectoryWalker> {
        let bound = LocalPaths::bind_host_path(path)?;
        LocalDirectoryWalker::open(bound, *options)
    }

    /// Copies a native regular file or directory tree through one unified
    /// entry.
    ///
    /// Both paths are bound using one current-directory snapshot. Regular files
    /// are staged in the target directory before publication; directory trees
    /// use the shared iterative native copy pipeline.
    ///
    /// # Parameters
    ///
    /// - `source`: Native source entry.
    /// - `target`: Native destination entry.
    /// - `options`: Conflict, metadata, symlink, atomicity, and durability
    ///   policy.
    ///
    /// # Returns
    ///
    /// Structured method, statistics, and achieved guarantees.
    ///
    /// # Errors
    ///
    /// Returns `LocalFileError` for source/target aliases, unsupported source
    /// kinds, policy conflicts, failed staging, or unmet required guarantees.
    pub fn copy(
        source: &Path,
        target: &Path,
        options: &LocalCopyOptions,
    ) -> LocalResult<LocalCopyOutcome> {
        let [source, target] = LocalPaths::bind_host_paths([source, target])?;
        require_directory_durability(
            options.durability(),
            LocalFileOperation::Copy,
            &source,
            &target,
        )?;
        let source_metadata = fs::symlink_metadata(&source)
            .map_err(|error| copy_io_error(&source, &target, error))?;
        reject_copy_alias(&source, &target, &source_metadata)?;

        let followed_metadata;
        let effective_metadata = if source_metadata.file_type().is_symlink() {
            match options.symlink_policy() {
                LocalSymlinkPolicy::Reject => {
                    return Err(LocalFileError::new(
                        LocalFileErrorKind::Unsupported,
                        LocalFileOperation::Copy,
                    )
                    .with_path(source)
                    .with_target(target));
                }
                LocalSymlinkPolicy::Preserve => {
                    return Err(LocalFileError::new(
                        LocalFileErrorKind::Unsupported,
                        LocalFileOperation::Copy,
                    )
                    .with_path(source)
                    .with_target(target));
                }
                LocalSymlinkPolicy::Follow => {
                    followed_metadata =
                        fs::metadata(&source).map_err(|error| {
                            copy_io_error(&source, &target, error)
                        })?;
                    &followed_metadata
                }
            }
        } else {
            &source_metadata
        };

        if effective_metadata.file_type().is_dir() {
            if options.atomicity() == LocalAtomicityRequirement::Required {
                return Err(LocalFileError::new(
                    LocalFileErrorKind::RequirementNotMet,
                    LocalFileOperation::Copy,
                )
                .with_path(source)
                .with_target(target));
            }
            if options.durability() == LocalDurabilityRequirement::Required {
                return Err(LocalFileError::new(
                    LocalFileErrorKind::RequirementNotMet,
                    LocalFileOperation::Copy,
                )
                .with_path(source)
                .with_target(target));
            }
            let internal_options = internal_copy_options(options);
            let stats = crate::local::copy_dir_all_with_paths(
                &source,
                &target,
                internal_options,
            )
            .map_err(|error| copy_pipeline_error(&source, &target, error))?;
            return Ok(LocalCopyOutcome::new(
                LocalCopyStats::from_internal(stats),
                LocalCopyMethod::Recursive,
                false,
                false,
            ));
        }
        if !effective_metadata.file_type().is_file() {
            return Err(LocalFileError::new(
                LocalFileErrorKind::TypeConflict,
                LocalFileOperation::Copy,
            )
            .with_path(source)
            .with_target(target));
        }

        let mut stats = crate::local::LocalCopyDirStats::default();
        crate::local::copy_file_with_options(
            &source,
            &target,
            internal_copy_options(options),
            &mut stats,
        )
        .map_err(|error| copy_pipeline_error(&source, &target, error))?;
        let durable = published_durability(
            options.durability(),
            fs::File::open(&target)
                .and_then(|file| file.sync_all())
                .and_then(|()| sync_rename_parent(&target)),
            LocalFileOperation::Copy,
            &source,
            &target,
        )?;
        Ok(LocalCopyOutcome::new(
            LocalCopyStats::from_internal(stats),
            LocalCopyMethod::StagedFile,
            true,
            durable,
        ))
    }

    /// Creates a directory using explicit ancestor policy.
    ///
    /// # Parameters
    ///
    /// - `path`: Native absolute or relative directory path.
    /// - `options`: Directory creation policy.
    ///
    /// # Returns
    ///
    /// An outcome indicating whether the requested entry was newly created.
    ///
    /// # Errors
    ///
    /// Returns `LocalFileError` when creation fails or an existing entry is not
    /// a directory.
    #[inline]
    pub fn create_directory(
        path: &Path,
        options: &LocalCreateDirectoryOptions,
    ) -> LocalResult<LocalCreateDirectoryOutcome> {
        let bound = LocalPaths::bind_host_path(path)?;
        let existed = bound.try_exists().map_err(|source| {
            LocalFileError::from_io(
                LocalFileOperation::CreateDirectory,
                Some(bound.clone()),
                None,
                source,
            )
        })?;
        let result = if options.recursive() {
            fs::create_dir_all(&bound)
        } else {
            fs::create_dir(&bound)
        };
        result
            .map(|()| LocalCreateDirectoryOutcome::new(!existed))
            .map_err(|source| {
                LocalFileError::from_io(
                    LocalFileOperation::CreateDirectory,
                    Some(bound),
                    None,
                    source,
                )
            })
    }

    /// Creates a cleanup-owned temporary file.
    ///
    /// The selected parent is bound before entry creation, and affixes are
    /// validated before any temporary entry is left behind.
    ///
    /// # Parameters
    ///
    /// - `options`: Parent directory, filename affixes, and collision limit.
    ///
    /// # Returns
    ///
    /// An open temporary file that removes its path on drop unless kept or
    /// persisted.
    ///
    /// # Errors
    ///
    /// Returns `LocalFileError` when the parent cannot be bound or created,
    /// affixes are invalid, or a unique file cannot be created.
    pub fn create_temp_file(
        options: &LocalTempFileOptions,
    ) -> LocalResult<LocalTempFile> {
        let parent = options
            .parent()
            .map_or_else(std::env::temp_dir, Path::to_path_buf);
        LocalTempFile::in_dir(
            parent,
            options.prefix(),
            options.suffix(),
            options.max_attempts(),
        )
        .map_err(|error| {
            LocalFileError::from_io(
                LocalFileOperation::CreateTempFile,
                options.parent().map(Path::to_path_buf),
                None,
                error,
            )
        })
    }

    /// Creates a cleanup-owned temporary directory.
    ///
    /// # Parameters
    ///
    /// - `options`: Parent directory, directory-name affixes, and collision
    ///   limit.
    ///
    /// # Returns
    ///
    /// A temporary directory that recursively removes itself on drop unless
    /// kept or persisted.
    ///
    /// # Errors
    ///
    /// Returns `LocalFileError` when the parent cannot be bound or created,
    /// affixes are invalid, or a unique directory cannot be created.
    pub fn create_temp_directory(
        options: &LocalTempDirectoryOptions,
    ) -> LocalResult<LocalTempDirectory> {
        let parent = options
            .parent()
            .map_or_else(std::env::temp_dir, Path::to_path_buf);
        LocalTempDirectory::in_dir_with_affixes(
            &parent,
            options.prefix(),
            options.suffix(),
            options.max_attempts(),
        )
        .map_err(|error| {
            LocalFileError::from_io(
                LocalFileOperation::CreateTempDirectory,
                options.parent().map(Path::to_path_buf),
                None,
                error,
            )
        })
    }

    /// Deletes a native file or final symbolic-link entry.
    ///
    /// # Parameters
    ///
    /// - `path`: Native absolute or relative path.
    /// - `options`: Missing-entry policy; recursive mode is ignored for files.
    ///
    /// # Returns
    ///
    /// An outcome indicating whether an entry was removed.
    ///
    /// # Errors
    ///
    /// Returns `LocalFileError` when the entry is a directory or removal fails.
    #[inline]
    pub fn delete_file(
        path: &Path,
        options: &LocalDeleteOptions,
    ) -> LocalResult<LocalDeleteOutcome> {
        let bound = LocalPaths::bind_host_path(path)?;
        let Some(metadata) = metadata_for_delete(
            &bound,
            options,
            LocalFileOperation::DeleteFile,
        )?
        else {
            return Ok(LocalDeleteOutcome::new(false));
        };
        if metadata.file_type().is_dir() {
            return Err(LocalFileError::new(
                LocalFileErrorKind::TypeConflict,
                LocalFileOperation::DeleteFile,
            )
            .with_path(bound));
        }
        fs::remove_file(&bound)
            .map(|()| LocalDeleteOutcome::new(true))
            .map_err(|source| {
                LocalFileError::from_io(
                    LocalFileOperation::DeleteFile,
                    Some(bound),
                    None,
                    source,
                )
            })
    }

    /// Deletes a native directory without following a final symbolic link.
    ///
    /// # Parameters
    ///
    /// - `path`: Native absolute or relative directory path.
    /// - `options`: Recursion and missing-entry policy.
    ///
    /// # Returns
    ///
    /// An outcome indicating whether a directory was removed.
    ///
    /// # Errors
    ///
    /// Returns `LocalFileError` when the entry is not a directory or removal
    /// fails.
    #[inline]
    pub fn delete_directory(
        path: &Path,
        options: &LocalDeleteOptions,
    ) -> LocalResult<LocalDeleteOutcome> {
        let bound = LocalPaths::bind_host_path(path)?;
        let Some(metadata) = metadata_for_delete(
            &bound,
            options,
            LocalFileOperation::DeleteDirectory,
        )?
        else {
            return Ok(LocalDeleteOutcome::new(false));
        };
        if !metadata.file_type().is_dir() {
            return Err(LocalFileError::new(
                LocalFileErrorKind::TypeConflict,
                LocalFileOperation::DeleteDirectory,
            )
            .with_path(bound));
        }
        let result = if options.recursive() {
            fs::remove_dir_all(&bound)
        } else {
            fs::remove_dir(&bound)
        };
        result
            .map(|()| LocalDeleteOutcome::new(true))
            .map_err(|source| {
                LocalFileError::from_io(
                    LocalFileOperation::DeleteDirectory,
                    Some(bound),
                    None,
                    source,
                )
            })
    }

    /// Renames a native entry with explicit overwrite and guarantee policy.
    ///
    /// Both paths are bound using one current-directory snapshot.
    ///
    /// # Parameters
    ///
    /// - `source`: Existing source entry.
    /// - `target`: Destination entry.
    /// - `options`: Overwrite, atomicity, and durability requirements.
    ///
    /// # Returns
    ///
    /// Guarantees actually achieved by the rename.
    ///
    /// # Errors
    ///
    /// Returns `LocalFileError` when source inspection, publication, or
    /// required durability fails.
    #[inline]
    pub fn rename(
        source: &Path,
        target: &Path,
        options: &LocalRenameOptions,
    ) -> LocalResult<LocalRenameOutcome> {
        let [source, target] = LocalPaths::bind_host_paths([source, target])?;
        require_directory_durability(
            options.durability(),
            LocalFileOperation::Rename,
            &source,
            &target,
        )?;
        let source_metadata = fs::symlink_metadata(&source)
            .map_err(|error| rename_io_error(&source, &target, error))?;
        let result = if options.overwrite() {
            if source_metadata.file_type().is_dir() {
                fs::rename(&source, &target)
            } else {
                crate::local::replace_file(&source, &target)
            }
        } else if source_metadata.file_type().is_dir() {
            crate::local::move_directory_without_replacing(&source, &target)
        } else {
            crate::local::move_file_without_replacing(&source, &target)
        };
        result.map_err(|error| rename_io_error(&source, &target, error))?;

        let durable = published_durability(
            options.durability(),
            sync_rename_parent(&target),
            LocalFileOperation::Rename,
            &source,
            &target,
        )?;
        let atomic = true;
        if options.atomicity() == LocalAtomicityRequirement::Required && !atomic
        {
            return Err(LocalFileError::new(
                LocalFileErrorKind::RequirementNotMet,
                LocalFileOperation::Rename,
            )
            .with_path(source)
            .with_target(target));
        }
        Ok(LocalRenameOutcome::new(atomic, durable))
    }
}

/// Reads final-entry metadata for a delete operation and handles missing
/// policy.
///
/// # Parameters
///
/// - `path`: Bound native path.
/// - `options`: Delete policy.
/// - `operation`: File or directory deletion operation.
///
/// # Returns
///
/// `Some` metadata for an existing entry or `None` for an accepted missing
/// entry.
///
/// # Errors
///
/// Returns `LocalFileError` when metadata inspection fails.
#[inline]
fn metadata_for_delete(
    path: &Path,
    options: &LocalDeleteOptions,
    operation: LocalFileOperation,
) -> LocalResult<Option<fs::Metadata>> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => Ok(Some(metadata)),
        Err(error)
            if error.kind() == io::ErrorKind::NotFound
                && options.missing_ok() =>
        {
            Ok(None)
        }
        Err(error) => Err(LocalFileError::from_io(
            operation,
            Some(path.to_path_buf()),
            None,
            error,
        )),
    }
}

/// Adds both rename paths to a native I/O failure.
///
/// # Parameters
///
/// - `source`: Bound source path.
/// - `target`: Bound destination path.
/// - `error`: Native rename failure.
///
/// # Returns
///
/// Structured rename error.
#[inline(always)]
fn rename_io_error(
    source: &Path,
    target: &Path,
    error: io::Error,
) -> LocalFileError {
    LocalFileError::from_io(
        LocalFileOperation::Rename,
        Some(source.to_path_buf()),
        Some(target.to_path_buf()),
        error,
    )
}

/// Synchronizes the destination parent directory where supported.
///
/// # Parameters
///
/// - `target`: Bound destination path.
///
/// # Errors
///
/// Returns native I/O errors from opening or synchronizing the parent.
#[inline]
fn sync_rename_parent(target: &Path) -> io::Result<()> {
    let parent = target.parent().unwrap_or_else(|| Path::new("."));
    #[cfg(unix)]
    {
        fs::File::open(parent)?.sync_all()
    }
    #[cfg(not(unix))]
    {
        let _ = parent;
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "directory durability is not supported on this platform",
        ))
    }
}

/// Rejects a required parent-durability guarantee before namespace mutation.
///
/// # Parameters
///
/// - `requirement`: Requested durability policy.
/// - `operation`: Mutating operation that would publish an entry.
/// - `source`: Primary path.
/// - `target`: Destination path.
///
/// # Errors
///
/// Returns `RequirementNotMet` when the host cannot provide directory
/// durability at all.
#[inline(always)]
fn require_directory_durability(
    requirement: LocalDurabilityRequirement,
    operation: LocalFileOperation,
    source: &Path,
    target: &Path,
) -> LocalResult<()> {
    if requirement == LocalDurabilityRequirement::Required
        && !LocalFileSystem::capabilities().supports_directory_durability()
    {
        return Err(LocalFileError::new(
            LocalFileErrorKind::RequirementNotMet,
            operation,
        )
        .with_path(source.to_path_buf())
        .with_target(target.to_path_buf()));
    }
    Ok(())
}

/// Converts post-publication synchronization into an achieved guarantee.
///
/// # Parameters
///
/// - `requirement`: Requested durability policy.
/// - `sync`: File and parent synchronization result after publication.
/// - `operation`: Operation that already published its destination.
/// - `source`: Primary path.
/// - `target`: Destination path.
///
/// # Returns
///
/// `true` after completed synchronization, or `false` for a permitted
/// preferred downgrade.
///
/// # Errors
///
/// Returns `PublicationIncomplete` with `Published` state when required
/// synchronization fails after the namespace mutation.
#[inline]
fn published_durability(
    requirement: LocalDurabilityRequirement,
    sync: io::Result<()>,
    operation: LocalFileOperation,
    source: &Path,
    target: &Path,
) -> LocalResult<bool> {
    match requirement {
        LocalDurabilityRequirement::NotRequired => Ok(false),
        LocalDurabilityRequirement::Preferred => Ok(sync.is_ok()),
        LocalDurabilityRequirement::Required => sync.map(|()| true).map_err(
            |error| {
                LocalFileError::from_io(
                    operation,
                    Some(source.to_path_buf()),
                    Some(target.to_path_buf()),
                    error,
                )
                .with_kind(LocalFileErrorKind::PublicationIncomplete)
                .with_mutation_state(crate::LocalMutationState::Published)
            },
        ),
    }
}

/// Opens the existing robust same-directory staged writer implementation.
///
/// # Parameters
///
/// - `path`: Bound destination path.
/// - `options`: Unified writer options.
///
/// # Returns
///
/// Open staged writer.
///
/// # Errors
///
/// Returns `LocalFileError` when staging cannot be prepared.
#[inline]
fn open_staged_writer(
    path: &Path,
    options: &LocalWriteOptions,
) -> LocalResult<crate::local::LocalAtomicWriter> {
    let mut native_options = crate::local::LocalAtomicWriteOptions::new()
        .with_target_symlink_replacement()
        .with_durability(options.durability());
    if options.mode() == LocalWriteMode::CreateNew {
        native_options = native_options.with_create_new();
    }
    if options.creates_parent() {
        native_options = native_options.with_parent();
    }
    if let Some(timeout) = options.open_retry_timeout() {
        native_options = native_options.with_open_retry_timeout(timeout);
    }
    crate::local::LocalAtomicWriter::new(path, native_options).map_err(
        |error| {
            let kind = error.kind();
            LocalFileError::from_io(
                LocalFileOperation::OpenWriter,
                Some(path.to_path_buf()),
                None,
                io::Error::new(kind, error),
            )
        },
    )
}

/// Converts unified copy policy to the existing shared native implementation.
///
/// # Parameters
///
/// - `options`: Unified public copy options.
///
/// # Returns
///
/// Equivalent shared copy pipeline options.
#[inline]
pub(crate) fn internal_copy_options(
    options: &LocalCopyOptions,
) -> crate::local::LocalCopyDirOptions {
    let mut result = crate::local::LocalCopyDirOptions::new()
        .with_conflict(options.conflict())
        .with_type_conflict(options.type_conflict());
    if options.symlink_policy() == LocalSymlinkPolicy::Follow {
        result = result.follow_symlinks();
    }
    if options.preserve_metadata() != LocalMetadataPreservePolicy::None {
        result = result.preserve_permissions();
    }
    result
}

/// Rejects textual self-copy and native hard-link aliases.
///
/// # Parameters
///
/// - `source`: Bound source path.
/// - `target`: Bound destination path.
/// - `source_metadata`: Final-entry source metadata.
///
/// # Errors
///
/// Returns `LocalFileError` when both paths identify the same entry.
#[inline]
fn reject_copy_alias(
    source: &Path,
    target: &Path,
    source_metadata: &fs::Metadata,
) -> LocalResult<()> {
    if source == target {
        return Err(copy_alias_error(source, target));
    }
    let target_metadata = match fs::symlink_metadata(target) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(copy_io_error(source, target, error)),
    };
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;

        if source_metadata.dev() == target_metadata.dev()
            && source_metadata.ino() == target_metadata.ino()
        {
            return Err(copy_alias_error(source, target));
        }
    }
    Ok(())
}

/// Creates a structured source/target alias error.
///
/// # Parameters
///
/// - `source`: Bound source path.
/// - `target`: Bound destination path.
///
/// # Returns
///
/// Invalid-input copy error.
#[inline(always)]
fn copy_alias_error(source: &Path, target: &Path) -> LocalFileError {
    LocalFileError::new(
        LocalFileErrorKind::InvalidInput,
        LocalFileOperation::Copy,
    )
    .with_path(source.to_path_buf())
    .with_target(target.to_path_buf())
}

/// Converts a shared copy-pipeline error without discarding its native source.
///
/// # Parameters
///
/// - `source`: Bound source path.
/// - `target`: Bound destination path.
/// - `error`: Shared structured copy error.
///
/// # Returns
///
/// Unified local filesystem error.
#[inline]
fn copy_pipeline_error(
    source: &Path,
    target: &Path,
    error: crate::local::LocalCopyDirError,
) -> LocalFileError {
    let kind = error.kind();
    copy_io_error(source, target, io::Error::new(kind, error))
}

/// Adds both copy paths to a native I/O failure.
///
/// # Parameters
///
/// - `source`: Bound source path.
/// - `target`: Bound destination path.
/// - `error`: Native I/O failure.
///
/// # Returns
///
/// Structured copy error.
#[inline(always)]
fn copy_io_error(
    source: &Path,
    target: &Path,
    error: io::Error,
) -> LocalFileError {
    LocalFileError::from_io(
        LocalFileOperation::Copy,
        Some(source.to_path_buf()),
        Some(target.to_path_buf()),
        error,
    )
}
