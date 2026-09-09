// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Host reader, writer, metadata, and directory-walker operations.

use std::fs;
use std::path::Path;
use std::time::Instant;

use super::HostLocalFileSystem;
use super::bind_host_path;
use super::open_staged_writer;
use super::resolve_host_path;
use super::test_io_fault;
use crate::LocalAtomicityRequirement;
use crate::LocalDirectoryWalker;
use crate::LocalFileError;
use crate::LocalFileErrorKind;
use crate::LocalFileMetadata;
use crate::LocalFileOperation;
use crate::LocalFileReader;
use crate::LocalFileWriter;
use crate::LocalListOptions;
use crate::LocalReadOptions;
use crate::LocalResult;
use crate::LocalSymlinkPolicy;
use crate::LocalWriteMode;
use crate::LocalWriteOptions;
use crate::local::ensure_required_directory_durability;
use crate::writer::internal::LocalFileWriterBackend;

impl HostLocalFileSystem {
    /// Reads metadata using an explicit path-resolution policy.
    ///
    /// Keeps the final symbolic link as an entry. Returns path-binding,
    /// forbidden intermediate-link traversal, or native metadata errors.
    pub fn metadata_with_policy(path: &Path, symlink_policy: LocalSymlinkPolicy) -> LocalResult<LocalFileMetadata> {
        let bound = bind_host_path(path)?;
        let resolved = if symlink_policy == LocalSymlinkPolicy::FollowAcrossScope {
            bound.clone()
        } else {
            resolve_host_path(&bound, symlink_policy, false)?
        };
        #[cfg(feature = "test-support")]
        crate::test_support::record_host_metadata_query();
        fs::symlink_metadata(&resolved)
            .map(|metadata| LocalFileMetadata::from_native(&metadata))
            .map_err(|source| LocalFileError::from_io(LocalFileOperation::Metadata, Some(bound), None, source))
    }

    /// Opens a Host reader using an explicit symbolic-link policy.
    ///
    /// Resolves the final link according to the policy and requires a regular
    /// file. Returns resolution or native open errors, or `TypeConflict` for
    /// another resource kind. The returned reader owns the opened handle.
    pub fn open_reader_with_policy(
        path: &Path,
        options: &LocalReadOptions,
        symlink_policy: LocalSymlinkPolicy,
    ) -> LocalResult<LocalFileReader> {
        let bound = resolve_host_path(path, symlink_policy, true)?;
        let metadata = test_io_fault("local-fs-open-reader-metadata")
            .map_or_else(|| fs::metadata(&bound), Err)
            .map_err(|source| {
                LocalFileError::from_io(LocalFileOperation::OpenReader, Some(bound.clone()), None, source)
            })?;
        if !metadata.file_type().is_file() {
            return Err(
                LocalFileError::new(LocalFileErrorKind::TypeConflict, LocalFileOperation::OpenReader).with_path(bound),
            );
        }
        let native_options = options
            .open_retry_timeout()
            .map_or_else(crate::read::OpenOptions::default, |timeout| {
                crate::read::OpenOptions::default().with_open_retry_timeout(timeout)
            });
        test_io_fault("local-fs-open-reader-native")
            .map_or_else(|| crate::local::open_native_reader_path(&bound, &native_options), Err)
            .and_then(LocalFileReader::from_file)
            .map_err(|source| {
                #[cfg(windows)]
                if source.kind() == std::io::ErrorKind::InvalidInput
                    && fs::symlink_metadata(&bound).is_ok_and(|metadata| metadata.file_type().is_symlink())
                {
                    return LocalFileError::new(LocalFileErrorKind::TypeConflict, LocalFileOperation::OpenReader)
                        .with_path(bound);
                }
                LocalFileError::from_io(LocalFileOperation::OpenReader, Some(bound), None, source)
            })
    }

    /// Opens a Host writer using an explicit symbolic-link policy.
    ///
    /// Create modes prepare same-directory staging; append opens an existing
    /// regular file directly. Parent creation may persist after a later
    /// failure. Resolution, type, guarantee, and native preparation errors
    /// are returned before a writer is handed to the caller.
    pub fn open_writer_with_policy(
        path: &Path,
        options: &LocalWriteOptions,
        symlink_policy: LocalSymlinkPolicy,
    ) -> LocalResult<LocalFileWriter> {
        let follow_final = options.mode() != LocalWriteMode::CreateNew;
        let diagnostic_path = bind_host_path(path)?;
        let bound = resolve_host_path(&diagnostic_path, symlink_policy, follow_final)?;
        if options.mode() == LocalWriteMode::Append && options.atomicity() == LocalAtomicityRequirement::Required {
            return Err(
                LocalFileError::new(LocalFileErrorKind::RequirementNotMet, LocalFileOperation::OpenWriter)
                    .with_reason("append mode cannot provide required atomic publication")
                    .with_path(diagnostic_path.clone()),
            );
        }
        if options.mode() != LocalWriteMode::Append {
            let implements_durability = Self::capabilities().supports_durable_write();
            ensure_required_directory_durability(
                options.durability(),
                LocalFileOperation::OpenWriter,
                &diagnostic_path,
                &diagnostic_path,
                implements_durability,
                "required directory durability is unavailable on this host",
            )?;
        }
        if options.creates_parent()
            && let Some(error) = test_io_fault("local-fs-open-writer-parent")
        {
            return Err(LocalFileError::from_io(
                LocalFileOperation::OpenWriter,
                Some(diagnostic_path.clone()),
                None,
                error,
            ));
        }
        let backend = match options.mode() {
            LocalWriteMode::CreateNew | LocalWriteMode::CreateOrReplace => LocalFileWriterBackend::Staged(
                open_staged_writer(&bound, options).map_err(|error| error.with_path(diagnostic_path.clone()))?,
            ),
            LocalWriteMode::Append => {
                let metadata = test_io_fault("local-fs-open-writer-append-metadata")
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
                    return Err(
                        LocalFileError::new(LocalFileErrorKind::TypeConflict, LocalFileOperation::OpenWriter)
                            .with_path(diagnostic_path.clone()),
                    );
                }
                let mut native_options = crate::write::OpenOptions::new(crate::write::Mode::AppendExisting);
                if let Some(timeout) = options.open_retry_timeout() {
                    native_options = native_options.with_open_retry_timeout(timeout);
                }
                let file = test_io_fault("local-fs-open-writer-append-native")
                    .map_or_else(|| crate::local::open_native_writer_path(&bound, &native_options), Err)
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
    /// The list option's policy overrides `symlink_policy`. Resolution or
    /// initial directory-open failures are returned here; later enumeration
    /// errors are yielded by the walker.
    pub fn list_with_policy(
        path: &Path,
        options: &LocalListOptions,
        symlink_policy: LocalSymlinkPolicy,
        started_at: Instant,
    ) -> LocalResult<LocalDirectoryWalker> {
        let policy = options.symlink_policy().unwrap_or(symlink_policy);
        let bound = resolve_host_path(path, policy, true)?;
        #[cfg(windows)]
        let diagnostic = path.to_path_buf();
        #[cfg(not(windows))]
        let diagnostic = path.to_path_buf();
        #[cfg(target_os = "macos")]
        let diagnostic = logical_macos_path(&diagnostic);
        LocalDirectoryWalker::open_with_diagnostic(bound, diagnostic, *options, policy, started_at)
    }
}

/// Restores the stable `/var` spelling used by macOS Host callers.
#[cfg(target_os = "macos")]
fn logical_macos_path(path: &Path) -> std::path::PathBuf {
    let private_var = Path::new("/private/var");
    path.strip_prefix(private_var)
        .map_or_else(|_| path.to_path_buf(), |suffix| Path::new("/var").join(suffix))
}
