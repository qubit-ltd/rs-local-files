// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
// =============================================================================
//! Host reader, writer, metadata, and directory-walker operations.

use std::fs;
use std::path::Path;

use crate::local::ensure_required_directory_durability;
use crate::writer::internal::LocalFileWriterBackend;
use crate::{
    LocalAtomicityRequirement,
    LocalDirectoryWalker,
    LocalFileError,
    LocalFileErrorKind,
    LocalFileMetadata,
    LocalFileOperation,
    LocalFileReader,
    LocalFileWriter,
    LocalListOptions,
    LocalPaths,
    LocalReadOptions,
    LocalResult,
    LocalSymlinkPolicy,
    LocalWriteMode,
    LocalWriteOptions,
};

use super::{
    HostLocalFileSystem,
    coverage_io_fault,
    open_staged_writer,
    resolve_host_path,
};

impl HostLocalFileSystem {
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
            let supports = Self::capabilities().supports_durable_file_copy();
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
            LocalWriteMode::CreateNew | LocalWriteMode::CreateOrReplace => {
                LocalFileWriterBackend::Staged(
                    open_staged_writer(&bound, options).map_err(|error| {
                        error.with_path(diagnostic_path.clone())
                    })?,
                )
            }
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
}
