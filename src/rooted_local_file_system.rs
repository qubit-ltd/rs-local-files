// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::{
    io,
    path::Path,
    sync::Arc,
};

use crate::{
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
    LocalFileKind,
    LocalFileMetadata,
    LocalFileOperation,
    LocalFileReader,
    LocalFileSystemCapabilities,
    LocalFileWriter,
    LocalListOptions,
    LocalReadOptions,
    LocalRenameOptions,
    LocalRenameOutcome,
    LocalResult,
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
                LocalFileOperation::OpenReader,
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
    pub fn metadata(&self, path: &Path) -> LocalResult<LocalFileMetadata> {
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
            && !self.capabilities.supports_directory_durability()
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
                let mut atomic_options = crate::atomic::Options::new()
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
        let existed = match self.root.symlink_metadata(&relative) {
            Ok(_) => true,
            Err(error) if error.kind() == io::ErrorKind::NotFound => false,
            Err(error) => {
                return Err(rooted_io_error(
                    LocalFileOperation::CreateDirectory,
                    path,
                    error,
                ));
            }
        };
        let result = if options.recursive() {
            self.root.create_dir_all(&relative)
        } else {
            self.root.create_dir(&relative)
        };
        result
            .map(|()| LocalCreateDirectoryOutcome::new(!existed))
            .map_err(|error| {
                rooted_io_error(
                    LocalFileOperation::CreateDirectory,
                    path,
                    error,
                )
            })
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
    /// Returns `LocalFileError` for invalid descendants, symbolic links,
    /// conflicts, unsupported required guarantees, or native copy failures.
    pub fn copy(
        &self,
        source: &Path,
        target: &Path,
        options: &LocalCopyOptions,
    ) -> LocalResult<LocalCopyOutcome> {
        if options.durability() == LocalDurabilityRequirement::Required
            && !self.capabilities.supports_directory_durability()
        {
            return Err(LocalFileError::new(
                LocalFileErrorKind::RequirementNotMet,
                LocalFileOperation::Copy,
            )
            .with_path(source.to_path_buf())
            .with_target(target.to_path_buf()));
        }
        let source_path = rooted_path(source, LocalFileOperation::Copy)?;
        let target_path = rooted_path(target, LocalFileOperation::Copy)?;
        let metadata =
            self.root.symlink_metadata(&source_path).map_err(|error| {
                rooted_io_error(LocalFileOperation::Copy, source, error)
            })?;
        let directory = metadata.kind() == crate::rooted::EntryKind::Directory;
        if directory && !options.recursive() {
            return Err(LocalFileError::new(
                LocalFileErrorKind::RequirementNotMet,
                LocalFileOperation::Copy,
            )
            .with_path(source.to_path_buf())
            .with_target(target.to_path_buf()));
        }
        if directory
            && options.atomicity() == crate::LocalAtomicityRequirement::Required
        {
            return Err(LocalFileError::new(
                LocalFileErrorKind::RequirementNotMet,
                LocalFileOperation::Copy,
            )
            .with_path(source.to_path_buf())
            .with_target(target.to_path_buf()));
        }
        let stats = self
            .root
            .copy(
                &source_path,
                &target_path,
                crate::local_file_system::internal_copy_options(options),
            )
            .map_err(|error| {
                let kind = error.kind();
                LocalFileError::from_io(
                    LocalFileOperation::Copy,
                    Some(source.to_path_buf()),
                    Some(target.to_path_buf()),
                    io::Error::new(kind, error),
                )
            })?;
        let durable = rooted_published_durability(
            options.durability(),
            self.root.sync_parent(&target_path),
            LocalFileOperation::Copy,
            source,
            target,
        )?;
        Ok(LocalCopyOutcome::new(
            LocalCopyStats::from_internal(stats),
            if directory {
                LocalCopyMethod::Recursive
            } else {
                LocalCopyMethod::StagedFile
            },
            !directory,
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
    /// Returns `LocalFileError` for invalid descendants, conflicts, unsupported
    /// required durability, or native rename failures.
    pub fn rename(
        &self,
        source: &Path,
        target: &Path,
        options: &LocalRenameOptions,
    ) -> LocalResult<LocalRenameOutcome> {
        if options.durability() == LocalDurabilityRequirement::Required
            && !self.capabilities.supports_directory_durability()
        {
            return Err(LocalFileError::new(
                LocalFileErrorKind::RequirementNotMet,
                LocalFileOperation::Rename,
            )
            .with_path(source.to_path_buf())
            .with_target(target.to_path_buf()));
        }
        let source_path = rooted_path(source, LocalFileOperation::Rename)?;
        let target_path = rooted_path(target, LocalFileOperation::Rename)?;
        let result = if options.overwrite() {
            self.root.rename(&source_path, &target_path)
        } else {
            self.root
                .rename_without_replacing(&source_path, &target_path)
        };
        result
            .map_err(|error| {
                LocalFileError::from_io(
                    LocalFileOperation::Rename,
                    Some(source.to_path_buf()),
                    Some(target.to_path_buf()),
                    error,
                )
            })?;
        let durable = rooted_published_durability(
            options.durability(),
            self.root.sync_parent(&target_path),
            LocalFileOperation::Rename,
            source,
            target,
        )?;
        Ok(LocalRenameOutcome::new(true, durable))
    }
}

/// Converts rooted post-publication synchronization into an achieved guarantee.
#[inline]
fn rooted_published_durability(
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
