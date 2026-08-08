// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
// qubit-style: allow multiple-public-types

#[path = "rooted_local_file_system/metadata.rs"]
mod metadata_operations;
mod path_support;

pub(crate) use path_support::rooted_metadata;
use path_support::{
    rooted_destination_is_directory,
    rooted_io_error,
    rooted_path,
    rooted_temp_parent,
    temp_candidate,
    validate_rooted_temp_parent,
};

use std::{
    fs::{
        self,
        File,
    },
    io,
    path::Path,
    path::PathBuf,
    sync::Arc,
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
    LocalFileOperation,
    LocalFileReader,
    LocalFileSystemCapabilities,
    LocalFileSystemLimits,
    LocalFileWriter,
    LocalListOptions,
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

/// Descriptor- or handle-relative authority for one opened native directory.
#[derive(Debug)]
pub(crate) struct RootedLocalFileSystem {
    /// Existing secure rooted implementation.
    root: Arc<crate::rooted::Root>,
    /// Capability snapshot cached when the authority is opened.
    capabilities: LocalFileSystemCapabilities,
    /// Best-effort path limits captured from the opened root authority.
    limits: LocalFileSystemLimits,
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
        if std::fs::metadata(path).is_ok_and(|metadata| !metadata.is_dir()) {
            return Err(LocalFileError::new(
                LocalFileErrorKind::NotDirectory,
                LocalFileOperation::OpenRoot,
            )
            .with_path(path.to_path_buf()));
        }
        let root = crate::rooted::Root::open(path).map_err(|error| {
            LocalFileError::from_io(
                LocalFileOperation::OpenRoot,
                Some(path.to_path_buf()),
                None,
                error,
            )
        })?;
        let root = Arc::new(root);
        let limits = root
            .try_clone_authority()
            .map(|file| crate::capability::probe_limits(&file))
            .unwrap_or_else(|_| {
                LocalFileSystemLimits::new(
                    crate::SizeLimit::Unknown,
                    crate::SizeLimit::Unknown,
                )
            });
        Ok(Self {
            root,
            capabilities: LocalFileSystemCapabilities::detect_rooted(),
            limits,
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

    /// Returns limits observed from the opened root authority.
    #[inline(always)]
    pub const fn limits(&self) -> LocalFileSystemLimits {
        self.limits
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
        symlink_policy: LocalSymlinkPolicy,
    ) -> LocalResult<LocalTempFile> {
        let requested_parent = rooted_temp_parent(
            options.parent(),
            LocalFileOperation::CreateTempFile,
        )?;
        let parent = if requested_parent.as_os_str().is_empty() {
            requested_parent
        } else {
            resolve_rooted_path(
                &self.root,
                &requested_parent,
                symlink_policy,
                true,
                LocalFileOperation::CreateTempFile,
            )?
            .as_path()
            .to_path_buf()
        };
        if options.creates_parent() && !parent.as_os_str().is_empty() {
            let parent_path =
                rooted_path(&parent, LocalFileOperation::CreateTempFile)?;
            self.root.create_dir_all(&parent_path).map_err(|error| {
                rooted_io_error(
                    LocalFileOperation::CreateTempFile,
                    &parent,
                    error,
                )
            })?;
        }
        validate_rooted_temp_parent(
            &self.root,
            &parent,
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
            )
            .with_kind(LocalFileErrorKind::InvalidOptions));
        }
        validate_temp_affixes(options.prefix(), options.suffix()).map_err(
            |error| {
                rooted_io_error(
                    LocalFileOperation::CreateTempFile,
                    &parent,
                    error,
                )
                .with_kind(LocalFileErrorKind::InvalidOptions)
            },
        )?;
        for _ in 0..options.max_attempts() {
            let resource_name = crate::local::try_random_file_name(
                "qubit-local-files-",
                options.prefix(),
                options.suffix(),
            )
            .map_err(|error| {
                rooted_io_error(
                    LocalFileOperation::CreateTempFile,
                    &parent,
                    error,
                )
            })?;
            let sandbox = temp_candidate(
                &parent,
                Some("sandbox-"),
                None,
                LocalFileOperation::CreateTempFile,
            )?;
            let sandbox_relative =
                rooted_path(&sandbox, LocalFileOperation::CreateTempFile)?;
            match self.root.create_dir(&sandbox_relative) {
                Ok(()) => {}
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                    continue;
                }
                Err(error) => {
                    return Err(rooted_io_error(
                        LocalFileOperation::CreateTempFile,
                        sandbox_relative.as_path(),
                        error,
                    ));
                }
            }
            let candidate = sandbox.join(resource_name);
            let relative =
                rooted_path(&candidate, LocalFileOperation::CreateTempFile)?;
            #[cfg(feature = "internal-test-support")]
            let opened = if crate::local::test_support_enabled(
                "rooted-temp-file-collision",
            ) {
                Err(io::Error::from(io::ErrorKind::AlreadyExists))
            } else if crate::local::test_support_enabled(
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
            #[cfg(not(feature = "internal-test-support"))]
            let opened = self.root.open_writer(
                &relative,
                &crate::write::OpenOptions::new(crate::write::Mode::CreateNew),
            );
            match opened {
                Ok(file) => {
                    let cleanup_sandbox = sandbox.clone();
                    let result = LocalTempFile::rooted(
                        Arc::clone(&self.root),
                        candidate,
                        sandbox,
                        file,
                        symlink_policy,
                    );
                    return match result {
                        Ok(resource) => Ok(resource),
                        Err(error) => {
                            let _ = self.root.remove_tree(&rooted_path(
                                &cleanup_sandbox,
                                LocalFileOperation::CreateTempFile,
                            )?);
                            Err(rooted_io_error(
                                LocalFileOperation::CreateTempFile,
                                relative.as_path(),
                                error,
                            ))
                        }
                    };
                }
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                    let _ = self.root.remove_tree(&sandbox_relative);
                    continue;
                }
                Err(error) => {
                    let _ = self.root.remove_tree(&sandbox_relative);
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
        symlink_policy: LocalSymlinkPolicy,
    ) -> LocalResult<LocalTempDirectory> {
        let requested_parent = rooted_temp_parent(
            options.parent(),
            LocalFileOperation::CreateTempDirectory,
        )?;
        let parent = if requested_parent.as_os_str().is_empty() {
            requested_parent
        } else {
            resolve_rooted_path(
                &self.root,
                &requested_parent,
                symlink_policy,
                true,
                LocalFileOperation::CreateTempDirectory,
            )?
            .as_path()
            .to_path_buf()
        };
        if options.creates_parent() && !parent.as_os_str().is_empty() {
            let parent_path =
                rooted_path(&parent, LocalFileOperation::CreateTempDirectory)?;
            self.root.create_dir_all(&parent_path).map_err(|error| {
                rooted_io_error(
                    LocalFileOperation::CreateTempDirectory,
                    &parent,
                    error,
                )
            })?;
        }
        validate_rooted_temp_parent(
            &self.root,
            &parent,
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
            )
            .with_kind(LocalFileErrorKind::InvalidOptions));
        }
        validate_temp_affixes(options.prefix(), options.suffix()).map_err(
            |error| {
                rooted_io_error(
                    LocalFileOperation::CreateTempDirectory,
                    &parent,
                    error,
                )
                .with_kind(LocalFileErrorKind::InvalidOptions)
            },
        )?;
        for _ in 0..options.max_attempts() {
            let resource_name = crate::local::try_random_file_name(
                "qubit-local-files-",
                options.prefix(),
                options.suffix(),
            )
            .map_err(|error| {
                rooted_io_error(
                    LocalFileOperation::CreateTempDirectory,
                    &parent,
                    error,
                )
            })?;
            let sandbox = temp_candidate(
                &parent,
                Some("sandbox-"),
                None,
                LocalFileOperation::CreateTempDirectory,
            )?;
            let sandbox_relative =
                rooted_path(&sandbox, LocalFileOperation::CreateTempDirectory)?;
            match self.root.create_dir(&sandbox_relative) {
                Ok(()) => {}
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                    continue;
                }
                Err(error) => {
                    return Err(rooted_io_error(
                        LocalFileOperation::CreateTempDirectory,
                        sandbox_relative.as_path(),
                        error,
                    ));
                }
            }
            let candidate = sandbox.join(resource_name);
            let relative = rooted_path(
                &candidate,
                LocalFileOperation::CreateTempDirectory,
            )?;
            #[cfg(feature = "internal-test-support")]
            let created = if crate::local::test_support_enabled(
                "rooted-temp-directory-collision",
            ) {
                Err(io::Error::from(io::ErrorKind::AlreadyExists))
            } else if crate::local::test_support_enabled(
                "rooted-temp-directory-create",
            ) {
                Err(io::Error::from(io::ErrorKind::PermissionDenied))
            } else {
                self.root.create_dir(&relative)
            };
            #[cfg(not(feature = "internal-test-support"))]
            let created = self.root.create_dir(&relative);
            match created {
                Ok(()) => {
                    let cleanup_sandbox = sandbox.clone();
                    let result = LocalTempDirectory::rooted(
                        Arc::clone(&self.root),
                        candidate,
                        sandbox,
                        symlink_policy,
                    );
                    return match result {
                        Ok(resource) => Ok(resource),
                        Err(error) => {
                            let _ = self.root.remove_tree(&rooted_path(
                                &cleanup_sandbox,
                                LocalFileOperation::CreateTempDirectory,
                            )?);
                            Err(rooted_io_error(
                                LocalFileOperation::CreateTempDirectory,
                                relative.as_path(),
                                error,
                            ))
                        }
                    };
                }
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                    let _ = self.root.remove_tree(&sandbox_relative);
                    continue;
                }
                Err(error) => {
                    let _ = self.root.remove_tree(&sandbox_relative);
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
        symlink_policy: LocalSymlinkPolicy,
    ) -> LocalResult<LocalFileReader> {
        let relative = resolve_rooted_path(
            &self.root,
            path,
            symlink_policy,
            true,
            LocalFileOperation::OpenReader,
        )?;
        let native_options = options.open_retry_timeout().map_or_else(
            crate::read::OpenOptions::default,
            |timeout| {
                crate::read::OpenOptions::default()
                    .with_open_retry_timeout(timeout)
            },
        );
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
    /// - `options`: Traversal policy and optional per-operation link override.
    ///
    /// # Returns
    ///
    /// A walker whose descendant operations derive from the opened authority.
    ///
    /// # Errors
    ///
    /// Returns `LocalFileError` for invalid descendants, scope escapes, or
    /// native directory-read failures.
    pub fn list(
        &self,
        path: &Path,
        options: &LocalListOptions,
        symlink_policy: LocalSymlinkPolicy,
    ) -> LocalResult<LocalDirectoryWalker> {
        let symlink_policy = options.symlink_policy().unwrap_or(symlink_policy);
        let relative = if path.as_os_str().is_empty() {
            None
        } else {
            Some(rooted_path(path, LocalFileOperation::List)?)
        };
        validate_rooted_list_start(&self.root, path, symlink_policy)?;
        if relative.is_some() && symlink_policy.follows() {
            let resolved = resolve_rooted_path(
                &self.root,
                path,
                symlink_policy,
                true,
                LocalFileOperation::List,
            )?;
            if relative.as_ref() != Some(&resolved) {
                return LocalDirectoryWalker::open_rooted_with_output(
                    Arc::clone(&self.root),
                    Some(resolved),
                    relative.as_ref().map_or_else(PathBuf::new, |path| {
                        path.as_path().to_path_buf()
                    }),
                    *options,
                    symlink_policy,
                );
            }
        }
        LocalDirectoryWalker::open_rooted(
            Arc::clone(&self.root),
            relative,
            *options,
            symlink_policy,
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
        symlink_policy: LocalSymlinkPolicy,
    ) -> LocalResult<LocalFileWriter> {
        use crate::writer::internal::LocalFileWriterBackend;

        if options.mode() == LocalWriteMode::Append
            && options.atomicity() == crate::LocalAtomicityRequirement::Required
        {
            return Err(LocalFileError::new(
                LocalFileErrorKind::RequirementNotMet,
                LocalFileOperation::OpenWriter,
            )
            .with_reason(
                "append mode cannot provide required atomic publication",
            )
            .with_path(path.to_path_buf()));
        }
        if options.mode() != LocalWriteMode::Append {
            ensure_required_directory_durability(
                options.durability(),
                LocalFileOperation::OpenWriter,
                path,
                path,
                self.capabilities.implements_durable_file_copy(),
                "required directory durability is unavailable for this rooted authority",
            )?;
        }
        let relative = resolve_rooted_path(
            &self.root,
            path,
            symlink_policy,
            options.mode() != LocalWriteMode::CreateNew,
            LocalFileOperation::OpenWriter,
        )?;
        let backend = match options.mode() {
            LocalWriteMode::CreateNew | LocalWriteMode::CreateOrReplace => {
                let mut atomic_options = crate::LocalAtomicWriteOptions::new()
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
        symlink_policy: LocalSymlinkPolicy,
    ) -> LocalResult<LocalCreateDirectoryOutcome> {
        let relative = resolve_rooted_path(
            &self.root,
            path,
            symlink_policy,
            false,
            LocalFileOperation::CreateDirectory,
        )?;
        #[cfg(feature = "internal-test-support")]
        let metadata = if crate::local::test_support_enabled(
            "rooted-local-create-directory-status",
        ) {
            Err(io::Error::from(io::ErrorKind::PermissionDenied))
        } else {
            self.root.symlink_metadata(&relative)
        };
        #[cfg(not(feature = "internal-test-support"))]
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
        symlink_policy: LocalSymlinkPolicy,
    ) -> LocalCopyResult {
        ensure_required_directory_durability(
            options.durability(),
            LocalFileOperation::Copy,
            source,
            target,
            self.capabilities.implements_durable_file_copy(),
            "required directory durability is unavailable for this rooted authority",
        )
        .map_err(copy_failure_unchanged)?;
        let symlink_policy =
            options.symlink_policy_override().unwrap_or(symlink_policy);
        let source_path = resolve_rooted_path(
            &self.root,
            source,
            symlink_policy,
            false,
            LocalFileOperation::Copy,
        )
        .map_err(copy_failure_unchanged)?;
        let target_path = resolve_rooted_path(
            &self.root,
            target,
            symlink_policy,
            false,
            LocalFileOperation::Copy,
        )
        .map_err(copy_failure_unchanged)?;
        let metadata =
            self.root.symlink_metadata(&source_path).map_err(|error| {
                copy_failure_unchanged(rooted_io_error(
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
            return Err(copy_failure_unchanged(
                LocalFileError::new(
                    LocalFileErrorKind::RequirementNotMet,
                    LocalFileOperation::Copy,
                )
                .with_reason("copy source type does not satisfy the selected source mode")
                .with_path(source.to_path_buf())
                .with_target(target.to_path_buf()),
            ));
        }
        let target_is_directory =
            rooted_destination_is_directory(&self.root, &target_path).map_err(
                |error| {
                    copy_failure_unchanged(rooted_io_error(
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
                copy_failure_unchanged(rooted_io_error(
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
            return Err(copy_failure_unchanged(
                LocalFileError::new(
                    LocalFileErrorKind::RequirementNotMet,
                    LocalFileOperation::Copy,
                )
                .with_reason("required copy guarantees are unavailable for this rooted authority")
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
            return Err(copy_failure_unchanged(
                LocalFileError::new(
                    LocalFileErrorKind::RequirementNotMet,
                    LocalFileOperation::Copy,
                )
                .with_reason(
                    "required atomic replacement is unavailable for this copy",
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
                copy_failure_unchanged(rooted_io_error(
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
                crate::local::internal_copy_options(options, symlink_policy),
                options.durability(),
            )
            .map_err(|error| {
                LocalCopyFailure::from_copy_dir_error(source, target, error)
            })?;
        let parent_durable = published_durability(
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
            copy_failure_published(error, LocalCopyStats::from_internal(stats))
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
        symlink_policy: LocalSymlinkPolicy,
    ) -> LocalResult<LocalDeleteOutcome> {
        let relative = resolve_rooted_path(
            &self.root,
            path,
            symlink_policy,
            false,
            LocalFileOperation::DeleteFile,
        )?;
        let result = self.root.remove_file(&relative);
        match result {
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
        symlink_policy: LocalSymlinkPolicy,
    ) -> LocalResult<LocalDeleteOutcome> {
        let relative = resolve_rooted_path(
            &self.root,
            path,
            symlink_policy,
            false,
            LocalFileOperation::DeleteDirectory,
        )?;
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
        symlink_policy: LocalSymlinkPolicy,
    ) -> LocalRenameResult {
        ensure_required_directory_durability(
            options.durability(),
            LocalFileOperation::Rename,
            source,
            target,
            self.capabilities.implements_durable_rename(),
            "required directory durability is unavailable for this rooted authority",
        )
        .map_err(rename_failure_unchanged)?;
        let source_path = resolve_rooted_path(
            &self.root,
            source,
            symlink_policy,
            false,
            LocalFileOperation::Rename,
        )
        .map_err(rename_failure_unchanged)?;
        let target_path = resolve_rooted_path(
            &self.root,
            target,
            symlink_policy,
            false,
            LocalFileOperation::Rename,
        )
        .map_err(rename_failure_unchanged)?;
        let result = if options.overwrite() {
            self.root.rename(&source_path, &target_path)
        } else {
            self.root
                .rename_without_replacing(&source_path, &target_path)
        };
        result.map_err(|error| {
            rename_failure_after_native_attempt(source, target, error)
        })?;
        let durable = published_durability(
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
        .map_err(rename_failure_renamed)?;
        Ok(LocalRenameOutcome::new(true, durable))
    }
}

/// Validates that the requested rooted listing start exists as a directory.
fn validate_rooted_list_start(
    root: &crate::rooted::Root,
    path: &Path,
    symlink_policy: LocalSymlinkPolicy,
) -> LocalResult<()> {
    if path.as_os_str().is_empty() {
        let metadata = root.metadata().map_err(|error| {
            rooted_io_error(LocalFileOperation::List, path, error)
        })?;
        if metadata.kind() != crate::rooted::EntryKind::Directory {
            return Err(LocalFileError::new(
                LocalFileErrorKind::TypeConflict,
                LocalFileOperation::List,
            )
            .with_path(path.to_path_buf()));
        }
        return Ok(());
    }
    let path = resolve_rooted_path(
        root,
        path,
        symlink_policy,
        true,
        LocalFileOperation::List,
    )?;
    let metadata = root.symlink_metadata(&path).map_err(|error| {
        rooted_io_error(LocalFileOperation::List, path.as_path(), error)
    })?;
    if metadata.kind() != crate::rooted::EntryKind::Directory {
        return Err(LocalFileError::new(
            LocalFileErrorKind::TypeConflict,
            LocalFileOperation::List,
        )
        .with_path(path.as_path().to_path_buf()));
    }
    Ok(())
}

/// Opens a rooted path or its nearest existing ancestor for probing.
fn probe_rooted_file(
    root: &crate::rooted::Root,
    path: &Path,
    symlink_policy: LocalSymlinkPolicy,
    operation: LocalFileOperation,
) -> LocalResult<Option<File>> {
    let relative =
        resolve_rooted_path(root, path, symlink_policy, true, operation)?;
    let mut candidate = relative.as_path().to_path_buf();
    loop {
        if candidate.as_os_str().is_empty() {
            return root.open_probe_root().map(Some).map_err(|error| {
                LocalFileError::from_io(
                    operation,
                    Some(path.to_path_buf()),
                    None,
                    error,
                )
            });
        }
        let candidate_path = crate::local::LocalRelativePath::new(&candidate)
            .map_err(|error| {
            LocalFileError::from_io(
                operation,
                Some(path.to_path_buf()),
                None,
                error,
            )
        })?;
        match root.open_probe_file(&candidate_path) {
            Ok(file) => return Ok(Some(file)),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                if !candidate.pop() {
                    return Ok(None);
                }
            }
            Err(_) => return Ok(None),
        }
    }
}

/// Resolves rooted path components while preserving final-entry semantics.
pub(crate) fn resolve_rooted_path(
    root: &crate::rooted::Root,
    path: &Path,
    symlink_policy: LocalSymlinkPolicy,
    follow_final: bool,
    operation: LocalFileOperation,
) -> LocalResult<crate::local::LocalRelativePath> {
    let relative = rooted_path(path, operation)?;
    if !symlink_policy.follows() {
        return Ok(relative);
    }
    let authority_root = root
        .authority_path()
        .map_err(|error| rooted_io_error(operation, path, error))?;
    let diagnostic = authority_root.join(relative.as_path());
    let mut components = diagnostic.components().peekable();
    let mut current = PathBuf::new();
    let mut has_symlink = false;
    while let Some(component) = components.next() {
        current.push(component.as_os_str());
        if !matches!(component, std::path::Component::Normal(_)) {
            continue;
        }
        let is_final = components.peek().is_none();
        if is_final && !follow_final {
            break;
        }
        match fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                has_symlink = true;
                break;
            }
            Ok(_) => {}
            Err(error) if error.kind() == io::ErrorKind::NotFound => break,
            Err(error) => {
                return Err(rooted_io_error(operation, path, error));
            }
        }
    }
    if !has_symlink {
        return Ok(relative);
    }

    let resolved = if follow_final {
        fs::canonicalize(&diagnostic)
    } else {
        let parent = diagnostic.parent().unwrap_or(&authority_root);
        fs::canonicalize(parent).map(|parent| {
            parent.join(
                diagnostic
                    .file_name()
                    .expect("validated rooted paths have a final component"),
            )
        })
    }
    .map_err(|error| rooted_io_error(operation, path, error))?;
    let canonical_root = fs::canonicalize(&authority_root)
        .map_err(|error| rooted_io_error(operation, path, error))?;
    if resolved.starts_with(&canonical_root) {
        let relative = resolved
            .strip_prefix(&canonical_root)
            .expect("a contained path has a root prefix");
        let relative = crate::local::LocalRelativePath::new(relative)
            .map_err(|error| rooted_io_error(operation, path, error))?;
        return Ok(relative);
    }
    if symlink_policy == LocalSymlinkPolicy::FollowWithinScope {
        return Err(LocalFileError::new(
            LocalFileErrorKind::InvalidPath,
            operation,
        )
        .with_reason("symbolic-link resolution escaped the rooted scope")
        .with_path(path.to_path_buf()));
    }
    Err(
        LocalFileError::new(LocalFileErrorKind::InvalidOptions, operation)
            .with_reason(
                "FollowAcrossScope is not supported by Rooted filesystems because Rooted authority cannot escape its opened root",
            )
            .with_path(path.to_path_buf()),
    )
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
