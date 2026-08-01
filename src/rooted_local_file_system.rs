// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
// qubit-style: allow coverage-cfg

use std::{
    io,
    path::Path,
    sync::Arc,
};

use crate::{
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
    LocalFileKind,
    LocalFileMetadata,
    LocalFileOperation,
    LocalFileReader,
    LocalFileSystemCapabilities,
    LocalFileWriter,
    LocalListOptions,
    LocalReadOptions,
    LocalRenameFailure,
    LocalRenameFailureState,
    LocalRenameOptions,
    LocalRenameOutcome,
    LocalRenameResult,
    LocalResult,
    LocalTempDirectory,
    LocalTempDirectoryOptions,
    LocalTempFile,
    LocalTempFileOptions,
    LocalWriteMode,
    LocalWriteOptions,
};

/// Descriptor- or handle-relative authority for one opened native directory.
#[derive(Debug)]
pub struct RootedLocalFileSystem {
    /// Existing secure rooted implementation.
    root: Arc<crate::rooted::Root>,
    /// Capability snapshot cached when the authority is opened.
    capabilities: LocalFileSystemCapabilities,
}

impl RootedLocalFileSystem {
    /// Opens a native root descriptor or handle.
    ///
    /// # Parameters
    ///
    /// - `path`: Native directory path to anchor.
    ///
    /// # Returns
    ///
    /// A rooted authority whose later operations do not rely on path lookup.
    ///
    /// # Errors
    ///
    /// Returns `LocalFileError` when the directory cannot be bound and opened
    /// securely or the current platform lacks rooted primitives.
    pub fn open(path: &Path) -> LocalResult<Self> {
        let root = crate::rooted::Root::open(path).map_err(|error| {
            LocalFileError::from_io(
                LocalFileOperation::OpenRoot,
                Some(path.to_path_buf()),
                None,
                error,
            )
        })?;
        Ok(Self {
            root: Arc::new(root),
            capabilities: LocalFileSystemCapabilities::detect(),
        })
    }

    /// Returns the non-authoritative diagnostic path captured at open time.
    #[must_use]
    #[inline(always)]
    pub fn diagnostic_path(&self) -> &Path {
        self.root.path()
    }

    /// Returns the capability snapshot cached for this opened authority.
    #[inline(always)]
    pub const fn capabilities(&self) -> LocalFileSystemCapabilities {
        self.capabilities
    }

    /// Creates a cleanup-owned temporary file below this opened root.
    ///
    /// The optional parent must be a validated rooted descendant. The returned
    /// resource retains this exact opened root authority, so later cleanup is
    /// unaffected by rename or replacement of the diagnostic root path.
    ///
    /// # Errors
    /// Returns `LocalFileError` when options are invalid, entry creation
    /// collides through all attempts, or rooted traversal/opening fails.
    pub fn create_temp_file(
        &self,
        options: &LocalTempFileOptions,
    ) -> LocalResult<LocalTempFile> {
        let parent = rooted_temp_parent(
            options.parent(),
            LocalFileOperation::CreateTempFile,
        )?;
        if options.max_attempts() == 0 {
            return Err(rooted_io_error(
                LocalFileOperation::CreateTempFile,
                &parent,
                io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "temporary entry retry count must be greater than zero",
                ),
            ));
        }
        for _ in 0..options.max_attempts() {
            let candidate = temp_candidate(
                &parent,
                options.prefix(),
                options.suffix(),
                LocalFileOperation::CreateTempFile,
            )?;
            let relative =
                rooted_path(&candidate, LocalFileOperation::CreateTempFile)?;
            #[cfg(coverage)]
            let opened = if crate::local::coverage_fault_enabled(
                "rooted-temp-file-collision",
            ) {
                Err(io::Error::from(io::ErrorKind::AlreadyExists))
            } else if crate::local::coverage_fault_enabled(
                "rooted-temp-file-open",
            ) {
                Err(io::Error::from(io::ErrorKind::PermissionDenied))
            } else {
                self.root.open_writer(
                    &relative,
                    &crate::write::OpenOptions::new(
                        crate::write::Mode::CreateNew,
                    ),
                )
            };
            #[cfg(not(coverage))]
            let opened = self.root.open_writer(
                &relative,
                &crate::write::OpenOptions::new(crate::write::Mode::CreateNew),
            );
            match opened {
                Ok(file) => {
                    return LocalTempFile::rooted(
                        Arc::clone(&self.root),
                        candidate,
                        file,
                    )
                    .map_err(|error| {
                        rooted_io_error(
                            LocalFileOperation::CreateTempFile,
                            relative.as_path(),
                            error,
                        )
                    });
                }
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                    continue;
                }
                Err(error) => {
                    return Err(rooted_io_error(
                        LocalFileOperation::CreateTempFile,
                        relative.as_path(),
                        error,
                    ));
                }
            }
        }
        Err(rooted_io_error(
            LocalFileOperation::CreateTempFile,
            &parent,
            io::Error::new(
                io::ErrorKind::AlreadyExists,
                "temporary file name attempts exhausted",
            ),
        ))
    }

    /// Creates a cleanup-owned temporary directory below this opened root.
    ///
    /// The optional parent must be a validated rooted descendant. The returned
    /// directory retains this exact opened root authority for recursive
    /// cleanup.
    ///
    /// # Errors
    /// Returns `LocalFileError` when options are invalid, entry creation
    /// collides through all attempts, or rooted traversal/creation fails.
    pub fn create_temp_directory(
        &self,
        options: &LocalTempDirectoryOptions,
    ) -> LocalResult<LocalTempDirectory> {
        let parent = rooted_temp_parent(
            options.parent(),
            LocalFileOperation::CreateTempDirectory,
        )?;
        if options.max_attempts() == 0 {
            return Err(rooted_io_error(
                LocalFileOperation::CreateTempDirectory,
                &parent,
                io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "temporary entry retry count must be greater than zero",
                ),
            ));
        }
        for _ in 0..options.max_attempts() {
            let candidate = temp_candidate(
                &parent,
                options.prefix(),
                options.suffix(),
                LocalFileOperation::CreateTempDirectory,
            )?;
            let relative = rooted_path(
                &candidate,
                LocalFileOperation::CreateTempDirectory,
            )?;
            #[cfg(coverage)]
            let created = if crate::local::coverage_fault_enabled(
                "rooted-temp-directory-collision",
            ) {
                Err(io::Error::from(io::ErrorKind::AlreadyExists))
            } else if crate::local::coverage_fault_enabled(
                "rooted-temp-directory-create",
            ) {
                Err(io::Error::from(io::ErrorKind::PermissionDenied))
            } else {
                self.root.create_dir(&relative)
            };
            #[cfg(not(coverage))]
            let created = self.root.create_dir(&relative);
            match created {
                Ok(()) => {
                    return LocalTempDirectory::rooted(
                        Arc::clone(&self.root),
                        candidate,
                    )
                    .map_err(|error| {
                        rooted_io_error(
                            LocalFileOperation::CreateTempDirectory,
                            relative.as_path(),
                            error,
                        )
                    });
                }
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                    continue;
                }
                Err(error) => {
                    return Err(rooted_io_error(
                        LocalFileOperation::CreateTempDirectory,
                        relative.as_path(),
                        error,
                    ));
                }
            }
        }
        Err(rooted_io_error(
            LocalFileOperation::CreateTempDirectory,
            &parent,
            io::Error::new(
                io::ErrorKind::AlreadyExists,
                "temporary directory name attempts exhausted",
            ),
        ))
    }

    /// Reads metadata for a final rooted entry without following a symlink.
    ///
    /// # Parameters
    ///
    /// - `path`: Validated relative descendant path.
    ///
    /// # Returns
    ///
    /// Normalized metadata for the rooted entry.
    ///
    /// # Errors
    ///
    /// Returns `LocalFileError` for lexical escape, symlink traversal, missing
    /// entries, or native metadata failures.
    #[inline]
    pub fn metadata(&self, path: &Path) -> LocalResult<LocalFileMetadata> {
        if path.as_os_str().is_empty() {
            return self.root.metadata().map(rooted_metadata).map_err(
                |error| {
                    rooted_io_error(LocalFileOperation::Metadata, path, error)
                },
            );
        }
        let relative = rooted_path(path, LocalFileOperation::Metadata)?;
        self.root
            .symlink_metadata(&relative)
            .map(rooted_metadata)
            .map_err(|error| {
                rooted_io_error(LocalFileOperation::Metadata, path, error)
            })
    }

    /// Opens a descriptor-relative reader for a rooted regular file.
    ///
    /// # Parameters
    ///
    /// - `path`: Validated relative descendant path.
    /// - `options`: Reader open policy.
    ///
    /// # Returns
    ///
    /// Owned regular-file reader.
    ///
    /// # Errors
    ///
    /// Returns `LocalFileError` for lexical escape, intermediate symlinks,
    /// invalid entry kinds, or native open failures.
    pub fn open_reader(
        &self,
        path: &Path,
        options: &LocalReadOptions,
    ) -> LocalResult<LocalFileReader> {
        let relative = rooted_path(path, LocalFileOperation::OpenReader)?;
        let metadata =
            self.root.symlink_metadata(&relative).map_err(|error| {
                rooted_io_error(LocalFileOperation::OpenReader, path, error)
            })?;
        if metadata.kind() != crate::rooted::EntryKind::File {
            return Err(LocalFileError::new(
                LocalFileErrorKind::TypeConflict,
                LocalFileOperation::OpenReader,
            )
            .with_path(path.to_path_buf()));
        }
        let native_options = options.open_retry_timeout().map_or_else(
            crate::read::OpenOptions::default,
            |timeout| {
                crate::read::OpenOptions::default()
                    .with_open_retry_timeout(timeout)
            },
        );
        self.root
            .open_reader(&relative, &native_options)
            .map(LocalFileReader::new)
            .map_err(|error| {
                rooted_io_error(LocalFileOperation::OpenReader, path, error)
            })
    }

    /// Creates a descriptor-relative lazy directory walker.
    ///
    /// # Parameters
    ///
    /// - `path`: Relative directory path, or an empty path for the authority
    ///   root.
    /// - `options`: Traversal policy; rooted follow mode is rejected.
    ///
    /// # Returns
    ///
    /// A walker whose descendant operations derive from the opened authority.
    ///
    /// # Errors
    ///
    /// Returns `LocalFileError` for invalid descendants, requested symlink
    /// following, or native directory-read failures.
    pub fn list(
        &self,
        path: &Path,
        options: &LocalListOptions,
    ) -> LocalResult<LocalDirectoryWalker> {
        let relative = if path.as_os_str().is_empty() {
            None
        } else {
            Some(rooted_path(path, LocalFileOperation::List)?)
        };
        LocalDirectoryWalker::open_rooted(
            Arc::clone(&self.root),
            relative,
            *options,
        )
    }

    /// Opens a descriptor-relative writer publication session.
    ///
    /// # Parameters
    ///
    /// - `path`: Validated relative destination path.
    /// - `options`: Publication mode and guarantee policy.
    ///
    /// # Returns
    ///
    /// A stateful rooted writer.
    ///
    /// # Errors
    ///
    /// Returns `LocalFileError` for invalid descendants, conflicts, unsupported
    /// required atomicity for append, invalid entry kinds, or native open
    /// failures.
    pub fn open_writer(
        &self,
        path: &Path,
        options: &LocalWriteOptions,
    ) -> LocalResult<LocalFileWriter> {
        use crate::writer::internal::LocalFileWriterBackend;

        if options.mode() == LocalWriteMode::Append
            && options.atomicity() == crate::LocalAtomicityRequirement::Required
        {
            return Err(LocalFileError::new(
                LocalFileErrorKind::RequirementNotMet,
                LocalFileOperation::OpenWriter,
            )
            .with_path(path.to_path_buf()));
        }
        if options.mode() != LocalWriteMode::Append
            && options.durability() == LocalDurabilityRequirement::Required
            && !self.capabilities.directory_durability_implemented()
        {
            return Err(LocalFileError::new(
                LocalFileErrorKind::RequirementNotMet,
                LocalFileOperation::OpenWriter,
            )
            .with_path(path.to_path_buf()));
        }
        let relative = rooted_path(path, LocalFileOperation::OpenWriter)?;
        let backend = match options.mode() {
            LocalWriteMode::CreateNew | LocalWriteMode::CreateOrReplace => {
                let mut atomic_options = crate::LocalAtomicWriteOptions::new()
                    .with_target_symlink_replacement()
                    .with_durability(options.durability());
                if options.mode() == LocalWriteMode::CreateNew {
                    atomic_options = atomic_options.with_create_new();
                }
                if options.creates_parent() {
                    atomic_options = atomic_options.with_parent();
                }
                if let Some(timeout) = options.open_retry_timeout() {
                    atomic_options =
                        atomic_options.with_open_retry_timeout(timeout);
                }
                let writer = self
                    .root
                    .begin_atomic_write_with_options(&relative, atomic_options)
                    .map_err(|error| {
                        let kind = error.kind();
                        rooted_io_error(
                            LocalFileOperation::OpenWriter,
                            path,
                            io::Error::new(kind, error),
                        )
                    })?;
                LocalFileWriterBackend::Rooted(writer)
            }
            LocalWriteMode::Append => {
                let metadata =
                    self.root.symlink_metadata(&relative).map_err(|error| {
                        rooted_io_error(
                            LocalFileOperation::OpenWriter,
                            path,
                            error,
                        )
                    })?;
                if metadata.kind() != crate::rooted::EntryKind::File {
                    return Err(LocalFileError::new(
                        LocalFileErrorKind::TypeConflict,
                        LocalFileOperation::OpenWriter,
                    )
                    .with_path(path.to_path_buf()));
                }
                let mut native_options = crate::write::OpenOptions::new(
                    crate::write::Mode::AppendExisting,
                );
                if let Some(timeout) = options.open_retry_timeout() {
                    native_options =
                        native_options.with_open_retry_timeout(timeout);
                }
                let file = self
                    .root
                    .open_writer(&relative, &native_options)
                    .map_err(|error| {
                        rooted_io_error(
                            LocalFileOperation::OpenWriter,
                            path,
                            error,
                        )
                    })?;
                LocalFileWriterBackend::Append(file)
            }
        };
        let diagnostic = self.root.path().join(relative.as_path());
        Ok(LocalFileWriter::new(diagnostic, backend, *options))
    }

    /// Creates a directory below the opened root.
    ///
    /// # Parameters
    ///
    /// - `path`: Validated relative descendant path.
    /// - `options`: Ancestor creation policy.
    ///
    /// # Returns
    ///
    /// An outcome indicating whether the requested entry was newly created.
    ///
    /// # Errors
    ///
    /// Returns `LocalFileError` for lexical escape, symlink traversal, type
    /// conflicts, or native creation failures.
    pub fn create_directory(
        &self,
        path: &Path,
        options: &LocalCreateDirectoryOptions,
    ) -> LocalResult<LocalCreateDirectoryOutcome> {
        let relative = rooted_path(path, LocalFileOperation::CreateDirectory)?;
        #[cfg(coverage)]
        let metadata = if crate::local::coverage_fault_enabled(
            "rooted-local-create-directory-status",
        ) {
            Err(io::Error::from(io::ErrorKind::PermissionDenied))
        } else {
            self.root.symlink_metadata(&relative)
        };
        #[cfg(not(coverage))]
        let metadata = self.root.symlink_metadata(&relative);
        let existing_directory = match metadata {
            Ok(metadata)
                if metadata.kind() == crate::rooted::EntryKind::Directory =>
            {
                Some(true)
            }
            Ok(_) => {
                return Err(LocalFileError::new(
                    LocalFileErrorKind::TypeConflict,
                    LocalFileOperation::CreateDirectory,
                )
                .with_path(path.to_path_buf()));
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => None,
            Err(error) => {
                return Err(rooted_io_error(
                    LocalFileOperation::CreateDirectory,
                    path,
                    error,
                ));
            }
        };
        let existed = existing_directory.is_some();
        if existed && !options.exists_ok() {
            return Err(rooted_io_error(
                LocalFileOperation::CreateDirectory,
                path,
                io::Error::from(io::ErrorKind::AlreadyExists),
            ));
        }
        if existing_directory == Some(true) {
            return Ok(LocalCreateDirectoryOutcome::new(false));
        }
        let result = if options.recursive() {
            self.root.create_dir_all(&relative)
        } else {
            self.root.create_dir(&relative)
        };
        match result {
            Ok(()) => Ok(LocalCreateDirectoryOutcome::new(!existed)),
            Err(error)
                if options.exists_ok()
                    && error.kind() == io::ErrorKind::AlreadyExists
                    && self.root.symlink_metadata(&relative).is_ok_and(
                        |metadata| {
                            metadata.kind()
                                == crate::rooted::EntryKind::Directory
                        },
                    ) =>
            {
                Ok(LocalCreateDirectoryOutcome::new(false))
            }
            Err(error) => Err(rooted_io_error(
                LocalFileOperation::CreateDirectory,
                path,
                error,
            )),
        }
    }

    /// Copies one rooted regular file or directory tree.
    ///
    /// # Parameters
    ///
    /// - `source`: Validated relative source path.
    /// - `target`: Validated relative destination path.
    /// - `options`: Unified copy policy.
    ///
    /// # Returns
    ///
    /// Structured copy statistics and achieved atomicity.
    ///
    /// # Errors
    ///
    /// Returns `LocalCopyFailure` for invalid descendants, symbolic links,
    /// conflicts, unsupported required guarantees, or native copy failures.
    #[allow(clippy::result_large_err)]
    pub fn copy(
        &self,
        source: &Path,
        target: &Path,
        options: &LocalCopyOptions,
    ) -> LocalCopyResult {
        if options.durability() == LocalDurabilityRequirement::Required
            && !self.capabilities.directory_durability_implemented()
        {
            return Err(rooted_copy_failure_unchanged(
                LocalFileError::new(
                    LocalFileErrorKind::RequirementNotMet,
                    LocalFileOperation::Copy,
                )
                .with_path(source.to_path_buf())
                .with_target(target.to_path_buf()),
            ));
        }
        let source_path = rooted_path(source, LocalFileOperation::Copy)
            .map_err(rooted_copy_failure_unchanged)?;
        let target_path = rooted_path(target, LocalFileOperation::Copy)
            .map_err(rooted_copy_failure_unchanged)?;
        let metadata =
            self.root.symlink_metadata(&source_path).map_err(|error| {
                rooted_copy_failure_unchanged(rooted_io_error(
                    LocalFileOperation::Copy,
                    source,
                    error,
                ))
            })?;
        let directory = metadata.kind() == crate::rooted::EntryKind::Directory;
        if crate::local::copy_source_mode_mismatch(
            directory,
            options.source_mode(),
        ) {
            return Err(rooted_copy_failure_unchanged(
                LocalFileError::new(
                    LocalFileErrorKind::RequirementNotMet,
                    LocalFileOperation::Copy,
                )
                .with_path(source.to_path_buf())
                .with_target(target.to_path_buf()),
            ));
        }
        let target_is_directory =
            rooted_destination_is_directory(&self.root, &target_path).map_err(
                |error| {
                    rooted_copy_failure_unchanged(rooted_io_error(
                        LocalFileOperation::Copy,
                        target,
                        error,
                    ))
                },
            )?;
        let target_exists = self
            .root
            .symlink_metadata(&target_path)
            .map(|_| true)
            .or_else(|error| {
                (error.kind() == io::ErrorKind::NotFound)
                    .then_some(false)
                    .ok_or(error)
            })
            .map_err(|error| {
                rooted_copy_failure_unchanged(rooted_io_error(
                    LocalFileOperation::Copy,
                    target,
                    error,
                ))
            })?;
        if options.type_conflict() == crate::LocalCopyTypeConflictPolicy::Skip
            && ((directory && !target_is_directory && target_exists)
                || (!directory && target_is_directory))
        {
            return Ok(LocalCopyOutcome::new(
                LocalCopyStats::skipped_one(),
                if directory {
                    LocalCopyMethod::Recursive
                } else {
                    LocalCopyMethod::StagedFile
                },
                false,
                false,
                options.preserve_metadata(),
            ));
        }
        if crate::local::copy_directory_guarantee_unavailable(
            directory,
            options.atomicity(),
            options.durability(),
        ) {
            return Err(rooted_copy_failure_unchanged(
                LocalFileError::new(
                    LocalFileErrorKind::RequirementNotMet,
                    LocalFileOperation::Copy,
                )
                .with_path(source.to_path_buf())
                .with_target(target.to_path_buf()),
            ));
        }
        if crate::local::copy_file_replace_requires_atomicity(
            directory,
            options.atomicity(),
            options.type_conflict(),
            target_is_directory,
        ) {
            return Err(rooted_copy_failure_unchanged(
                LocalFileError::new(
                    LocalFileErrorKind::RequirementNotMet,
                    LocalFileOperation::Copy,
                )
                .with_path(source.to_path_buf())
                .with_target(target.to_path_buf()),
            ));
        }
        if options.creates_parent()
            && let Some(parent) = target_path
                .as_path()
                .parent()
                .filter(|parent| !parent.as_os_str().is_empty())
        {
            let parent = crate::local::LocalRelativePath::new(parent)
                .expect("parent of a validated rooted path is valid");
            self.root.create_dir_all(&parent).map_err(|error| {
                rooted_copy_failure_unchanged(rooted_io_error(
                    LocalFileOperation::Copy,
                    target,
                    error,
                ))
            })?;
        }
        let stats = self
            .root
            .copy_with_durability(
                &source_path,
                &target_path,
                crate::local_file_system::internal_copy_options(options),
                options.durability(),
            )
            .map_err(|error| {
                LocalCopyFailure::from_copy_dir_error(source, target, error)
            })?;
        let parent_durable = rooted_published_durability(
            options.durability(),
            || {
                self.root.sync_parent(&target_path)?;
                if options.creates_parent() {
                    sync_rooted_copy_parent_chain(&self.root, &target_path)?;
                }
                Ok(())
            },
            LocalFileOperation::Copy,
            source,
            target,
        )
        .map_err(|error| {
            rooted_copy_failure_published(
                error,
                LocalCopyStats::from_internal(stats),
            )
        })?;
        let durable = !directory && stats.files_durable() && parent_durable;
        Ok(LocalCopyOutcome::new(
            LocalCopyStats::from_internal(stats),
            if directory {
                LocalCopyMethod::Recursive
            } else {
                LocalCopyMethod::StagedFile
            },
            !directory && stats.atomic_publication(),
            durable,
            options.preserve_metadata(),
        ))
    }

    /// Deletes a rooted file or final symbolic-link entry.
    ///
    /// # Parameters
    ///
    /// - `path`: Validated relative entry path.
    /// - `options`: Missing-entry policy.
    ///
    /// # Returns
    ///
    /// An outcome indicating whether an entry was removed.
    ///
    /// # Errors
    ///
    /// Returns `LocalFileError` for invalid descendants, directory type
    /// conflicts, or native removal failures.
    pub fn delete_file(
        &self,
        path: &Path,
        options: &LocalDeleteOptions,
    ) -> LocalResult<LocalDeleteOutcome> {
        let relative = rooted_path(path, LocalFileOperation::DeleteFile)?;
        match self.root.remove_file(&relative) {
            Ok(()) => Ok(LocalDeleteOutcome::new(true)),
            Err(error)
                if error.kind() == io::ErrorKind::NotFound
                    && options.missing_ok() =>
            {
                Ok(LocalDeleteOutcome::new(false))
            }
            Err(error) => Err(rooted_io_error(
                LocalFileOperation::DeleteFile,
                path,
                error,
            )),
        }
    }

    /// Deletes a rooted directory without following a final link.
    ///
    /// # Parameters
    ///
    /// - `path`: Validated relative directory path.
    /// - `options`: Recursion and missing-entry policy.
    ///
    /// # Returns
    ///
    /// An outcome indicating whether a directory was removed.
    ///
    /// # Errors
    ///
    /// Returns `LocalFileError` for invalid descendants, type conflicts, or
    /// native removal failures.
    pub fn delete_directory(
        &self,
        path: &Path,
        options: &LocalDeleteOptions,
    ) -> LocalResult<LocalDeleteOutcome> {
        let relative = rooted_path(path, LocalFileOperation::DeleteDirectory)?;
        let result = if options.recursive() {
            self.root.remove_tree(&relative)
        } else {
            self.root.remove_empty_dir(&relative)
        };
        match result {
            Ok(()) => Ok(LocalDeleteOutcome::new(true)),
            Err(error)
                if error.kind() == io::ErrorKind::NotFound
                    && options.missing_ok() =>
            {
                Ok(LocalDeleteOutcome::new(false))
            }
            Err(error) => Err(rooted_io_error(
                LocalFileOperation::DeleteDirectory,
                path,
                error,
            )),
        }
    }

    /// Renames one rooted entry to another without leaving the authority.
    ///
    /// # Parameters
    ///
    /// - `source`: Validated relative source path.
    /// - `target`: Validated relative destination path.
    /// - `options`: Overwrite and guarantee policy.
    ///
    /// # Returns
    ///
    /// Achieved rename guarantees.
    ///
    /// # Errors
    ///
    /// Returns `LocalRenameFailure` for invalid descendants, conflicts,
    /// unsupported required durability, or native rename failures. The failure
    /// retains the strongest namespace state proven by rooted native I/O.
    pub fn rename(
        &self,
        source: &Path,
        target: &Path,
        options: &LocalRenameOptions,
    ) -> LocalRenameResult {
        if options.durability() == LocalDurabilityRequirement::Required
            && !self.capabilities.directory_durability_implemented()
        {
            return Err(rooted_rename_failure_unchanged(
                LocalFileError::new(
                    LocalFileErrorKind::RequirementNotMet,
                    LocalFileOperation::Rename,
                )
                .with_path(source.to_path_buf())
                .with_target(target.to_path_buf()),
            ));
        }
        let source_path = rooted_path(source, LocalFileOperation::Rename)
            .map_err(rooted_rename_failure_unchanged)?;
        let target_path = rooted_path(target, LocalFileOperation::Rename)
            .map_err(rooted_rename_failure_unchanged)?;
        let result = if options.overwrite() {
            self.root.rename(&source_path, &target_path)
        } else {
            self.root
                .rename_without_replacing(&source_path, &target_path)
        };
        result.map_err(|error| {
            rooted_rename_failure_after_native_attempt(source, target, error)
        })?;
        let durable = rooted_published_durability(
            options.durability(),
            || {
                self.root.sync_parent(&source_path)?;
                if source_path.as_path().parent()
                    != target_path.as_path().parent()
                {
                    self.root.sync_parent(&target_path)?;
                }
                Ok(())
            },
            LocalFileOperation::Rename,
            source,
            target,
        )
        .map_err(rooted_rename_failure_renamed)?;
        Ok(LocalRenameOutcome::new(true, durable))
    }
}

/// Synchronizes ancestors that may have gained newly created directories.
fn sync_rooted_copy_parent_chain(
    root: &crate::rooted::Root,
    target: &crate::local::LocalRelativePath,
) -> io::Result<()> {
    let mut parent = target.as_path().parent().map(Path::to_path_buf);
    while let Some(path) = parent.filter(|path| !path.as_os_str().is_empty()) {
        let path = crate::local::LocalRelativePath::new(&path)
            .expect("parent of a validated rooted path is valid");
        root.sync_parent(&path)?;
        parent = path.as_path().parent().map(Path::to_path_buf);
    }
    Ok(())
}

/// Wraps a rooted preflight failure that proves no namespace mutation occurred.
#[must_use]
#[inline(always)]
fn rooted_rename_failure_unchanged(
    error: LocalFileError,
) -> LocalRenameFailure {
    LocalRenameFailure::new(error, LocalRenameFailureState::Unchanged)
}

/// Wraps a rooted failure after a completed native rename.
#[inline(always)]
fn rooted_rename_failure_renamed(error: LocalFileError) -> LocalRenameFailure {
    LocalRenameFailure::new(error, LocalRenameFailureState::Renamed)
}

/// Maps a rooted native rename failure to the strongest guaranteed state.
fn rooted_rename_failure_after_native_attempt(
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
    LocalRenameFailure::new(
        LocalFileError::from_io(
            LocalFileOperation::Rename,
            Some(source.to_path_buf()),
            Some(target.to_path_buf()),
            error,
        ),
        state,
    )
}

/// Wraps a pre-publication rooted copy error with an unchanged state.
#[inline]
fn rooted_copy_failure_unchanged(error: LocalFileError) -> LocalCopyFailure {
    LocalCopyFailure::new(
        error,
        LocalCopyFailureState::Unchanged,
        LocalCopyStats::default(),
        None,
        None,
    )
}

/// Wraps a rooted post-publication durability error with a published state.
#[inline]
fn rooted_copy_failure_published(
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

/// Converts rooted post-publication synchronization into an achieved guarantee.
#[inline]
fn rooted_published_durability(
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
            })
        }
    }
}

/// Validates a rooted descendant and preserves the offending native path.
///
/// # Parameters
///
/// - `path`: Candidate relative descendant.
/// - `operation`: Operation requesting the path.
///
/// # Returns
///
/// Existing validated rooted path representation.
///
/// # Errors
///
/// Returns `LocalFileError` for empty, absolute, prefixed, dot, or parent
/// paths.
#[inline]
fn rooted_path(
    path: &Path,
    operation: LocalFileOperation,
) -> LocalResult<crate::local::LocalRelativePath> {
    crate::local::LocalRelativePath::new(path).map_err(|error| {
        LocalFileError::from_io(
            operation,
            Some(path.to_path_buf()),
            None,
            error,
        )
    })
}

/// Reports whether a rooted destination currently names a real directory.
fn rooted_destination_is_directory(
    root: &crate::rooted::Root,
    path: &crate::local::LocalRelativePath,
) -> io::Result<bool> {
    match root.symlink_metadata(path) {
        Ok(metadata) => {
            Ok(metadata.kind() == crate::rooted::EntryKind::Directory)
        }
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error),
    }
}

/// Validates an optional rooted temporary-resource parent.
///
/// # Errors
/// Returns `LocalFileError` when the configured parent is not a normal
/// relative descendant of the opened root.
#[inline]
fn rooted_temp_parent(
    parent: Option<&Path>,
    operation: LocalFileOperation,
) -> LocalResult<std::path::PathBuf> {
    parent.map_or_else(
        || Ok(std::path::PathBuf::new()),
        |parent| {
            rooted_path(parent, operation)
                .map(|path| path.as_path().to_path_buf())
        },
    )
}

/// Generates one rooted temporary-entry candidate beneath a validated parent.
///
/// # Errors
/// Returns `LocalFileError` when an affix is invalid or randomness is
/// unavailable.
#[inline]
fn temp_candidate(
    parent: &Path,
    prefix: Option<&str>,
    suffix: Option<&str>,
    operation: LocalFileOperation,
) -> LocalResult<std::path::PathBuf> {
    crate::local::try_random_file_name("qubit-local-files-", prefix, suffix)
        .map(|name| parent.join(name))
        .map_err(|error| rooted_io_error(operation, parent, error))
}

/// Converts descriptor-relative metadata to the unified metadata type.
///
/// # Parameters
///
/// - `metadata`: Existing rooted metadata.
///
/// # Returns
///
/// Unified normalized metadata.
#[inline]
pub(crate) fn rooted_metadata(
    metadata: crate::rooted::Metadata,
) -> LocalFileMetadata {
    let kind = match metadata.kind() {
        crate::rooted::EntryKind::File => LocalFileKind::File,
        crate::rooted::EntryKind::Directory => LocalFileKind::Directory,
        crate::rooted::EntryKind::Symlink => LocalFileKind::Symlink,
        crate::rooted::EntryKind::Other => LocalFileKind::Other,
    };
    LocalFileMetadata::from_parts(
        kind,
        metadata.size(),
        metadata.accessed_at(),
        metadata.modified_at(),
        metadata.created_at(),
    )
}

/// Adds rooted operation and descendant context to a native I/O failure.
///
/// # Parameters
///
/// - `operation`: Rooted operation that failed.
/// - `path`: Offending descendant path.
/// - `error`: Native I/O failure.
///
/// # Returns
///
/// Structured rooted local filesystem error.
#[inline(always)]
fn rooted_io_error(
    operation: LocalFileOperation,
    path: &Path,
    error: io::Error,
) -> LocalFileError {
    LocalFileError::from_io(operation, Some(path.to_path_buf()), None, error)
}
