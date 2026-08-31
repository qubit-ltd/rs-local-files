// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Stateful Host or Rooted local filesystem service.
// qubit-style: allow source-test-pair

use super::HostLocalFileSystem;
use super::LocalFileError;
use super::LocalFileErrorKind;
use super::LocalFileKind;
use super::LocalFileMetadata;
use super::LocalFileOperation;
use super::LocalFileReader;
use super::LocalFileSystem;
use super::LocalFileWriter;
use super::LocalNamespace;
use super::LocalReadOptions;
use super::LocalResult;
use super::LocalWriteOptions;
use super::Path;
use super::Read;
use super::operation_error;
use super::reject_directory_qualified_file;

impl LocalFileSystem {
    /// Reads final-entry metadata without following the final symlink.
    pub fn metadata(&self, path: &Path) -> LocalResult<LocalFileMetadata> {
        self.core
            .fail_if_requested(crate::test_support::TestFaultPoint::Metadata)
            .map_err(|error| {
                LocalFileError::from_io(LocalFileOperation::Metadata, Some(path.to_path_buf()), None, error)
                    .with_current_directory(self.current_directory.clone())
            })?;
        let resolved = self.resolve(path, LocalFileOperation::Metadata)?;
        let metadata = match &self.core.namespace {
            LocalNamespace::Host => {
                HostLocalFileSystem::metadata_with_policy(resolved.authority_relative(), self.symlink_policy)
            }
            LocalNamespace::Rooted(rooted) => rooted.metadata(resolved.authority_relative(), self.symlink_policy),
        }
        .map_err(|error| {
            operation_error(
                error,
                LocalFileOperation::Metadata,
                resolved.namespace_absolute(),
                None,
                self.current_directory(),
            )
        })?;
        if resolved.directory_required() && metadata.kind() != LocalFileKind::Directory {
            return Err(
                LocalFileError::new(LocalFileErrorKind::NotDirectory, LocalFileOperation::Metadata)
                    .with_path(resolved.namespace_absolute().to_path_buf())
                    .with_current_directory(self.current_directory.clone()),
            );
        }
        Ok(metadata)
    }

    /// Opens a reader using this instance's default reader options.
    pub fn open_reader(&self, path: &Path) -> LocalResult<LocalFileReader> {
        self.open_reader_with_options(path, &self.defaults.read)
    }

    /// Opens a reader using one complete explicit options value.
    pub fn open_reader_with_options(&self, path: &Path, options: &LocalReadOptions) -> LocalResult<LocalFileReader> {
        let resolved = self.resolve(path, LocalFileOperation::OpenReader)?;
        reject_directory_qualified_file(&resolved, LocalFileOperation::OpenReader, self.current_directory())?;
        match &self.core.namespace {
            LocalNamespace::Host => HostLocalFileSystem::open_reader_with_policy(
                resolved.authority_relative(),
                options,
                self.symlink_policy,
            ),
            LocalNamespace::Rooted(rooted) => {
                rooted.open_reader(resolved.authority_relative(), options, self.symlink_policy)
            }
        }
        .map_err(|error| {
            operation_error(
                error,
                LocalFileOperation::OpenReader,
                resolved.namespace_absolute(),
                None,
                self.current_directory(),
            )
        })
    }

    /// Reads at most `max_bytes` using the default reader options.
    pub fn read_prefix(&self, path: &Path, max_bytes: usize) -> LocalResult<Vec<u8>> {
        self.read_prefix_with_options(path, max_bytes, &self.defaults.read)
    }

    /// Reads at most `max_bytes` using one complete explicit options value.
    pub fn read_prefix_with_options(
        &self,
        path: &Path,
        max_bytes: usize,
        options: &LocalReadOptions,
    ) -> LocalResult<Vec<u8>> {
        let error_path = self
            .resolve(path, LocalFileOperation::Read)?
            .namespace_absolute()
            .to_path_buf();
        let mut reader = self.open_reader_with_options(path, options)?;
        if max_bytes == 0 {
            return Ok(Vec::new());
        }
        let mut result = Vec::with_capacity(max_bytes.min(8192));
        let mut buffer = [0_u8; 8192];
        while result.len() < max_bytes {
            let read_len = (max_bytes - result.len()).min(buffer.len());
            #[cfg(feature = "internal-test-support")]
            if crate::local::take_test_support("local-fs-read-prefix-read") {
                return Err(LocalFileError::from_io(
                    LocalFileOperation::Read,
                    Some(error_path),
                    None,
                    std::io::Error::other("injected prefix read failure"),
                )
                .with_current_directory(self.current_directory.clone()));
            }
            let count = reader.read(&mut buffer[..read_len]).map_err(|source| {
                LocalFileError::from_io(LocalFileOperation::Read, Some(error_path.clone()), None, source)
                    .with_current_directory(self.current_directory.clone())
            })?;
            if count == 0 {
                break;
            }
            result.extend_from_slice(&buffer[..count]);
        }
        Ok(result)
    }

    /// Opens a writer using this instance's default writer options.
    pub fn open_writer(&self, path: &Path) -> LocalResult<LocalFileWriter> {
        self.open_writer_with_options(path, &self.defaults.write)
    }

    /// Opens a writer using one complete explicit options value.
    pub fn open_writer_with_options(&self, path: &Path, options: &LocalWriteOptions) -> LocalResult<LocalFileWriter> {
        let resolved = self.resolve(path, LocalFileOperation::OpenWriter)?;
        self.reject_root_operand(&resolved, LocalFileOperation::OpenWriter)?;
        reject_directory_qualified_file(&resolved, LocalFileOperation::OpenWriter, self.current_directory())?;
        match &self.core.namespace {
            LocalNamespace::Host => HostLocalFileSystem::open_writer_with_policy(
                resolved.authority_relative(),
                options,
                self.symlink_policy,
            ),
            LocalNamespace::Rooted(rooted) => {
                rooted.open_writer(resolved.authority_relative(), options, self.symlink_policy)
            }
        }
        .map(|writer| {
            writer.bind_namespace(
                resolved.namespace_absolute().to_path_buf(),
                self.current_directory.clone(),
            )
        })
        .map_err(|error| {
            operation_error(
                error,
                LocalFileOperation::OpenWriter,
                resolved.namespace_absolute(),
                None,
                self.current_directory(),
            )
        })
    }
}
