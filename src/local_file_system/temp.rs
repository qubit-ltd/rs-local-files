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
use super::LocalFileOperation;
use super::LocalFileSystem;
use super::LocalNamespace;
use super::LocalResult;
use super::LocalTempDirectory;
use super::LocalTempDirectoryOptions;
use super::LocalTempFile;
use super::LocalTempFileOptions;
use super::Path;
use super::operation_error;
use super::resolve_operation_path;
use super::validate_temp_attempts;

impl LocalFileSystem {
    /// Creates a temporary file using this instance's default options.
    pub fn create_temp_file(&self) -> LocalResult<LocalTempFile> {
        self.create_temp_file_with_options(&self.defaults.temp_file)
    }

    /// Creates a temporary file using one complete explicit options value.
    pub fn create_temp_file_with_options(&self, options: &LocalTempFileOptions) -> LocalResult<LocalTempFile> {
        validate_temp_attempts(options.max_attempts(), LocalFileOperation::CreateTempFile)
            .map_err(|error| error.with_current_directory(self.current_directory.clone()))?;
        let parent = options.parent().unwrap_or_else(|| Path::new(""));
        let resolver = self.resolver();
        let resolved = resolve_operation_path(&resolver, parent, LocalFileOperation::CreateTempFile)?;
        let options = options.clone().with_parent(resolved.authority_relative());
        let resource = match &self.core.namespace {
            LocalNamespace::Host => HostLocalFileSystem::create_temp_file_with_policy(&options, self.symlink_policy),
            LocalNamespace::Rooted(rooted) => rooted.create_temp_file(&options, self.symlink_policy),
        }
        .map_err(|error| {
            operation_error(
                error,
                LocalFileOperation::CreateTempFile,
                resolved.namespace_absolute(),
                None,
                self.current_directory(),
            )
        })?;
        resource.bind_namespace(resolver)
    }

    /// Creates a temporary directory using this instance's default options.
    pub fn create_temp_directory(&self) -> LocalResult<LocalTempDirectory> {
        self.create_temp_directory_with_options(&self.defaults.temp_directory)
    }

    /// Creates a temporary directory using one complete explicit options value.
    pub fn create_temp_directory_with_options(
        &self,
        options: &LocalTempDirectoryOptions,
    ) -> LocalResult<LocalTempDirectory> {
        validate_temp_attempts(options.max_attempts(), LocalFileOperation::CreateTempDirectory)
            .map_err(|error| error.with_current_directory(self.current_directory.clone()))?;
        let parent = options.parent().unwrap_or_else(|| Path::new(""));
        let resolver = self.resolver();
        let resolved = resolve_operation_path(&resolver, parent, LocalFileOperation::CreateTempDirectory)?;
        let options = options.clone().with_parent(resolved.authority_relative());
        let resource = match &self.core.namespace {
            LocalNamespace::Host => {
                HostLocalFileSystem::create_temp_directory_with_policy(&options, self.symlink_policy)
            }
            LocalNamespace::Rooted(rooted) => rooted.create_temp_directory(&options, self.symlink_policy),
        }
        .map_err(|error| {
            operation_error(
                error,
                LocalFileOperation::CreateTempDirectory,
                resolved.namespace_absolute(),
                None,
                self.current_directory(),
            )
        })?;
        resource.bind_namespace(resolver)
    }
}
