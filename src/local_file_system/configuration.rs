// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Stateful Host or Rooted local filesystem service.
// qubit-style: allow source-test-pair

#[cfg(feature = "test-support")]
use super::Arc;
use super::LocalCopyOptions;
use super::LocalCreateDirectoryOptions;
use super::LocalDeleteOptions;
use super::LocalFileOperation;
use super::LocalFileSystem;
#[cfg(feature = "test-support")]
use super::LocalFileSystemCore;
use super::LocalFileSystemScope;
use super::LocalListOptions;
use super::LocalNamespace;
use super::LocalReadOptions;
use super::LocalRenameOptions;
use super::LocalResult;
use super::LocalSymlinkPolicy;
use super::LocalTempDirectoryOptions;
use super::LocalTempFileOptions;
use super::LocalWriteOptions;
use super::Path;
use super::operation_error;
use super::resolve_operation_path;
use super::validate_copy_options;
use super::validate_list_options;
use super::validate_scope_symlink_policy;
use super::validate_temp_attempts;

impl LocalFileSystem {
    /// Returns the namespace kind interpreted by this instance.
    #[inline]
    pub fn scope(&self) -> LocalFileSystemScope {
        match &self.core.namespace {
            LocalNamespace::Host => LocalFileSystemScope::Host,
            LocalNamespace::Rooted(_) => LocalFileSystemScope::Rooted,
        }
    }

    /// Returns this instance's normalized namespace-absolute PWD.
    #[inline]
    pub fn current_directory(&self) -> &Path {
        &self.current_directory
    }

    /// Changes this instance's PWD after resolving and validating a directory.
    pub fn set_current_directory(&mut self, path: &Path) -> LocalResult<()> {
        let resolver = self.resolver();
        let resolved = resolve_operation_path(&resolver, path, LocalFileOperation::SetCurrentDirectory)?;
        self.validate_directory(&resolved).map_err(|error| {
            operation_error(
                error,
                LocalFileOperation::SetCurrentDirectory,
                resolved.namespace_absolute(),
                None,
                self.current_directory(),
            )
        })?;
        self.current_directory = resolved.namespace_absolute().to_path_buf();
        Ok(())
    }

    /// Returns the default symlink policy inherited by operations.
    #[cfg_attr(feature = "test-support", inline(never))]

    pub const fn symlink_policy(&self) -> LocalSymlinkPolicy {
        self.symlink_policy
    }

    /// Changes this instance's default symlink policy transactionally.
    pub fn set_symlink_policy(&mut self, policy: LocalSymlinkPolicy) -> LocalResult<()> {
        validate_scope_symlink_policy(self.scope(), policy, LocalFileOperation::Configure, None)
            .map_err(|error| error.with_current_directory(self.current_directory.clone()))?;
        self.symlink_policy = policy;
        Ok(())
    }

    /// Returns the construction-time Rooted path used only for diagnostics.
    #[cfg_attr(feature = "test-support", inline(never))]

    pub fn diagnostic_root(&self) -> Option<&Path> {
        match &self.core.namespace {
            LocalNamespace::Host => None,
            LocalNamespace::Rooted(rooted) => Some(rooted.diagnostic_path()),
        }
    }

    /// Returns the default reader options.
    #[cfg_attr(feature = "test-support", inline(never))]

    pub const fn default_read_options(&self) -> &LocalReadOptions {
        &self.defaults.read
    }

    /// Replaces the default reader options.
    pub fn set_default_read_options(&mut self, options: LocalReadOptions) -> LocalResult<()> {
        self.defaults.read = options;
        Ok(())
    }

    /// Returns the default writer options.
    #[cfg_attr(feature = "test-support", inline(never))]

    pub const fn default_write_options(&self) -> &LocalWriteOptions {
        &self.defaults.write
    }

    /// Replaces the default writer options.
    pub fn set_default_write_options(&mut self, options: LocalWriteOptions) -> LocalResult<()> {
        self.defaults.write = options;
        Ok(())
    }

    /// Returns the default listing options.
    #[cfg_attr(feature = "test-support", inline(never))]

    pub const fn default_list_options(&self) -> &LocalListOptions {
        &self.defaults.list
    }

    /// Replaces the default listing options after structural validation.
    pub fn set_default_list_options(&mut self, options: LocalListOptions) -> LocalResult<()> {
        validate_list_options(self.scope(), self.symlink_policy, &options, None)
            .map_err(|error| error.with_current_directory(self.current_directory.clone()))?;
        self.defaults.list = options;
        Ok(())
    }

    /// Returns the default copy options.
    #[cfg_attr(feature = "test-support", inline(never))]

    pub const fn default_copy_options(&self) -> &LocalCopyOptions {
        &self.defaults.copy
    }

    /// Replaces the default copy options after structural validation.
    pub fn set_default_copy_options(&mut self, options: LocalCopyOptions) -> LocalResult<()> {
        validate_copy_options(self.scope(), self.symlink_policy, &options, None, None)
            .map_err(|error| error.with_current_directory(self.current_directory.clone()))?;
        self.defaults.copy = options;
        Ok(())
    }

    /// Returns the default directory-creation options.
    #[cfg_attr(feature = "test-support", inline(never))]

    pub const fn default_create_directory_options(&self) -> &LocalCreateDirectoryOptions {
        &self.defaults.create_directory
    }

    /// Replaces the default directory-creation options.
    pub fn set_default_create_directory_options(&mut self, options: LocalCreateDirectoryOptions) -> LocalResult<()> {
        self.defaults.create_directory = options;
        Ok(())
    }

    /// Returns the default deletion options.
    #[cfg_attr(feature = "test-support", inline(never))]

    pub const fn default_delete_options(&self) -> &LocalDeleteOptions {
        &self.defaults.delete
    }

    /// Replaces the default deletion options.
    pub fn set_default_delete_options(&mut self, options: LocalDeleteOptions) -> LocalResult<()> {
        self.defaults.delete = options;
        Ok(())
    }

    /// Returns the default rename options.
    #[cfg_attr(feature = "test-support", inline(never))]

    pub const fn default_rename_options(&self) -> &LocalRenameOptions {
        &self.defaults.rename
    }

    /// Replaces the default rename options.
    pub fn set_default_rename_options(&mut self, options: LocalRenameOptions) -> LocalResult<()> {
        self.defaults.rename = options;
        Ok(())
    }

    /// Returns the default temporary-file options.
    #[cfg_attr(feature = "test-support", inline(never))]

    pub const fn default_temp_file_options(&self) -> &LocalTempFileOptions {
        &self.defaults.temp_file
    }

    /// Replaces the default temporary-file options after validation.
    pub fn set_default_temp_file_options(&mut self, options: LocalTempFileOptions) -> LocalResult<()> {
        validate_temp_attempts(options.max_attempts(), LocalFileOperation::Configure)
            .map_err(|error| error.with_current_directory(self.current_directory.clone()))?;
        self.defaults.temp_file = options;
        Ok(())
    }

    /// Returns the default temporary-directory options.
    #[cfg_attr(feature = "test-support", inline(never))]

    pub const fn default_temp_directory_options(&self) -> &LocalTempDirectoryOptions {
        &self.defaults.temp_directory
    }

    /// Replaces the default temporary-directory options after validation.
    pub fn set_default_temp_directory_options(&mut self, options: LocalTempDirectoryOptions) -> LocalResult<()> {
        validate_temp_attempts(options.max_attempts(), LocalFileOperation::Configure)
            .map_err(|error| error.with_current_directory(self.current_directory.clone()))?;
        self.defaults.temp_directory = options;
        Ok(())
    }

    /// Installs an instance-local fault plan in test-support builds.
    #[cfg(feature = "test-support")]
    #[doc(hidden)]
    #[must_use]
    #[inline]
    pub fn with_test_faults(mut self, test_faults: Option<crate::TestFaultPlan>) -> Self {
        self.core = Arc::new(LocalFileSystemCore {
            namespace: self.core.namespace.clone(),
            capabilities: self.core.capabilities,
            limits: self.core.limits,
            test_faults,
        });
        self
    }
}
