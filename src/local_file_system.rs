// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

// qubit-style: allow coverage-cfg
use std::{
    fs,
    io,
    path::{
        Path,
        PathBuf,
    },
};

use crate::{
    LocalAtomicityRequirement,
    LocalCopyFailure,
    LocalCopyFailureState,
    LocalCopyMethod,
    LocalCopyOptions,
    LocalCopyOutcome,
    LocalCopyResult,
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
    LocalRenameFailure,
    LocalRenameFailureState,
    LocalRenameOptions,
    LocalRenameOutcome,
    LocalRenameResult,
    LocalResult,
    LocalSymlinkPolicy,
    LocalTempDirectory,
    LocalTempDirectoryOptions,
    LocalTempFile,
    LocalTempFileOptions,
    LocalWriteMode,
    LocalWriteOptions,
};

/// Host-wide native local filesystem service.
pub struct LocalFileSystem {
    /// Prevents construction of this stateless service type.
    _private: (),
}

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
    pub fn open_reader(
        path: &Path,
        options: &LocalReadOptions,
    ) -> LocalResult<LocalFileReader> {
        let bound = LocalPaths::bind_host_path(path)?;
        let metadata = coverage_io_fault("local-fs-open-reader-metadata")
            .map_or_else(|| fs::symlink_metadata(&bound), Err)
            .map_err(|source| {
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
        coverage_io_fault("local-fs-open-reader-native")
            .map_or_else(
                || {
                    crate::local::open_native_reader_path(
                        &bound,
                        &native_options,
                    )
                },
                Err,
            )
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
            coverage_io_fault("local-fs-open-writer-parent")
                .map_or_else(|| fs::create_dir_all(parent), Err)
                .map_err(|error| {
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
                    coverage_io_fault("local-fs-open-writer-append-metadata")
                        .map_or_else(|| fs::symlink_metadata(&bound), Err)
                        .map_err(|error| {
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
                let mut native_options = crate::write::OpenOptions::new(
                    crate::write::Mode::AppendExisting,
                );
                if let Some(timeout) = options.open_retry_timeout() {
                    native_options =
                        native_options.with_open_retry_timeout(timeout);
                }
                let file =
                    coverage_io_fault("local-fs-open-writer-append-native")
                        .map_or_else(
                            || {
                                crate::local::open_native_writer_path(
                                    &bound,
                                    &native_options,
                                )
                            },
                            Err,
                        )
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
    /// Returns `LocalCopyFailure` for source/target aliases, unsupported source
    /// kinds, policy conflicts, failed staging, or unmet required guarantees.
    #[allow(clippy::result_large_err)]
    pub fn copy(
        source: &Path,
        target: &Path,
        options: &LocalCopyOptions,
    ) -> LocalCopyResult {
        let [source, target] = LocalPaths::bind_host_paths([source, target])
            .map_err(copy_failure_unchanged)?;
        require_directory_durability(
            options.durability(),
            LocalFileOperation::Copy,
            &source,
            &target,
        )
        .map_err(copy_failure_unchanged)?;
        let source_metadata =
            coverage_io_fault("local-fs-copy-source-metadata")
                .map_or_else(|| fs::symlink_metadata(&source), Err)
                .map_err(|error| {
                    copy_failure_unchanged(copy_io_error(
                        &source, &target, error,
                    ))
                })?;
        let followed_metadata;
        let effective_metadata = if source_metadata.file_type().is_symlink() {
            match options.symlink_policy() {
                LocalSymlinkPolicy::Reject => {
                    return Err(copy_failure_unchanged(
                        LocalFileError::new(
                            LocalFileErrorKind::Unsupported,
                            LocalFileOperation::Copy,
                        )
                        .with_path(source)
                        .with_target(target),
                    ));
                }
                LocalSymlinkPolicy::Follow => {
                    followed_metadata =
                        coverage_io_fault("local-fs-copy-follow-metadata")
                            .map_or_else(|| fs::metadata(&source), Err)
                            .map_err(|error| {
                                copy_failure_unchanged(copy_io_error(
                                    &source, &target, error,
                                ))
                            })?;
                    &followed_metadata
                }
            }
        } else {
            &source_metadata
        };

        reject_copy_alias(&source, &target, effective_metadata)
            .map_err(copy_failure_unchanged)?;

        let target_is_directory =
            destination_is_directory(&target).map_err(|error| {
                copy_failure_unchanged(copy_io_error(&source, &target, error))
            })?;
        let source_is_directory = effective_metadata.file_type().is_dir();
        if options.type_conflict() == crate::LocalCopyTypeConflictPolicy::Skip
            && ((source_is_directory
                && !target_is_directory
                && target.exists())
                || (!source_is_directory && target_is_directory))
        {
            return Ok(LocalCopyOutcome::new(
                LocalCopyStats::skipped_one(),
                if source_is_directory {
                    LocalCopyMethod::Recursive
                } else {
                    LocalCopyMethod::StagedFile
                },
                false,
                false,
                options.preserve_metadata(),
            ));
        }

        if source_is_directory {
            if options.source_mode() == crate::LocalCopySourceMode::File {
                return Err(copy_failure_unchanged(
                    LocalFileError::new(
                        LocalFileErrorKind::RequirementNotMet,
                        LocalFileOperation::Copy,
                    )
                    .with_path(source)
                    .with_target(target),
                ));
            }
            if options.atomicity() == LocalAtomicityRequirement::Required {
                return Err(copy_failure_unchanged(
                    LocalFileError::new(
                        LocalFileErrorKind::RequirementNotMet,
                        LocalFileOperation::Copy,
                    )
                    .with_path(source)
                    .with_target(target),
                ));
            }
            if options.durability() == LocalDurabilityRequirement::Required {
                return Err(copy_failure_unchanged(
                    LocalFileError::new(
                        LocalFileErrorKind::RequirementNotMet,
                        LocalFileOperation::Copy,
                    )
                    .with_path(source)
                    .with_target(target),
                ));
            }
            prepare_copy_parent(&target, options).map_err(|error| {
                copy_failure_unchanged(copy_io_error(&source, &target, error))
            })?;
            let internal_options = internal_copy_options(options);
            let stats = crate::local::copy_dir_all_with_paths(
                &source,
                &target,
                internal_options,
            )
            .map_err(|error| copy_pipeline_failure(&source, &target, error))?;
            return Ok(LocalCopyOutcome::new(
                LocalCopyStats::from_internal(stats),
                LocalCopyMethod::Recursive,
                false,
                false,
                options.preserve_metadata(),
            ));
        }
        if !effective_metadata.file_type().is_file() {
            return Err(copy_failure_unchanged(
                LocalFileError::new(
                    LocalFileErrorKind::TypeConflict,
                    LocalFileOperation::Copy,
                )
                .with_path(source)
                .with_target(target),
            ));
        }
        if options.source_mode() == crate::LocalCopySourceMode::Tree {
            return Err(copy_failure_unchanged(
                LocalFileError::new(
                    LocalFileErrorKind::RequirementNotMet,
                    LocalFileOperation::Copy,
                )
                .with_path(source)
                .with_target(target),
            ));
        }
        if options.atomicity() == LocalAtomicityRequirement::Required
            && options.type_conflict()
                == crate::LocalCopyTypeConflictPolicy::Replace
            && target_is_directory
        {
            return Err(copy_failure_unchanged(
                LocalFileError::new(
                    LocalFileErrorKind::RequirementNotMet,
                    LocalFileOperation::Copy,
                )
                .with_path(source)
                .with_target(target),
            ));
        }

        let parent_dirs_to_sync = prepare_copy_parent(&target, options)
            .map_err(|error| {
                copy_failure_unchanged(copy_io_error(&source, &target, error))
            })?;

        let mut stats = crate::local::LocalCopyDirStats::default();
        crate::local::copy_file_with_options(
            &source,
            &target,
            internal_copy_options(options),
            &mut stats,
        )
        .map_err(|error| copy_pipeline_failure(&source, &target, error))?;
        let durable = published_durability(
            options.durability(),
            || {
                fs::File::open(&target)
                    .and_then(|file| file.sync_all())
                    .and_then(|()| sync_parent_directory(&target))
                    .and_then(|()| {
                        sync_created_parent_directories(&parent_dirs_to_sync)
                    })
            },
            LocalFileOperation::Copy,
            &source,
            &target,
        )
        .map_err(|error| {
            copy_failure_published(error, LocalCopyStats::from_internal(stats))
        })?;
        Ok(LocalCopyOutcome::new(
            LocalCopyStats::from_internal(stats),
            LocalCopyMethod::StagedFile,
            stats.atomic_publication(),
            durable,
            options.preserve_metadata(),
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
    pub fn create_directory(
        path: &Path,
        options: &LocalCreateDirectoryOptions,
    ) -> LocalResult<LocalCreateDirectoryOutcome> {
        let bound = LocalPaths::bind_host_path(path)?;
        let existing_directory =
            coverage_io_fault("local-fs-create-directory-exists")
                .map_or_else(|| fs::symlink_metadata(&bound), Err)
                .map(|metadata| metadata.file_type().is_dir())
                .or_else(|error| {
                    if error.kind() == io::ErrorKind::NotFound {
                        Ok(false)
                    } else {
                        Err(error)
                    }
                })
                .map_err(|source| {
                    LocalFileError::from_io(
                        LocalFileOperation::CreateDirectory,
                        Some(bound.clone()),
                        None,
                        source,
                    )
                })?;
        let existed = fs::symlink_metadata(&bound).is_ok();
        if existed && !options.exists_ok() {
            return Err(LocalFileError::from_io(
                LocalFileOperation::CreateDirectory,
                Some(bound),
                None,
                io::Error::from(io::ErrorKind::AlreadyExists),
            ));
        }
        if existed && existing_directory {
            return Ok(LocalCreateDirectoryOutcome::new(false));
        }
        let result = if options.recursive() {
            fs::create_dir_all(&bound)
        } else {
            fs::create_dir(&bound)
        };
        match result {
            Ok(()) => Ok(LocalCreateDirectoryOutcome::new(!existed)),
            Err(source)
                if options.exists_ok()
                    && source.kind() == io::ErrorKind::AlreadyExists
                    && fs::symlink_metadata(&bound).is_ok_and(|metadata| {
                        metadata.file_type().is_dir()
                    }) =>
            {
                Ok(LocalCreateDirectoryOutcome::new(false))
            }
            Err(source) => Err(LocalFileError::from_io(
                LocalFileOperation::CreateDirectory,
                Some(bound),
                None,
                source,
            )),
        }
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
        let parent = LocalPaths::bind_host_path(&parent)?;
        crate::local::create_temp_file_in_dir(
            &parent,
            options.prefix(),
            options.suffix(),
            options.max_attempts(),
        )
        .map(|(path, file)| LocalTempFile::host(path, file))
        .map_err(|error| {
            LocalFileError::from_io(
                LocalFileOperation::CreateTempFile,
                Some(parent),
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
        let parent = LocalPaths::bind_host_path(&parent)?;
        crate::local::create_temp_dir_in_dir_with_affixes(
            &parent,
            options.prefix(),
            options.suffix(),
            options.max_attempts(),
        )
        .map(LocalTempDirectory::host)
        .map_err(|error| {
            LocalFileError::from_io(
                LocalFileOperation::CreateTempDirectory,
                Some(parent),
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
        match coverage_io_fault("local-fs-delete-file-remove")
            .map_or_else(|| fs::remove_file(&bound), Err)
        {
            Ok(()) => Ok(LocalDeleteOutcome::new(true)),
            Err(source)
                if options.missing_ok()
                    && source.kind() == io::ErrorKind::NotFound =>
            {
                Ok(LocalDeleteOutcome::new(false))
            }
            Err(source) => Err(LocalFileError::from_io(
                LocalFileOperation::DeleteFile,
                Some(bound),
                None,
                source,
            )),
        }
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
            coverage_io_fault("local-fs-delete-directory-remove")
                .map_or_else(|| fs::remove_dir_all(&bound), Err)
        } else {
            coverage_io_fault("local-fs-delete-directory-remove")
                .map_or_else(|| fs::remove_dir(&bound), Err)
        };
        match result {
            Ok(()) => Ok(LocalDeleteOutcome::new(true)),
            Err(source)
                if options.missing_ok()
                    && source.kind() == io::ErrorKind::NotFound =>
            {
                Ok(LocalDeleteOutcome::new(false))
            }
            Err(source) => Err(LocalFileError::from_io(
                LocalFileOperation::DeleteDirectory,
                Some(bound),
                None,
                source,
            )),
        }
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
    /// Returns `LocalRenameFailure` when source inspection, publication, or
    /// required durability fails. The failure retains the strongest namespace
    /// state established by the native rename contract.
    pub fn rename(
        source: &Path,
        target: &Path,
        options: &LocalRenameOptions,
    ) -> LocalRenameResult {
        let [source, target] = LocalPaths::bind_host_paths([source, target])
            .map_err(rename_failure_unchanged)?;
        require_directory_durability(
            options.durability(),
            LocalFileOperation::Rename,
            &source,
            &target,
        )
        .map_err(rename_failure_unchanged)?;
        let source_metadata =
            coverage_io_fault("local-fs-rename-source-metadata")
                .map_or_else(|| fs::symlink_metadata(&source), Err)
                .map_err(|error| {
                    rename_failure_unchanged(rename_io_error(
                        &source, &target, error,
                    ))
                })?;
        #[cfg(coverage)]
        if crate::local::coverage_fault_enabled("rename-native-indeterminate") {
            return Err(rename_failure_indeterminate(rename_io_error(
                &source,
                &target,
                io::Error::from_raw_os_error(libc::EIO),
            )));
        }
        let result = if let Some(error) =
            coverage_io_fault("local-fs-rename-native-error")
        {
            Err(error)
        } else {
            if options.overwrite() {
                if source_metadata.file_type().is_dir() {
                    fs::rename(&source, &target)
                } else {
                    crate::local::replace_file(&source, &target)
                }
            } else if source_metadata.file_type().is_dir() {
                crate::local::move_directory_without_replacing(&source, &target)
            } else {
                crate::local::move_file_without_replacing(&source, &target)
            }
        };
        result.map_err(|error| {
            rename_failure_after_native_attempt(&source, &target, error)
        })?;

        let durable = published_durability(
            options.durability(),
            || sync_rename_parents(&source, &target),
            LocalFileOperation::Rename,
            &source,
            &target,
        )
        .map_err(rename_failure_renamed)?;
        // The native rename path above either publishes atomically or fails
        // before publication; no fallback copy-and-delete path is used.
        Ok(LocalRenameOutcome::new(true, durable))
    }
}

/// Creates missing copy target parents and returns directories requiring sync.
#[inline]
fn prepare_copy_parent(
    target: &Path,
    options: &LocalCopyOptions,
) -> io::Result<Vec<PathBuf>> {
    if options.creates_parent() {
        crate::local::ensure_parent_path_with_sync_dirs(target)
    } else {
        Ok(Vec::new())
    }
}

/// Synchronizes newly created copy target parents from deepest to shallowest.
fn sync_created_parent_directories(paths: &[PathBuf]) -> io::Result<()> {
    #[cfg(unix)]
    {
        paths
            .iter()
            .rev()
            .try_for_each(|path| sync_parent_directory(path))
    }
    #[cfg(not(unix))]
    {
        let _ = paths;
        Ok(())
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
fn metadata_for_delete(
    path: &Path,
    options: &LocalDeleteOptions,
    operation: LocalFileOperation,
) -> LocalResult<Option<fs::Metadata>> {
    match coverage_io_fault("local-fs-delete-metadata")
        .map_or_else(|| fs::symlink_metadata(path), Err)
    {
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

/// Returns an injected native I/O failure selected by coverage tests.
///
/// # Parameters
///
/// - `fault`: Stable selector for one facade-native I/O boundary.
///
/// # Returns
///
/// `Some` deterministic I/O error only when the matching coverage fault is
/// enabled; `None` otherwise.
#[cfg(coverage)]
#[must_use]
#[inline(always)]
fn coverage_io_fault(fault: &str) -> Option<io::Error> {
    crate::local::coverage_fault_enabled(fault)
        .then(|| io::Error::from_raw_os_error(libc::EIO))
}

/// Disables facade I/O fault injection outside coverage builds.
///
/// # Parameters
///
/// - `fault`: Ignored selector retained for source-compatible call sites.
///
/// # Returns
///
/// Always `None`, allowing the native I/O operation to proceed unchanged.
#[cfg(not(coverage))]
#[inline(always)]
fn coverage_io_fault(_fault: &str) -> Option<io::Error> {
    None
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
#[inline]
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

/// Wraps a preflight failure that proves no namespace mutation occurred.
#[inline(always)]
fn rename_failure_unchanged(error: LocalFileError) -> LocalRenameFailure {
    LocalRenameFailure::new(error, LocalRenameFailureState::Unchanged)
}

/// Wraps a failure after a completed native rename.
#[inline(always)]
fn rename_failure_renamed(error: LocalFileError) -> LocalRenameFailure {
    LocalRenameFailure::new(error, LocalRenameFailureState::Renamed)
}

/// Maps a native rename failure to the strongest state guaranteed by its
/// contract.
#[inline]
fn rename_failure_after_native_attempt(
    source: &Path,
    target: &Path,
    error: io::Error,
) -> LocalRenameFailure {
    let state = match error.kind() {
        io::ErrorKind::AlreadyExists
        | io::ErrorKind::CrossesDevices
        | io::ErrorKind::NotFound => LocalRenameFailureState::Unchanged,
        _ => LocalRenameFailureState::Indeterminate,
    };
    LocalRenameFailure::new(rename_io_error(source, target, error), state)
}

/// Wraps a failure whose native rename effect cannot be proven.
#[cfg(coverage)]
#[inline(always)]
fn rename_failure_indeterminate(error: LocalFileError) -> LocalRenameFailure {
    LocalRenameFailure::new(error, LocalRenameFailureState::Indeterminate)
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
fn sync_parent_directory(path: &Path) -> io::Result<()> {
    let parent = path.parent().unwrap_or(Path::new("."));
    #[cfg(coverage)]
    if crate::local::coverage_fault_enabled("copy-parent-sync")
        || crate::local::coverage_fault_enabled("rename-parent-sync")
    {
        return Err(io::Error::from_raw_os_error(libc::EIO));
    }
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

/// Synchronizes every parent directory changed by a completed rename.
fn sync_rename_parents(source: &Path, target: &Path) -> io::Result<()> {
    sync_parent_directory(source)?;
    if source.parent() != target.parent() {
        sync_parent_directory(target)?;
    }
    Ok(())
}

/// Reports whether the final destination entry is a real directory.
fn destination_is_directory(path: &Path) -> io::Result<bool> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => Ok(metadata.file_type().is_dir()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error),
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
fn require_directory_durability(
    requirement: LocalDurabilityRequirement,
    operation: LocalFileOperation,
    source: &Path,
    target: &Path,
) -> LocalResult<()> {
    let supports_directory_durability =
        LocalFileSystem::capabilities().supports_directory_durability();
    #[cfg(coverage)]
    let supports_directory_durability = supports_directory_durability
        && !crate::local::coverage_fault_enabled(
            "local-fs-required-directory-durability",
        );
    if requirement == LocalDurabilityRequirement::Required
        && !supports_directory_durability
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
/// - `sync`: File and parent synchronization operation after publication.
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
fn published_durability(
    requirement: LocalDurabilityRequirement,
    sync: impl FnOnce() -> io::Result<()>,
    operation: LocalFileOperation,
    source: &Path,
    target: &Path,
) -> LocalResult<bool> {
    match requirement {
        LocalDurabilityRequirement::NotRequired => Ok(false),
        LocalDurabilityRequirement::Preferred => Ok(sync().is_ok()),
        LocalDurabilityRequirement::Required => {
            sync().map(|()| true).map_err(|error| {
                LocalFileError::from_io(
                    operation,
                    Some(source.to_path_buf()),
                    Some(target.to_path_buf()),
                    error,
                )
                .with_kind(LocalFileErrorKind::PublicationIncomplete)
                .with_mutation_state(crate::LocalMutationState::Published)
            })
        }
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
pub(crate) fn internal_copy_options(
    options: &LocalCopyOptions,
) -> crate::local::LocalCopyDirOptions {
    let mut result = crate::local::LocalCopyDirOptions::new()
        .with_conflict(options.conflict())
        .with_type_conflict(options.type_conflict());
    if options.symlink_policy() == LocalSymlinkPolicy::Follow {
        result = result.follow_symlinks();
    }
    if options.preserve_metadata() == LocalMetadataPreservePolicy::Permissions {
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
fn reject_copy_alias(
    source: &Path,
    target: &Path,
    source_metadata: &fs::Metadata,
) -> LocalResult<()> {
    if source == target {
        return Err(copy_alias_error(source, target));
    }
    let target_metadata =
        match coverage_io_fault("local-fs-copy-target-metadata")
            .map_or_else(|| fs::symlink_metadata(target), Err)
        {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Ok(());
            }
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
    #[cfg(windows)]
    {
        if !source_metadata.file_type().is_symlink()
            && !target_metadata.file_type().is_symlink()
            && windows_file_identity(source)
                .map_err(|error| copy_io_error(source, target, error))?
                == windows_file_identity(target)
                    .map_err(|error| copy_io_error(source, target, error))?
        {
            return Err(copy_alias_error(source, target));
        }
    }
    Ok(())
}

/// Returns the stable Windows identity for a final filesystem entry.
///
/// # Parameters
///
/// - `path`: Entry whose identity is required.
///
/// # Returns
///
/// The volume serial number and file index reported by the opened handle.
///
/// # Errors
///
/// Returns an I/O error when the entry cannot be opened or Windows cannot
/// inspect its handle.
#[cfg(windows)]
fn windows_file_identity(path: &Path) -> io::Result<(u32, u64)> {
    use std::os::windows::io::AsRawHandle;

    use windows_sys::Win32::Storage::FileSystem::{
        BY_HANDLE_FILE_INFORMATION,
        GetFileInformationByHandle,
    };

    let file = fs::File::open(path)?;
    let mut information = BY_HANDLE_FILE_INFORMATION::default();
    // SAFETY: `file` owns a live handle and `information` is a correctly sized
    // writable buffer for `GetFileInformationByHandle`.
    if unsafe {
        GetFileInformationByHandle(file.as_raw_handle(), &raw mut information)
    } == 0
    {
        return Err(io::Error::last_os_error());
    }
    let file_index = (u64::from(information.nFileIndexHigh) << 32)
        | u64::from(information.nFileIndexLow);
    Ok((information.dwVolumeSerialNumber, file_index))
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
#[must_use]
#[inline]
fn copy_alias_error(source: &Path, target: &Path) -> LocalFileError {
    LocalFileError::new(
        LocalFileErrorKind::InvalidInput,
        LocalFileOperation::Copy,
    )
    .with_path(source.to_path_buf())
    .with_target(target.to_path_buf())
}

/// Converts a pipeline failure into a lossless public copy failure.
#[inline(always)]
fn copy_pipeline_failure(
    source: &Path,
    target: &Path,
    error: crate::local::LocalCopyDirError,
) -> LocalCopyFailure {
    LocalCopyFailure::from_copy_dir_error(source, target, error)
}

/// Wraps a pre-publication copy error with an unchanged destination state.
#[inline]
fn copy_failure_unchanged(error: LocalFileError) -> LocalCopyFailure {
    LocalCopyFailure::new(
        error,
        LocalCopyFailureState::Unchanged,
        LocalCopyStats::default(),
        None,
        None,
    )
}

/// Wraps a post-publication durability error with a published destination
/// state.
#[inline]
fn copy_failure_published(
    error: LocalFileError,
    partial_stats: LocalCopyStats,
) -> LocalCopyFailure {
    LocalCopyFailure::new(
        error,
        LocalCopyFailureState::Published,
        partial_stats,
        None,
        None,
    )
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
#[inline]
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
