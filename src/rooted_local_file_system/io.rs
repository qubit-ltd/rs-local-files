// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

// Rooted io operations.
// qubit-style: allow source-test-pair

use super::Arc;
use super::LocalDirectoryWalker;
use super::LocalFileError;
use super::LocalFileErrorKind;
use super::LocalFileOperation;
use super::LocalFileReader;
use super::LocalFileWriter;
use super::LocalListOptions;
use super::LocalReadOptions;
use super::LocalResult;
use super::LocalSymlinkPolicy;
use super::LocalWriteMode;
use super::LocalWriteOptions;
use super::Path;
use super::PathBuf;
use super::RootedLocalFileSystem;
use super::ensure_required_directory_durability;
use super::io;
use super::resolve_rooted_path;
use super::rooted_io_error;
use super::rooted_path;
use super::validate_rooted_list_start;

impl RootedLocalFileSystem {
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
        let relative = resolve_rooted_path(&self.root, path, symlink_policy, true, LocalFileOperation::OpenReader)?;
        let native_options = options
            .open_retry_timeout()
            .map_or_else(crate::read::OpenOptions::default, |timeout| {
                crate::read::OpenOptions::default().with_open_retry_timeout(timeout)
            });
        let metadata = self
            .root
            .symlink_metadata(&relative)
            .map_err(|error| rooted_io_error(LocalFileOperation::OpenReader, path, error))?;
        if metadata.kind() != crate::rooted::EntryKind::File {
            return Err(
                LocalFileError::new(LocalFileErrorKind::TypeConflict, LocalFileOperation::OpenReader)
                    .with_path(path.to_path_buf()),
            );
        }
        self.root
            .open_reader(&relative, &native_options)
            .map(LocalFileReader::new)
            .map_err(|error| rooted_io_error(LocalFileOperation::OpenReader, path, error))
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
            let resolved = resolve_rooted_path(&self.root, path, symlink_policy, true, LocalFileOperation::List)?;
            if relative.as_ref() != Some(&resolved) {
                return LocalDirectoryWalker::open_rooted_with_output(
                    Arc::clone(&self.root),
                    Some(resolved),
                    relative
                        .as_ref()
                        .map_or_else(PathBuf::new, |path| path.as_path().to_path_buf()),
                    *options,
                    symlink_policy,
                );
            }
        }
        LocalDirectoryWalker::open_rooted(Arc::clone(&self.root), relative, *options, symlink_policy)
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

        if options.mode() == LocalWriteMode::Append && options.atomicity() == crate::LocalAtomicityRequirement::Required
        {
            return Err(
                LocalFileError::new(LocalFileErrorKind::RequirementNotMet, LocalFileOperation::OpenWriter)
                    .with_reason("append mode cannot provide required atomic publication")
                    .with_path(path.to_path_buf()),
            );
        }
        if options.mode() != LocalWriteMode::Append {
            ensure_required_directory_durability(
                options.durability(),
                LocalFileOperation::OpenWriter,
                path,
                path,
                self.capabilities.supports_durable_file_copy(),
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
                let mut atomic_options = crate::LocalAtomicWriteOptions::new().with_durability(options.durability());
                if options.mode() == LocalWriteMode::CreateNew {
                    atomic_options = atomic_options.with_create_new();
                }
                if options.creates_parent() {
                    atomic_options = atomic_options.with_parent();
                }
                if let Some(timeout) = options.open_retry_timeout() {
                    atomic_options = atomic_options.with_open_retry_timeout(timeout);
                }
                let writer = self
                    .root
                    .begin_atomic_write_with_options(&relative, atomic_options)
                    .map_err(|error| {
                        let kind = error.kind();
                        rooted_io_error(LocalFileOperation::OpenWriter, path, io::Error::new(kind, error))
                    })?;
                LocalFileWriterBackend::Rooted(writer)
            }
            LocalWriteMode::Append => {
                let metadata = self
                    .root
                    .symlink_metadata(&relative)
                    .map_err(|error| rooted_io_error(LocalFileOperation::OpenWriter, path, error))?;
                if metadata.kind() != crate::rooted::EntryKind::File {
                    return Err(
                        LocalFileError::new(LocalFileErrorKind::TypeConflict, LocalFileOperation::OpenWriter)
                            .with_path(path.to_path_buf()),
                    );
                }
                let mut native_options = crate::write::OpenOptions::new(crate::write::Mode::AppendExisting);
                if let Some(timeout) = options.open_retry_timeout() {
                    native_options = native_options.with_open_retry_timeout(timeout);
                }
                let file = self
                    .root
                    .open_writer(&relative, &native_options)
                    .map_err(|error| rooted_io_error(LocalFileOperation::OpenWriter, path, error))?;
                LocalFileWriterBackend::Append(file)
            }
        };
        let diagnostic = self.root.path().join(relative.as_path());
        Ok(LocalFileWriter::new(diagnostic, backend, *options))
    }
}
