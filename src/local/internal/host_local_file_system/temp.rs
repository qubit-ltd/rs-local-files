// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

// Host temp operations.
// qubit-style: allow source-test-pair

use std::time::Instant;

use super::HostLocalFileSystem;
use super::LocalCopyOptions;
use super::LocalFileError;
use super::LocalFileErrorKind;
use super::LocalFileOperation;
use super::LocalMetadataPreservePolicy;
use super::LocalResult;
use super::LocalSymlinkPolicy;
use super::LocalTempDirectory;
use super::LocalTempDirectoryOptions;
use super::LocalTempFile;
use super::LocalTempFileOptions;
use super::LocalWriteMode;
use super::LocalWriteOptions;
use super::Path;
use super::fs;
use super::io;
use super::resolve_host_path;
use super::validate_temp_affixes;

impl HostLocalFileSystem {
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
        let parent = options.parent().map_or_else(std::env::temp_dir, Path::to_path_buf);
        let parent = resolve_host_path(&parent, symlink_policy, true)?;
        if options.creates_parent() {
            create_host_temp_parent(&parent, LocalFileOperation::CreateTempFile)?;
        }
        validate_host_temp_parent(&parent, LocalFileOperation::CreateTempFile)?;
        validate_temp_affixes(options.prefix(), options.suffix()).map_err(|error| {
            LocalFileError::from_io(LocalFileOperation::CreateTempFile, Some(parent.clone()), None, error)
                .with_kind(LocalFileErrorKind::InvalidOptions)
        })?;
        let sandbox =
            crate::local::create_temp_dir_in_dir_with_affixes(&parent, Some("sandbox-"), None, options.max_attempts())
                .map_err(|error| {
                    let invalid_options = error.kind() == io::ErrorKind::InvalidInput;
                    let error =
                        LocalFileError::from_io(LocalFileOperation::CreateTempFile, Some(parent.clone()), None, error);
                    if invalid_options {
                        error.with_kind(LocalFileErrorKind::InvalidOptions)
                    } else {
                        error
                    }
                })?;
        let created =
            crate::local::create_temp_file_in_dir(&sandbox, options.prefix(), options.suffix(), options.max_attempts());
        let result = match created {
            Ok((path, file)) => LocalTempFile::host(path, sandbox.clone(), file, symlink_policy),
            Err(error) => {
                let _ = std::fs::remove_dir_all(&sandbox);
                Err(error)
            }
        };
        result.map_err(|error| {
            let invalid_options = error.kind() == io::ErrorKind::InvalidInput;
            let error = LocalFileError::from_io(LocalFileOperation::CreateTempFile, Some(parent), None, error);
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
        let parent = options.parent().map_or_else(std::env::temp_dir, Path::to_path_buf);
        let parent = resolve_host_path(&parent, symlink_policy, true)?;
        if options.creates_parent() {
            create_host_temp_parent(&parent, LocalFileOperation::CreateTempDirectory)?;
        }
        validate_host_temp_parent(&parent, LocalFileOperation::CreateTempDirectory)?;
        validate_temp_affixes(options.prefix(), options.suffix()).map_err(|error| {
            LocalFileError::from_io(
                LocalFileOperation::CreateTempDirectory,
                Some(parent.clone()),
                None,
                error,
            )
            .with_kind(LocalFileErrorKind::InvalidOptions)
        })?;
        let sandbox =
            crate::local::create_temp_dir_in_dir_with_affixes(&parent, Some("sandbox-"), None, options.max_attempts())
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
            Ok(path) => LocalTempDirectory::host(path, sandbox.clone(), symlink_policy),
            Err(error) => {
                let _ = std::fs::remove_dir_all(&sandbox);
                Err(error)
            }
        };
        result.map_err(|error| {
            let invalid_options = error.kind() == io::ErrorKind::InvalidInput;
            let error = LocalFileError::from_io(LocalFileOperation::CreateTempDirectory, Some(parent), None, error);
            if invalid_options {
                error.with_kind(LocalFileErrorKind::InvalidOptions)
            } else {
                error
            }
        })
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
pub(crate) fn open_staged_writer(
    path: &Path,
    options: &LocalWriteOptions,
) -> LocalResult<crate::local::LocalAtomicWriter> {
    let mut native_options = crate::local::LocalAtomicWriteOptions::new().with_durability(options.durability());
    if options.mode() == LocalWriteMode::CreateNew {
        native_options = native_options.with_create_new();
    }
    if options.creates_parent() {
        native_options = native_options.with_create_parent();
    }
    if let Some(timeout) = options.open_retry_timeout() {
        native_options = native_options.with_open_retry_timeout(timeout);
    }
    crate::local::LocalAtomicWriter::new(path, native_options).map_err(|error| {
        let kind = error.kind();
        LocalFileError::from_io(
            LocalFileOperation::OpenWriter,
            Some(path.to_path_buf()),
            None,
            io::Error::new(kind, error),
        )
    })
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
    started_at: Instant,
) -> crate::local::LocalCopyDirOptions {
    let symlink_policy = options.symlink_policy_override().unwrap_or(symlink_policy);
    let mut result = crate::local::LocalCopyDirOptions::new()
        .with_conflict(options.conflict())
        .with_type_conflict(options.type_conflict())
        .with_symlink_policy(symlink_policy)
        .with_durability(options.durability())
        .with_started_at(started_at);
    if let Some(value) = options.max_depth() {
        result = result.with_max_depth(value);
    }
    if let Some(value) = options.max_entries() {
        result = result.with_max_entries(value);
    }
    if let Some(value) = options.max_bytes() {
        result = result.with_max_bytes(value);
    }
    if let Some(value) = options.max_open_directories() {
        result = result.with_max_open_directories(value);
    }
    if let Some(value) = options.deadline() {
        result = result.with_deadline(value);
    }
    if options.preserve_metadata() == LocalMetadataPreservePolicy::Permissions {
        result = result.preserve_permissions();
    }
    result
}

/// Confirms that a host temporary-resource parent is an existing directory.
// qubit-style: allow coverage-cfg
#[cfg_attr(not(coverage), inline)]
#[cfg_attr(coverage, inline(never))]
fn validate_host_temp_parent(parent: &Path, operation: LocalFileOperation) -> LocalResult<()> {
    let metadata = match fs::metadata(parent) {
        Ok(metadata) => metadata,
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
        return Err(LocalFileError::new(LocalFileErrorKind::NotDirectory, operation).with_path(parent.to_path_buf()));
    }
    Ok(())
}

/// Creates a missing Host temporary parent while deferring an existing-file
/// collision to the common type validator.
fn create_host_temp_parent(parent: &Path, operation: LocalFileOperation) -> LocalResult<()> {
    match fs::create_dir_all(parent) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => Ok(()),
        Err(error) => Err(LocalFileError::from_io(
            operation,
            Some(parent.to_path_buf()),
            None,
            error,
        )),
    }
}
