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
use super::LocalCreateDirectoryOptions;
use super::LocalCreateDirectoryOutcome;
use super::LocalDeleteOptions;
use super::LocalDeleteOutcome;
use super::LocalDirectoryWalker;
use super::LocalFileError;
use super::LocalFileErrorKind;
use super::LocalFileOperation;
use super::LocalFileSystem;
use super::LocalListOptions;
use super::LocalNamespace;
use super::LocalResult;
use super::Path;
use super::operation_error;
use super::operation_failure_path;
use super::reject_directory_qualified_file;
use super::resolve_operation_path;
use super::validate_list_options;
use super::with_current_directory;

impl LocalFileSystem {
    /// Opens a directory walker using this instance's default list options.
    pub fn list(&self, path: &Path) -> LocalResult<LocalDirectoryWalker> {
        self.list_with_options(path, &self.defaults.list)
    }

    /// Opens a walker using one complete explicit options value.
    pub fn list_with_options(&self, path: &Path, options: &LocalListOptions) -> LocalResult<LocalDirectoryWalker> {
        let resolver = self.resolver_for(path, LocalFileOperation::List)?;
        let resolved = resolve_operation_path(&resolver, path, LocalFileOperation::List)?;
        validate_list_options(
            self.scope(),
            self.symlink_policy,
            options,
            Some(resolved.namespace_absolute()),
        )
        .map_err(|error| with_current_directory(error, resolver.current_directory()))?;
        match &self.core.namespace {
            LocalNamespace::Host => {
                HostLocalFileSystem::list_with_policy(resolved.authority_relative(), options, self.symlink_policy)
            }
            LocalNamespace::Rooted(rooted) => rooted.list(
                resolved.authority_relative(),
                resolved.namespace_absolute(),
                options,
                self.symlink_policy,
            ),
        }
        .map(|walker| walker.bind_current_directory(resolver.current_directory().map(Path::to_path_buf)))
        .map_err(|error| {
            operation_error(
                error,
                LocalFileOperation::List,
                resolved.namespace_absolute(),
                None,
                resolver.current_directory(),
            )
        })
    }

    /// Creates a directory using this instance's default options.
    pub fn create_directory(&self, path: &Path) -> LocalResult<LocalCreateDirectoryOutcome> {
        self.create_directory_with_options(path, &self.defaults.create_directory)
    }

    /// Creates a directory using one complete explicit options value.
    pub fn create_directory_with_options(
        &self,
        path: &Path,
        options: &LocalCreateDirectoryOptions,
    ) -> LocalResult<LocalCreateDirectoryOutcome> {
        let resolver = self.resolver_for(path, LocalFileOperation::CreateDirectory)?;
        let resolved = resolve_operation_path(&resolver, path, LocalFileOperation::CreateDirectory)?;
        if self.is_root_operand(&resolved) {
            if options.exists_ok() {
                return Ok(LocalCreateDirectoryOutcome::new(false));
            }
            let error = LocalFileError::new(LocalFileErrorKind::AlreadyExists, LocalFileOperation::CreateDirectory)
                .with_path(resolved.namespace_absolute().to_path_buf());
            return Err(with_current_directory(error, resolver.current_directory()));
        }
        match &self.core.namespace {
            LocalNamespace::Host => HostLocalFileSystem::create_directory_with_policy(
                resolved.authority_relative(),
                options,
                self.symlink_policy,
            ),
            LocalNamespace::Rooted(rooted) => {
                rooted.create_directory(resolved.authority_relative(), options, self.symlink_policy)
            }
        }
        .map_err(|error| {
            let path = operation_failure_path(&error, self.scope(), resolved.namespace_absolute());
            operation_error(
                error,
                LocalFileOperation::CreateDirectory,
                &path,
                None,
                resolver.current_directory(),
            )
        })
    }

    /// Deletes a non-directory entry using this instance's default options.
    pub fn delete_file(&self, path: &Path) -> LocalResult<LocalDeleteOutcome> {
        self.delete_file_with_options(path, &self.defaults.delete)
    }

    /// Deletes a non-directory entry using complete explicit options.
    pub fn delete_file_with_options(
        &self,
        path: &Path,
        options: &LocalDeleteOptions,
    ) -> LocalResult<LocalDeleteOutcome> {
        let resolver = self.resolver_for(path, LocalFileOperation::DeleteFile)?;
        let resolved = resolve_operation_path(&resolver, path, LocalFileOperation::DeleteFile)?;
        self.reject_root_operand(&resolved, LocalFileOperation::DeleteFile, resolver.current_directory())?;
        reject_directory_qualified_file(&resolved, LocalFileOperation::DeleteFile, resolver.current_directory())?;
        match &self.core.namespace {
            LocalNamespace::Host => HostLocalFileSystem::delete_file_with_policy(
                resolved.authority_relative(),
                options,
                self.symlink_policy,
            ),
            LocalNamespace::Rooted(rooted) => {
                rooted.delete_file(resolved.authority_relative(), options, self.symlink_policy)
            }
        }
        .map_err(|error| {
            operation_error(
                error,
                LocalFileOperation::DeleteFile,
                resolved.namespace_absolute(),
                None,
                resolver.current_directory(),
            )
        })
    }

    /// Deletes a directory using this instance's default options.
    pub fn delete_directory(&self, path: &Path) -> LocalResult<LocalDeleteOutcome> {
        self.delete_directory_with_options(path, &self.defaults.delete)
    }

    /// Deletes a directory using complete explicit options.
    pub fn delete_directory_with_options(
        &self,
        path: &Path,
        options: &LocalDeleteOptions,
    ) -> LocalResult<LocalDeleteOutcome> {
        let resolver = self.resolver_for(path, LocalFileOperation::DeleteDirectory)?;
        let resolved = resolve_operation_path(&resolver, path, LocalFileOperation::DeleteDirectory)?;
        self.reject_root_operand(
            &resolved,
            LocalFileOperation::DeleteDirectory,
            resolver.current_directory(),
        )?;
        match &self.core.namespace {
            LocalNamespace::Host => HostLocalFileSystem::delete_directory_with_policy(
                resolved.authority_relative(),
                options,
                self.symlink_policy,
            ),
            LocalNamespace::Rooted(rooted) => {
                rooted.delete_directory(resolved.authority_relative(), options, self.symlink_policy)
            }
        }
        .map_err(|error| {
            let path = operation_failure_path(&error, self.scope(), resolved.namespace_absolute());
            operation_error(
                error,
                LocalFileOperation::DeleteDirectory,
                &path,
                None,
                resolver.current_directory(),
            )
        })
    }
}
