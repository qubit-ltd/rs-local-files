// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

// qubit-style: allow coverage-cfg

mod path_resolution;

pub(crate) use path_resolution::resolve_host_path;

use std::{
    fs,
    io,
    path::{
        Path,
        PathBuf,
    },
};

use crate::local::{
    copy_failure_published,
    copy_failure_unchanged,
    ensure_required_directory_durability,
    published_durability,
    rename_failure_after_native_attempt,
    rename_failure_renamed,
    rename_failure_unchanged,
    validate_temp_affixes,
};
use crate::{
    LocalAtomicityRequirement,
    LocalCopyFailure,
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
#[cfg(coverage)]
use crate::{
    LocalRenameFailure,
    LocalRenameFailureState,
};

/// Host-wide native local filesystem service.
pub(crate) struct HostLocalFileSystem {
    /// Prevents construction of this stateless service type.
    _private: (),
}

impl HostLocalFileSystem {
    /// Returns a snapshot of capabilities for the current host platform.
    #[inline(always)]
    pub const fn capabilities() -> LocalFileSystemCapabilities {
        LocalFileSystemCapabilities::detect_host()
    }

    /// Reads metadata using an explicit path-resolution policy.
    pub fn metadata_with_policy(
        path: &Path,
        symlink_policy: LocalSymlinkPolicy,
    ) -> LocalResult<LocalFileMetadata> {
        let bound = LocalPaths::bind_host_path(path)?;
        let resolved = resolve_host_path(&bound, symlink_policy, false)?;
        fs::symlink_metadata(&resolved)
            .map(|metadata| LocalFileMetadata::from_native(&metadata))
            .map_err(|source| {
                LocalFileError::from_io(
                    LocalFileOperation::Metadata,
                    Some(bound),
                    None,
                    source,
                )
            })
    }

    /// Opens a Host reader using an explicit symbolic-link policy.
    pub fn open_reader_with_policy(
        path: &Path,
        options: &LocalReadOptions,
        symlink_policy: LocalSymlinkPolicy,
    ) -> LocalResult<LocalFileReader> {
        let bound = resolve_host_path(path, symlink_policy, !cfg!(windows))?;
        let metadata = coverage_io_fault("local-fs-open-reader-metadata")
            .map_or_else(|| fs::metadata(&bound), Err)
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
                #[cfg(windows)]
                if source.kind() == std::io::ErrorKind::InvalidInput
                    && fs::symlink_metadata(&bound)
                        .is_ok_and(|metadata| metadata.file_type().is_symlink())
                {
                    return LocalFileError::new(
                        LocalFileErrorKind::TypeConflict,
                        LocalFileOperation::OpenReader,
                    )
                    .with_path(bound);
                }
                LocalFileError::from_io(
                    LocalFileOperation::OpenReader,
                    Some(bound),
                    None,
                    source,
                )
            })
    }

    /// Opens a Host writer using an explicit symbolic-link policy.
    pub fn open_writer_with_policy(
        path: &Path,
        options: &LocalWriteOptions,
        symlink_policy: LocalSymlinkPolicy,
    ) -> LocalResult<LocalFileWriter> {
        use crate::writer::internal::LocalFileWriterBackend;

        let follow_final = options.mode() != LocalWriteMode::CreateNew;
        let diagnostic_path = LocalPaths::bind_host_path(path)?;
        let bound =
            resolve_host_path(&diagnostic_path, symlink_policy, follow_final)?;
        if options.mode() == LocalWriteMode::Append
            && options.atomicity() == LocalAtomicityRequirement::Required
        {
            return Err(LocalFileError::new(
                LocalFileErrorKind::RequirementNotMet,
                LocalFileOperation::OpenWriter,
            )
            .with_reason(
                "append mode cannot provide required atomic publication",
            )
            .with_path(diagnostic_path.clone()));
        }
        if options.mode() != LocalWriteMode::Append {
            let supports =
                Self::capabilities().directory_durability_implemented();
            #[cfg(coverage)]
            let supports = supports
                && !crate::local::coverage_fault_enabled(
                    "local-fs-required-directory-durability",
                );
            ensure_required_directory_durability(
                options.durability(),
                LocalFileOperation::OpenWriter,
                &diagnostic_path,
                &diagnostic_path,
                supports,
                "required directory durability is unavailable on this host",
            )?;
        }
        if options.creates_parent()
            && let Some(parent) = bound.parent()
        {
            coverage_io_fault("local-fs-open-writer-parent")
                .map_or_else(|| fs::create_dir_all(parent), Err)
                .map_err(|error| {
                    LocalFileError::from_io(
                        LocalFileOperation::OpenWriter,
                        Some(diagnostic_path.clone()),
                        None,
                        error,
                    )
                })?;
        }
        let backend = match options.mode() {
            LocalWriteMode::CreateNew => LocalFileWriterBackend::Staged(
                open_staged_writer(&bound, options).map_err(|error| {
                    error.with_path(diagnostic_path.clone())
                })?,
            ),
            LocalWriteMode::CreateOrReplace => LocalFileWriterBackend::Staged(
                open_staged_writer(&bound, options).map_err(|error| {
                    error.with_path(diagnostic_path.clone())
                })?,
            ),
            LocalWriteMode::Append => {
                let metadata =
                    coverage_io_fault("local-fs-open-writer-append-metadata")
                        .map_or_else(|| fs::symlink_metadata(&bound), Err)
                        .map_err(|error| {
                            LocalFileError::from_io(
                                LocalFileOperation::OpenWriter,
                                Some(diagnostic_path.clone()),
                                None,
                                error,
                            )
                        })?;
                if !metadata.file_type().is_file() {
                    return Err(LocalFileError::new(
                        LocalFileErrorKind::TypeConflict,
                        LocalFileOperation::OpenWriter,
                    )
                    .with_path(diagnostic_path.clone()));
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
                                Some(diagnostic_path.clone()),
                                None,
                                error,
                            )
                        })?;
                LocalFileWriterBackend::Append(file)
            }
        };
        Ok(LocalFileWriter::new(diagnostic_path, backend, *options))
    }

    /// Opens a Host directory walker using an explicit symbolic-link policy.
    ///
    /// The root path is bound before the directory is opened, so later process
    /// working-directory changes cannot redirect traversal.
    ///
    /// # Parameters
    ///
    /// - `path`: Native absolute or relative directory path.
    /// - `options`: Traversal policy fixed for the walker lifetime.
    /// - `symlink_policy`: Default policy for symbolic links encountered by the
    ///   walker.
    ///
    /// # Returns
    ///
    /// A lazy iterator yielding structured entries or path-specific errors.
    ///
    /// # Errors
    ///
    /// Returns `LocalFileError` when the root cannot be bound or opened.
    pub fn list_with_policy(
        path: &Path,
        options: &LocalListOptions,
        symlink_policy: LocalSymlinkPolicy,
    ) -> LocalResult<LocalDirectoryWalker> {
        let policy = options.symlink_policy().unwrap_or(symlink_policy);
        let bound = resolve_host_path(
            path,
            LocalSymlinkPolicy::FollowAcrossScope,
            true,
        )?;
        LocalDirectoryWalker::open(bound, *options, policy)
    }

    /// Copies through a Host namespace using an explicit symbolic-link policy.
    ///
    /// # Parameters
    ///
    /// - `source`: Native source entry.
    /// - `target`: Native destination entry.
    /// - `options`: Copy conflict, metadata, and guarantee policy.
    /// - `symlink_policy`: Policy for symbolic links encountered in a tree.
    ///
    /// # Returns
    ///
    /// Structured copy statistics and achieved guarantees.
    ///
    /// # Errors
    ///
    /// Returns `LocalCopyFailure` when source inspection, copying, or required
    /// guarantees fail.
    #[allow(clippy::result_large_err)]
    pub fn copy_with_policy(
        source: &Path,
        target: &Path,
        options: &LocalCopyOptions,
        symlink_policy: LocalSymlinkPolicy,
    ) -> LocalCopyResult {
        Self::copy_with_policy_scoped(
            source,
            target,
            options,
            symlink_policy,
            None,
        )
    }

    /// Copies through a Host namespace while constraining followed directory
    /// links to an optional canonical scope root.
    ///
    /// # Parameters
    ///
    /// - `source`: Native source entry.
    /// - `target`: Native destination entry.
    /// - `options`: Copy conflict, metadata, and guarantee policy.
    /// - `symlink_policy`: Policy for symbolic links encountered in a tree.
    /// - `scope_root`: Optional canonical root that followed links must stay
    ///   beneath.
    ///
    /// # Returns
    ///
    /// Structured copy statistics and achieved guarantees.
    ///
    /// # Errors
    ///
    /// Returns `LocalCopyFailure` when source inspection, copying, or required
    /// guarantees fail.
    #[allow(clippy::result_large_err)]
    pub fn copy_with_policy_scoped(
        source: &Path,
        target: &Path,
        options: &LocalCopyOptions,
        symlink_policy: LocalSymlinkPolicy,
        scope_root: Option<&Path>,
    ) -> LocalCopyResult {
        let symlink_policy =
            options.symlink_policy_override().unwrap_or(symlink_policy);
        let [source, target] = LocalPaths::bind_host_paths([source, target])
            .map_err(copy_failure_unchanged)?;
        let source = resolve_host_path(
            &source,
            LocalSymlinkPolicy::FollowAcrossScope,
            false,
        )
        .map_err(copy_failure_unchanged)?;
        let target = resolve_host_path(
            &target,
            LocalSymlinkPolicy::FollowAcrossScope,
            false,
        )
        .map_err(copy_failure_unchanged)?;
        let supports = Self::capabilities().directory_durability_implemented();
        #[cfg(coverage)]
        let supports = supports
            && !crate::local::coverage_fault_enabled(
                "local-fs-required-directory-durability",
            );
        ensure_required_directory_durability(
            options.durability(),
            LocalFileOperation::Copy,
            &source,
            &target,
            supports,
            "required directory durability is unavailable on this host",
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
        if source_metadata.file_type().is_symlink() {
            if options.source_mode() == crate::LocalCopySourceMode::Tree {
                return Err(copy_failure_unchanged(
                    LocalFileError::new(
                        LocalFileErrorKind::RequirementNotMet,
                        LocalFileOperation::Copy,
                    )
                    .with_reason(
                        "a symbolic-link entry is not a directory tree source",
                    )
                    .with_path(source)
                    .with_target(target),
                ));
            }
            return copy_symlink_entry(&source, &target, options);
        }
        let effective_metadata = &source_metadata;

        reject_copy_alias(&source, &target, effective_metadata)
            .map_err(copy_failure_unchanged)?;

        let source_is_directory = effective_metadata.file_type().is_dir();
        if source_is_directory {
            if crate::local::copy_source_mode_mismatch(
                source_is_directory,
                options.source_mode(),
            ) {
                return Err(copy_failure_unchanged(
                    LocalFileError::new(
                        LocalFileErrorKind::RequirementNotMet,
                        LocalFileOperation::Copy,
                    )
                    .with_reason(
                        "copy source is a directory but file mode was required",
                    )
                    .with_path(source)
                    .with_target(target),
                ));
            }
            if crate::local::copy_directory_guarantee_unavailable(
                source_is_directory,
                options.atomicity(),
                options.durability(),
            ) {
                return Err(copy_failure_unchanged(
                    LocalFileError::new(
                        LocalFileErrorKind::RequirementNotMet,
                        LocalFileOperation::Copy,
                    )
                    .with_reason("required directory copy guarantees are unavailable on this host")
                    .with_path(source)
                    .with_target(target),
                ));
            }
            prepare_copy_parent(&target, options).map_err(|error| {
                copy_failure_unchanged(copy_io_error(&source, &target, error))
            })?;
            let internal_options =
                internal_copy_options(options, symlink_policy);
            let stats = scope_root
                .map_or_else(
                    || {
                        crate::local::copy_dir_all_with_paths(
                            &source,
                            &target,
                            internal_options,
                        )
                    },
                    |scope_root| {
                        crate::local::copy_dir_all_with_paths_scoped(
                            &source,
                            &target,
                            internal_options,
                            scope_root,
                        )
                    },
                )
                .map_err(|error| {
                    copy_pipeline_failure(&source, &target, error)
                })?;
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
        if crate::local::copy_source_mode_mismatch(
            source_is_directory,
            options.source_mode(),
        ) {
            return Err(copy_failure_unchanged(
                LocalFileError::new(
                    LocalFileErrorKind::RequirementNotMet,
                    LocalFileOperation::Copy,
                )
                .with_reason(
                    "copy source is a file but directory mode was required",
                )
                .with_path(source)
                .with_target(target),
            ));
        }
        let target_is_directory =
            destination_is_directory(&target).map_err(|error| {
                copy_failure_unchanged(copy_io_error(&source, &target, error))
            })?;
        if crate::local::copy_file_replace_requires_atomicity(
            source_is_directory,
            options.atomicity(),
            options.type_conflict(),
            target_is_directory,
        ) {
            return Err(copy_failure_unchanged(
                LocalFileError::new(
                    LocalFileErrorKind::RequirementNotMet,
                    LocalFileOperation::Copy,
                )
                .with_reason(
                    "required atomic replacement is unavailable for this copy",
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
            internal_copy_options(options, symlink_policy),
            &mut stats,
        )
        .map_err(|error| copy_pipeline_failure(&source, &target, error))?;
        let parent_durable = published_durability(
            options.durability(),
            || {
                sync_parent_directory(&target).and_then(|()| {
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
        let durable = stats.files_durable() && parent_durable;
        Ok(LocalCopyOutcome::new(
            LocalCopyStats::from_internal(stats),
            LocalCopyMethod::StagedFile,
            stats.atomic_publication(),
            durable,
            options.preserve_metadata(),
        ))
    }

    /// Creates a Host directory using an explicit symbolic-link policy.
    ///
    /// # Parameters
    ///
    /// - `path`: Native absolute or relative directory path.
    /// - `options`: Directory creation policy.
    /// - `symlink_policy`: Policy for intermediate symbolic links.
    ///
    /// # Returns
    ///
    /// An outcome indicating whether the requested entry was newly created.
    ///
    /// # Errors
    ///
    /// Returns `LocalFileError` when creation fails or an existing entry is not
    /// a directory.
    pub fn create_directory_with_policy(
        path: &Path,
        options: &LocalCreateDirectoryOptions,
        symlink_policy: LocalSymlinkPolicy,
    ) -> LocalResult<LocalCreateDirectoryOutcome> {
        let bound = resolve_host_path(path, symlink_policy, false)?;
        let existing_directory =
            match coverage_io_fault("local-fs-create-directory-exists")
                .map_or_else(|| fs::symlink_metadata(&bound), Err)
            {
                Ok(metadata) if metadata.file_type().is_dir() => Some(true),
                Ok(_) => {
                    return Err(LocalFileError::new(
                        LocalFileErrorKind::TypeConflict,
                        LocalFileOperation::CreateDirectory,
                    )
                    .with_path(bound));
                }
                Err(error) if error.kind() == io::ErrorKind::NotFound => None,
                Err(source) => {
                    return Err(LocalFileError::from_io(
                        LocalFileOperation::CreateDirectory,
                        Some(bound),
                        None,
                        source,
                    ));
                }
            };
        let existed = existing_directory.is_some();
        if existed && !options.exists_ok() {
            return Err(LocalFileError::from_io(
                LocalFileOperation::CreateDirectory,
                Some(bound),
                None,
                io::Error::from(io::ErrorKind::AlreadyExists),
            ));
        }
        if existing_directory == Some(true) {
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

    /// Creates a Host cleanup-owned temporary file.
    ///
    /// The selected parent is bound before entry creation, and affixes are
    /// validated before any temporary entry is left behind.
    /// # Parameters
    ///
    /// - `options`: Parent directory, filename affixes, and collision limit.
    /// - `symlink_policy`: Policy for the temporary resource parent.
    ///
    /// # Returns
    ///
    /// An open temporary file that removes its path unless kept or persisted.
    ///
    /// # Errors
    ///
    /// Returns `LocalFileError` when the parent cannot be bound or created,
    /// affixes are invalid, or a unique file cannot be created.
    pub fn create_temp_file_with_policy(
        options: &LocalTempFileOptions,
        symlink_policy: LocalSymlinkPolicy,
    ) -> LocalResult<LocalTempFile> {
        let parent = options
            .parent()
            .map_or_else(std::env::temp_dir, Path::to_path_buf);
        let parent = resolve_host_path(&parent, symlink_policy, true)?;
        if options.creates_parent() {
            fs::create_dir_all(&parent).map_err(|error| {
                LocalFileError::from_io(
                    LocalFileOperation::CreateTempFile,
                    Some(parent.clone()),
                    None,
                    error,
                )
            })?;
        }
        validate_host_temp_parent(&parent, LocalFileOperation::CreateTempFile)?;
        validate_temp_affixes(options.prefix(), options.suffix()).map_err(
            |error| {
                LocalFileError::from_io(
                    LocalFileOperation::CreateTempFile,
                    Some(parent.clone()),
                    None,
                    error,
                )
                .with_kind(LocalFileErrorKind::InvalidOptions)
            },
        )?;
        let sandbox = crate::local::create_temp_dir_in_dir_with_affixes(
            &parent,
            Some("sandbox-"),
            None,
            options.max_attempts(),
        )
        .map_err(|error| {
            let invalid_options = error.kind() == io::ErrorKind::InvalidInput;
            let error = LocalFileError::from_io(
                LocalFileOperation::CreateTempFile,
                Some(parent.clone()),
                None,
                error,
            );
            if invalid_options {
                error.with_kind(LocalFileErrorKind::InvalidOptions)
            } else {
                error
            }
        })?;
        let created = crate::local::create_temp_file_in_dir(
            &sandbox,
            options.prefix(),
            options.suffix(),
            options.max_attempts(),
        );
        let result = match created {
            Ok((path, file)) => {
                LocalTempFile::host(path, sandbox.clone(), file, symlink_policy)
            }
            Err(error) => {
                let _ = std::fs::remove_dir_all(&sandbox);
                Err(error)
            }
        };
        result.map_err(|error| {
            let invalid_options = error.kind() == io::ErrorKind::InvalidInput;
            let error = LocalFileError::from_io(
                LocalFileOperation::CreateTempFile,
                Some(parent),
                None,
                error,
            );
            if invalid_options {
                error.with_kind(LocalFileErrorKind::InvalidOptions)
            } else {
                error
            }
        })
    }

    /// Creates a Host cleanup-owned temporary directory.
    ///
    /// # Parameters
    ///
    /// - `options`: Parent directory, directory-name affixes, and collision
    ///   limit.
    /// - `symlink_policy`: Policy for the temporary resource parent.
    ///
    /// # Returns
    ///
    /// A temporary directory that recursively removes itself unless kept or
    /// persisted.
    ///
    /// # Errors
    ///
    /// Returns `LocalFileError` when the parent cannot be bound or created,
    /// affixes are invalid, or a unique directory cannot be created.
    pub fn create_temp_directory_with_policy(
        options: &LocalTempDirectoryOptions,
        symlink_policy: LocalSymlinkPolicy,
    ) -> LocalResult<LocalTempDirectory> {
        let parent = options
            .parent()
            .map_or_else(std::env::temp_dir, Path::to_path_buf);
        let parent = resolve_host_path(&parent, symlink_policy, true)?;
        if options.creates_parent() {
            fs::create_dir_all(&parent).map_err(|error| {
                LocalFileError::from_io(
                    LocalFileOperation::CreateTempDirectory,
                    Some(parent.clone()),
                    None,
                    error,
                )
            })?;
        }
        validate_host_temp_parent(
            &parent,
            LocalFileOperation::CreateTempDirectory,
        )?;
        validate_temp_affixes(options.prefix(), options.suffix()).map_err(
            |error| {
                LocalFileError::from_io(
                    LocalFileOperation::CreateTempDirectory,
                    Some(parent.clone()),
                    None,
                    error,
                )
                .with_kind(LocalFileErrorKind::InvalidOptions)
            },
        )?;
        let sandbox = crate::local::create_temp_dir_in_dir_with_affixes(
            &parent,
            Some("sandbox-"),
            None,
            options.max_attempts(),
        )
        .map_err(|error| {
            let invalid_options = error.kind() == io::ErrorKind::InvalidInput;
            let error = LocalFileError::from_io(
                LocalFileOperation::CreateTempDirectory,
                Some(parent.clone()),
                None,
                error,
            );
            if invalid_options {
                error.with_kind(LocalFileErrorKind::InvalidOptions)
            } else {
                error
            }
        })?;
        let created = crate::local::create_temp_dir_in_dir_with_affixes(
            &sandbox,
            options.prefix(),
            options.suffix(),
            options.max_attempts(),
        );
        let result = match created {
            Ok(path) => {
                LocalTempDirectory::host(path, sandbox.clone(), symlink_policy)
            }
            Err(error) => {
                let _ = std::fs::remove_dir_all(&sandbox);
                Err(error)
            }
        };
        result.map_err(|error| {
            let invalid_options = error.kind() == io::ErrorKind::InvalidInput;
            let error = LocalFileError::from_io(
                LocalFileOperation::CreateTempDirectory,
                Some(parent),
                None,
                error,
            );
            if invalid_options {
                error.with_kind(LocalFileErrorKind::InvalidOptions)
            } else {
                error
            }
        })
    }

    /// Deletes a Host file or final symbolic-link entry using an explicit
    /// symbolic-link policy.
    ///
    /// # Parameters
    ///
    /// - `path`: Native file or symbolic-link path.
    /// - `options`: Missing-entry policy.
    /// - `symlink_policy`: Policy for intermediate symbolic links.
    ///
    /// # Returns
    ///
    /// An outcome indicating whether an entry was removed.
    ///
    /// # Errors
    ///
    /// Returns `LocalFileError` when the entry is a directory or removal fails.
    pub fn delete_file_with_policy(
        path: &Path,
        options: &LocalDeleteOptions,
        symlink_policy: LocalSymlinkPolicy,
    ) -> LocalResult<LocalDeleteOutcome> {
        let bound = resolve_host_path(path, symlink_policy, false)?;
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

    /// Deletes a Host directory without following a final symbolic link.
    ///
    /// # Parameters
    ///
    /// - `path`: Native directory path.
    /// - `options`: Recursion and missing-entry policy.
    /// - `symlink_policy`: Policy for intermediate symbolic links.
    ///
    /// # Returns
    ///
    /// An outcome indicating whether a directory was removed.
    ///
    /// # Errors
    ///
    /// Returns `LocalFileError` when the entry is not a directory or removal
    /// fails.
    pub fn delete_directory_with_policy(
        path: &Path,
        options: &LocalDeleteOptions,
        symlink_policy: LocalSymlinkPolicy,
    ) -> LocalResult<LocalDeleteOutcome> {
        let bound = resolve_host_path(path, symlink_policy, false)?;
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

    /// Renames a Host entry with explicit overwrite, guarantee, and
    /// symbolic-link policies.
    ///
    /// Both paths are bound using one current-directory snapshot.
    ///
    /// # Parameters
    ///
    /// - `source`: Existing source entry.
    /// - `target`: Destination entry.
    /// - `options`: Overwrite, atomicity, and durability requirements.
    /// - `symlink_policy`: Policy for intermediate symbolic links.
    ///
    /// # Returns
    ///
    /// Guarantees actually achieved by the rename.
    ///
    /// # Errors
    ///
    /// Returns `LocalRenameFailure` when source inspection, publication, or a
    /// required guarantee fails.
    pub fn rename_with_policy(
        source: &Path,
        target: &Path,
        options: &LocalRenameOptions,
        symlink_policy: LocalSymlinkPolicy,
    ) -> LocalRenameResult {
        let [source, target] = LocalPaths::bind_host_paths([source, target])
            .map_err(rename_failure_unchanged)?;
        let source = resolve_host_path(&source, symlink_policy, false)
            .map_err(rename_failure_unchanged)?;
        let target = resolve_host_path(&target, symlink_policy, false)
            .map_err(rename_failure_unchanged)?;
        let supports = Self::capabilities().directory_durability_implemented();
        #[cfg(coverage)]
        let supports = supports
            && !crate::local::coverage_fault_enabled(
                "local-fs-required-directory-durability",
            );
        ensure_required_directory_durability(
            options.durability(),
            LocalFileOperation::Rename,
            &source,
            &target,
            supports,
            "required directory durability is unavailable on this host",
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

/// Copies a final symbolic-link entry without dereferencing it.
#[allow(clippy::result_large_err)]
fn copy_symlink_entry(
    source: &Path,
    target: &Path,
    options: &LocalCopyOptions,
) -> LocalCopyResult {
    let existing = match fs::symlink_metadata(target) {
        Ok(metadata) => Some(metadata),
        Err(error) if error.kind() == io::ErrorKind::NotFound => None,
        Err(error) => {
            return Err(copy_failure_unchanged(copy_io_error(
                source, target, error,
            )));
        }
    };
    if existing.is_some() {
        if options.conflict() == crate::LocalCopyConflictPolicy::Skip {
            return Ok(LocalCopyOutcome::new(
                LocalCopyStats::skipped_one(),
                LocalCopyMethod::StagedFile,
                false,
                false,
                options.preserve_metadata(),
            ));
        }
        if options.conflict() == crate::LocalCopyConflictPolicy::Fail {
            return Err(copy_failure_unchanged(copy_io_error(
                source,
                target,
                io::Error::from(io::ErrorKind::AlreadyExists),
            )));
        }
        if existing
            .as_ref()
            .is_some_and(|metadata| metadata.file_type().is_dir())
            && options.type_conflict()
                == crate::LocalCopyTypeConflictPolicy::Fail
        {
            return Err(copy_failure_unchanged(copy_io_error(
                source,
                target,
                io::Error::from(io::ErrorKind::AlreadyExists),
            )));
        }
        let remove_result = if existing
            .as_ref()
            .is_some_and(|metadata| metadata.file_type().is_dir())
        {
            fs::remove_dir_all(target)
        } else {
            fs::remove_file(target)
        };
        if let Err(error) = remove_result {
            return Err(copy_failure_unchanged(copy_io_error(
                source, target, error,
            )));
        }
    }
    if let Err(error) = prepare_copy_parent(target, options) {
        return Err(copy_failure_unchanged(copy_io_error(
            source, target, error,
        )));
    }
    let link_target = match fs::read_link(source) {
        Ok(target) => target,
        Err(error) => {
            return Err(copy_failure_unchanged(copy_io_error(
                source, target, error,
            )));
        }
    };
    if let Err(error) = create_symlink_entry(&link_target, source, target) {
        return Err(copy_failure_unchanged(copy_io_error(
            source, target, error,
        )));
    }
    let stats = crate::local::LocalCopyDirStats {
        files: 1,
        overwritten: u64::from(existing.is_some()),
        files_durable: false,
        ..Default::default()
    };
    let public_stats = LocalCopyStats::from_internal(stats);
    let durable = match options.durability() {
        crate::LocalDurabilityRequirement::NotRequired => false,
        crate::LocalDurabilityRequirement::Preferred => {
            sync_parent_directory(target).is_ok()
        }
        crate::LocalDurabilityRequirement::Required => {
            if let Err(error) = sync_parent_directory(target) {
                return Err(copy_failure_published(
                    copy_io_error(source, target, error),
                    public_stats,
                ));
            }
            true
        }
    };
    Ok(LocalCopyOutcome::new(
        public_stats,
        LocalCopyMethod::StagedFile,
        false,
        durable,
        options.preserve_metadata(),
    ))
}

/// Creates a symbolic link with the platform-specific link-kind API.
fn create_symlink_entry(
    link_target: &Path,
    _source: &Path,
    target: &Path,
) -> io::Result<()> {
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(link_target, target)
    }
    #[cfg(windows)]
    {
        if fs::metadata(_source).is_ok_and(|metadata| metadata.is_dir()) {
            std::os::windows::fs::symlink_dir(link_target, target)
        } else {
            std::os::windows::fs::symlink_file(link_target, target)
        }
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = (link_target, _source, target);
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "symbolic links are unsupported on this platform",
        ))
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
    symlink_policy: LocalSymlinkPolicy,
) -> crate::local::LocalCopyDirOptions {
    let symlink_policy =
        options.symlink_policy_override().unwrap_or(symlink_policy);
    let mut result = crate::local::LocalCopyDirOptions::new()
        .with_conflict(options.conflict())
        .with_type_conflict(options.type_conflict())
        .with_symlink_policy(symlink_policy)
        .with_durability(options.durability());
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
        if source_metadata.file_type().is_dir()
            || target_metadata.file_type().is_dir()
        {
            return Ok(());
        }
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
    use std::os::windows::fs::OpenOptionsExt;
    use std::os::windows::io::AsRawHandle;

    use windows_sys::Win32::Storage::FileSystem::{
        BY_HANDLE_FILE_INFORMATION,
        FILE_FLAG_BACKUP_SEMANTICS,
        GetFileInformationByHandle,
    };

    let file = fs::OpenOptions::new()
        .read(true)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS)
        .open(path)?;
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
/// Invalid-options copy error.
#[must_use]
#[inline]
fn copy_alias_error(source: &Path, target: &Path) -> LocalFileError {
    LocalFileError::new(
        LocalFileErrorKind::InvalidOptions,
        LocalFileOperation::Copy,
    )
    .with_path(source.to_path_buf())
    .with_target(target.to_path_buf())
}

/// Confirms that a host temporary-resource parent is an existing directory.
#[inline]
fn validate_host_temp_parent(
    parent: &Path,
    operation: LocalFileOperation,
) -> LocalResult<()> {
    let metadata = match fs::metadata(parent) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            return Ok(());
        }
        Err(error) => {
            return Err(LocalFileError::from_io(
                operation,
                Some(parent.to_path_buf()),
                None,
                error,
            ));
        }
    };
    if !metadata.is_dir() {
        return Err(LocalFileError::new(
            LocalFileErrorKind::NotDirectory,
            operation,
        )
        .with_path(parent.to_path_buf()));
    }
    Ok(())
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
