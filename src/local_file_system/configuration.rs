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
use super::LocalFileError;
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
use super::with_current_directory;
use crate::local_file_system_validation::validate_rename_options;
use crate::local_file_system_validation::validate_temp_options;
use crate::local_file_system_validation::validate_write_options;

impl LocalFileSystem {
    /// Returns the namespace kind interpreted by this instance.
    ///
    /// # Returns
    ///
    /// `Host` for process-wide native paths or `Rooted` for paths interpreted
    /// below a retained directory authority.
    #[must_use = "inspect which namespace this filesystem uses"]
    #[inline]
    pub fn scope(&self) -> LocalFileSystemScope {
        match &self.core.namespace {
            LocalNamespace::Host => LocalFileSystemScope::Host,
            LocalNamespace::Rooted(_) => LocalFileSystemScope::Rooted,
        }
    }

    /// Returns the process PWD for Host or the retained virtual PWD for Rooted.
    ///
    /// Host queries the native process state on every call and returns an error
    /// when that state is unavailable. Rooted returns an owned virtual
    /// snapshot. The returned path is owned and can outlive this filesystem.
    ///
    /// # Errors
    ///
    /// Returns [`LocalFileError`] only when a Host instance cannot read the
    /// process working directory.
    pub fn current_directory(&self) -> LocalResult<std::path::PathBuf> {
        self.current_directory
            .snapshot(LocalFileOperation::CurrentDirectory, None)
    }

    /// Changes the process PWD for Host or the virtual PWD for Rooted.
    ///
    /// A Host change is process-global: the mutable receiver does not isolate
    /// this state from other Host instances or relative-path users. Prefer
    /// absolute Host paths in library and multi-threaded code.
    ///
    /// Host delegates directly to the native process operation. Rooted resolves
    /// and validates the requested directory before changing instance state.
    ///
    /// # Parameters
    ///
    /// - `path`: Host-native path or Rooted virtual path to select.
    ///
    /// # Errors
    ///
    /// Returns [`LocalFileError`] when the path cannot be resolved, escapes a
    /// Rooted authority, violates the symlink policy, or is not a directory.
    /// On error, a Rooted instance retains its previous virtual PWD.
    pub fn set_current_directory(&mut self, path: &Path) -> LocalResult<()> {
        if self.scope() == LocalFileSystemScope::Host {
            return std::env::set_current_dir(path).map_err(|source| {
                LocalFileError::from_io(
                    LocalFileOperation::SetCurrentDirectory,
                    Some(path.to_path_buf()),
                    None,
                    source,
                )
            });
        }
        let resolver = self.resolver_for(path, LocalFileOperation::SetCurrentDirectory)?;
        let resolved = resolve_operation_path(&resolver, path, LocalFileOperation::SetCurrentDirectory)?;
        self.validate_directory(&resolved).map_err(|error| {
            operation_error(
                error,
                LocalFileOperation::SetCurrentDirectory,
                resolved.namespace_absolute(),
                None,
                resolver.current_directory(),
            )
        })?;
        let replaced = self
            .current_directory
            .replace_virtual(resolved.namespace_absolute().to_path_buf());
        debug_assert!(replaced, "Rooted filesystem must retain a virtual PWD");
        Ok(())
    }

    /// Returns the default symlink policy inherited by operations that do not
    /// supply an explicit policy.
    #[must_use = "inspect the default symbolic-link policy"]
    #[inline]
    pub const fn symlink_policy(&self) -> LocalSymlinkPolicy {
        self.symlink_policy
    }

    /// Changes this instance's default symlink policy transactionally.
    ///
    /// # Parameters
    ///
    /// - `policy`: Policy inherited by subsequent operations.
    ///
    /// # Errors
    ///
    /// Returns [`LocalFileError`] when the policy is invalid for the current
    /// scope. The previous policy remains installed on error.
    pub fn set_symlink_policy(&mut self, policy: LocalSymlinkPolicy) -> LocalResult<()> {
        validate_scope_symlink_policy(self.scope(), policy, LocalFileOperation::Configure, None)
            .map_err(|error| with_current_directory(error, self.current_directory.virtual_path()))?;
        self.symlink_policy = policy;
        Ok(())
    }

    /// Returns the construction-time Rooted path used only for diagnostics.
    ///
    /// # Returns
    ///
    /// `Some` for a Rooted filesystem and `None` for Host. The path is not an
    /// authority and may become stale after a native rename or replacement.
    #[must_use = "inspect the default options"]
    #[inline]
    pub fn diagnostic_root(&self) -> Option<&Path> {
        match &self.core.namespace {
            LocalNamespace::Host => None,
            LocalNamespace::Rooted(rooted) => Some(rooted.diagnostic_path()),
        }
    }

    /// Returns the reader options inherited by calls without explicit options.
    #[inline]
    #[must_use = "inspect the default options"]
    pub const fn default_read_options(&self) -> &LocalReadOptions {
        &self.defaults.read
    }

    /// Replaces the reader options inherited by subsequent calls.
    ///
    /// # Errors
    ///
    /// This setter is currently infallible; its `Result` keeps all
    /// configuration setters uniform and allows future validation.
    pub fn set_default_read_options(&mut self, options: LocalReadOptions) -> LocalResult<()> {
        self.defaults.read = options;
        Ok(())
    }

    /// Returns the writer options inherited by calls without explicit options.
    #[inline]
    #[must_use = "inspect the default options"]
    pub const fn default_write_options(&self) -> &LocalWriteOptions {
        &self.defaults.write
    }

    /// Replaces the writer options inherited by subsequent calls.
    ///
    /// # Errors
    ///
    /// Returns `RequirementNotMet` for statically unavailable guarantees.
    /// The previously installed options remain unchanged on error.
    pub fn set_default_write_options(&mut self, options: LocalWriteOptions) -> LocalResult<()> {
        validate_write_options(&options, self.capabilities(), LocalFileOperation::Configure)
            .map_err(|error| with_current_directory(error, self.current_directory.virtual_path()))?;
        self.defaults.write = options;
        Ok(())
    }

    /// Returns the listing options inherited by calls without explicit options.
    #[inline]
    #[must_use = "inspect the default options"]
    pub const fn default_list_options(&self) -> &LocalListOptions {
        &self.defaults.list
    }

    /// Replaces the default listing options after structural validation.
    ///
    /// # Errors
    ///
    /// Returns [`LocalFileError`] for a zero open-directory limit or a
    /// scope-incompatible symlink policy; existing defaults remain unchanged.
    pub fn set_default_list_options(&mut self, options: LocalListOptions) -> LocalResult<()> {
        validate_list_options(self.scope(), self.symlink_policy, &options, None)
            .map_err(|error| with_current_directory(error, self.current_directory.virtual_path()))?;
        self.defaults.list = options;
        Ok(())
    }

    /// Returns the copy options inherited by calls without explicit options.
    #[inline]
    #[must_use = "inspect the default options"]
    pub const fn default_copy_options(&self) -> &LocalCopyOptions {
        &self.defaults.copy
    }

    /// Replaces the default copy options after structural validation.
    ///
    /// # Errors
    ///
    /// Returns [`LocalFileError`] for a scope-incompatible symlink policy or
    /// an unrepresentable deadline; existing defaults remain unchanged.
    /// Source-dependent requirements and budget exhaustion are checked when
    /// a copy runs.
    pub fn set_default_copy_options(&mut self, options: LocalCopyOptions) -> LocalResult<()> {
        validate_copy_options(
            self.scope(),
            self.symlink_policy,
            &options,
            self.capabilities(),
            None,
            None,
        )
        .map_err(|error| with_current_directory(error, self.current_directory.virtual_path()))?;
        self.defaults.copy = options;
        Ok(())
    }

    /// Returns the directory-creation options inherited by defaulted calls.
    #[inline]
    #[must_use = "inspect the default options"]
    pub const fn default_create_directory_options(&self) -> &LocalCreateDirectoryOptions {
        &self.defaults.create_directory
    }

    /// Replaces the default directory-creation options.
    ///
    /// # Errors
    ///
    /// This setter is currently infallible; its `Result` keeps all
    /// configuration setters uniform and allows future validation.
    pub fn set_default_create_directory_options(&mut self, options: LocalCreateDirectoryOptions) -> LocalResult<()> {
        self.defaults.create_directory = options;
        Ok(())
    }

    /// Returns the deletion options inherited by calls without explicit
    /// options.
    #[inline]
    #[must_use = "inspect the default options"]
    pub const fn default_delete_options(&self) -> &LocalDeleteOptions {
        &self.defaults.delete
    }

    /// Replaces the default deletion options.
    ///
    /// # Errors
    ///
    /// This setter is currently infallible; its `Result` keeps all
    /// configuration setters uniform and allows future validation.
    pub fn set_default_delete_options(&mut self, options: LocalDeleteOptions) -> LocalResult<()> {
        self.defaults.delete = options;
        Ok(())
    }

    /// Returns the rename options inherited by calls without explicit options.
    #[inline]
    #[must_use = "inspect the default options"]
    pub const fn default_rename_options(&self) -> &LocalRenameOptions {
        &self.defaults.rename
    }

    /// Replaces the default rename options.
    ///
    /// # Errors
    ///
    /// Returns `RequirementNotMet` for statically unavailable guarantees.
    /// The previously installed options remain unchanged on error.
    pub fn set_default_rename_options(&mut self, options: LocalRenameOptions) -> LocalResult<()> {
        validate_rename_options(&options, self.capabilities(), LocalFileOperation::Configure)
            .map_err(|error| with_current_directory(error, self.current_directory.virtual_path()))?;
        self.defaults.rename = options;
        Ok(())
    }

    /// Returns the temporary-file options inherited by defaulted calls.
    #[inline]
    #[must_use = "inspect the default options"]
    pub const fn default_temp_file_options(&self) -> &LocalTempFileOptions {
        &self.defaults.temp_file
    }

    /// Replaces the default temporary-file options after validation.
    ///
    /// # Errors
    ///
    /// Returns [`LocalFileError`] for invalid affixes or a zero attempt budget;
    /// existing defaults remain unchanged.
    pub fn set_default_temp_file_options(&mut self, options: LocalTempFileOptions) -> LocalResult<()> {
        validate_temp_options(
            options.prefix(),
            options.suffix(),
            options.max_attempts(),
            LocalFileOperation::Configure,
        )
        .map_err(|error| with_current_directory(error, self.current_directory.virtual_path()))?;
        self.defaults.temp_file = options;
        Ok(())
    }

    /// Returns the temporary-directory options inherited by defaulted calls.
    #[inline]
    #[must_use = "inspect the default options"]
    pub const fn default_temp_directory_options(&self) -> &LocalTempDirectoryOptions {
        &self.defaults.temp_directory
    }

    /// Replaces the default temporary-directory options after validation.
    ///
    /// # Errors
    ///
    /// Returns [`LocalFileError`] for invalid affixes or a zero attempt budget;
    /// existing defaults remain unchanged.
    pub fn set_default_temp_directory_options(&mut self, options: LocalTempDirectoryOptions) -> LocalResult<()> {
        validate_temp_options(
            options.prefix(),
            options.suffix(),
            options.max_attempts(),
            LocalFileOperation::Configure,
        )
        .map_err(|error| with_current_directory(error, self.current_directory.virtual_path()))?;
        self.defaults.temp_directory = options;
        Ok(())
    }

    /// Installs an instance-local fault plan in test-support builds.
    #[cfg(feature = "test-support")]
    #[doc(hidden)]
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
