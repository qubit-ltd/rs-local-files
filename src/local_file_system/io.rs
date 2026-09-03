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
use super::resolve_operation_path;
use super::with_current_directory;

impl LocalFileSystem {
    /// Reads final-entry metadata without following the final symlink.
    pub fn metadata(&self, path: &Path) -> LocalResult<LocalFileMetadata> {
        let resolver = self.resolver_for(path, LocalFileOperation::Metadata)?;
        let resolved = resolve_operation_path(&resolver, path, LocalFileOperation::Metadata)?;
        self.core
            .fail_if_requested(crate::test_support::TestFaultPoint::Metadata)
            .map_err(|error| {
                with_current_directory(
                    LocalFileError::from_io(LocalFileOperation::Metadata, Some(path.to_path_buf()), None, error),
                    resolver.current_directory(),
                )
            })?;
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
                resolver.current_directory(),
            )
        })?;
        if resolved.directory_required() && metadata.kind() != LocalFileKind::Directory {
            let error = LocalFileError::new(LocalFileErrorKind::NotDirectory, LocalFileOperation::Metadata)
                .with_path(resolved.namespace_absolute().to_path_buf());
            return Err(with_current_directory(error, resolver.current_directory()));
        }
        Ok(metadata)
    }

    /// Opens a reader using this instance's default reader options.
    pub fn open_reader(&self, path: &Path) -> LocalResult<LocalFileReader> {
        self.open_reader_with_options(path, &self.defaults.read)
    }

    /// Opens a reader using one complete explicit options value.
    pub fn open_reader_with_options(&self, path: &Path, options: &LocalReadOptions) -> LocalResult<LocalFileReader> {
        let resolver = self.resolver_for(path, LocalFileOperation::OpenReader)?;
        let resolved = resolve_operation_path(&resolver, path, LocalFileOperation::OpenReader)?;
        reject_directory_qualified_file(&resolved, LocalFileOperation::OpenReader, resolver.current_directory())?;
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
                resolver.current_directory(),
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
        let resolver = self.resolver_for(path, LocalFileOperation::Read)?;
        let error_path = resolve_operation_path(&resolver, path, LocalFileOperation::Read)?
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
            #[cfg(feature = "test-support")]
            if crate::local::take_test_support("local-fs-read-prefix-read") {
                let error = LocalFileError::from_io(
                    LocalFileOperation::Read,
                    Some(error_path),
                    None,
                    std::io::Error::other("injected prefix read failure"),
                );
                return Err(with_current_directory(error, resolver.current_directory()));
            }
            let count = reader.read(&mut buffer[..read_len]).map_err(|source| {
                with_current_directory(
                    LocalFileError::from_io(LocalFileOperation::Read, Some(error_path.clone()), None, source),
                    resolver.current_directory(),
                )
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
        let resolver = self.resolver_for(path, LocalFileOperation::OpenWriter)?;
        let resolved = resolve_operation_path(&resolver, path, LocalFileOperation::OpenWriter)?;
        self.reject_root_operand(&resolved, LocalFileOperation::OpenWriter, resolver.current_directory())?;
        reject_directory_qualified_file(&resolved, LocalFileOperation::OpenWriter, resolver.current_directory())?;
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
                resolver.current_directory().map(Path::to_path_buf),
            )
        })
        .map_err(|error| {
            operation_error(
                error,
                LocalFileOperation::OpenWriter,
                resolved.namespace_absolute(),
                None,
                resolver.current_directory(),
            )
        })
    }
}
