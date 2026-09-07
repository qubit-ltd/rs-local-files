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
use super::Instant;
use super::LocalCopyOptions;
use super::LocalCopyResult;
use super::LocalFileError;
use super::LocalFileErrorKind;
use super::LocalFileOperation;
use super::LocalFileSystem;
use super::LocalFileSystemScope;
use super::LocalNamespace;
use super::LocalRenameOptions;
use super::LocalRenameResult;
use super::Path;
use super::copy_failure_unchanged;
use super::rename_failure_unchanged;
use super::resolve_operation_path;
use super::validate_copy_options;
use super::with_current_directory;

impl LocalFileSystem {
    /// Copies an entry using this instance's default copy options.
    ///
    /// Returns the outcome or structured failure described by
    /// [`Self::copy_with_options`].
    pub fn copy(&self, source: &Path, destination: &Path) -> LocalCopyResult {
        self.copy_with_options(source, destination, &self.defaults.copy)
    }

    /// Copies an entry using one complete explicit options value.
    ///
    /// Resolves both paths from one current-directory snapshot. `Entry`
    /// accepts a regular file or final symbolic link; `Tree` requires a real
    /// directory; `Auto` selects from the observed kind. Final links are
    /// copied as entries. A directory-qualified path requires tree mode.
    ///
    /// # Errors
    ///
    /// Returns a structured failure for invalid paths/options, unsupported
    /// source kinds, unmet requirements, exhausted budgets, or native I/O
    /// failures. Source-mode mismatch is rejected before destination mutation.
    /// Tree copies can fail after publishing descendants; inspect the failure
    /// state, partial statistics, and cleanup error before retrying.
    #[allow(clippy::result_large_err)]
    pub fn copy_with_options(&self, source: &Path, destination: &Path, options: &LocalCopyOptions) -> LocalCopyResult {
        let started_at = Instant::now();
        let request_source = source;
        let request_destination = destination;
        validate_copy_options(
            self.scope(),
            self.symlink_policy,
            options,
            self.capabilities(),
            Some(request_source),
            Some(request_destination),
        )
        .map_err(|error| {
            copy_failure_unchanged(with_current_directory(
                error
                    .with_path(request_source.to_path_buf())
                    .with_target(request_destination.to_path_buf()),
                self.current_directory.virtual_path(),
            ))
        })?;
        let resolver = self
            .resolver_for_pair(request_source, request_destination, LocalFileOperation::Copy)
            .map_err(|error| copy_failure_unchanged(error.with_target(request_destination.to_path_buf())))?;
        let source = resolve_operation_path(&resolver, request_source, LocalFileOperation::Copy)
            .map_err(|error| copy_failure_unchanged(error.with_target(request_destination.to_path_buf())))?;
        let destination =
            resolve_operation_path(&resolver, request_destination, LocalFileOperation::Copy).map_err(|error| {
                copy_failure_unchanged(
                    error
                        .with_path(request_source.to_path_buf())
                        .with_target(request_destination.to_path_buf()),
                )
            })?;
        self.reject_root_operand(&source, LocalFileOperation::Copy, resolver.current_directory())
            .map_err(|error| {
                copy_failure_unchanged(error.with_target(destination.namespace_absolute().to_path_buf()))
            })?;
        self.reject_root_operand(&destination, LocalFileOperation::Copy, resolver.current_directory())
            .map_err(|error| {
                copy_failure_unchanged(
                    error
                        .with_path(source.namespace_absolute().to_path_buf())
                        .with_target(destination.namespace_absolute().to_path_buf()),
                )
            })?;
        let directory_qualified = source.directory_required() || destination.directory_required();
        let directory_options;
        let options = if directory_qualified {
            match options.source_mode() {
                crate::LocalCopySourceMode::Auto => {
                    directory_options = (*options).with_tree_source();
                    &directory_options
                }
                crate::LocalCopySourceMode::Tree => options,
                crate::LocalCopySourceMode::Entry => {
                    let error = LocalFileError::new(LocalFileErrorKind::NotDirectory, LocalFileOperation::Copy)
                        .with_reason("directory-qualified copy paths are incompatible with entry source mode")
                        .with_path(source.namespace_absolute().to_path_buf())
                        .with_target(destination.namespace_absolute().to_path_buf());
                    return Err(copy_failure_unchanged(with_current_directory(
                        error,
                        resolver.current_directory(),
                    )));
                }
            }
        } else {
            options
        };
        let result = match &self.core.namespace {
            LocalNamespace::Host => HostLocalFileSystem::copy_with_policy(
                source.authority_relative(),
                destination.authority_relative(),
                options,
                self.symlink_policy,
                started_at,
            ),
            LocalNamespace::Rooted(rooted) => rooted.copy(
                source.authority_relative(),
                destination.authority_relative(),
                options,
                self.symlink_policy,
                started_at,
            ),
        };
        result.map_err(|failure| {
            failure.remap_namespace(
                source.namespace_absolute(),
                destination.namespace_absolute(),
                source.authority_relative(),
                destination.authority_relative(),
                self.scope() == LocalFileSystemScope::Rooted,
                resolver.current_directory(),
            )
        })
    }

    /// Renames an entry using this instance's default rename options.
    ///
    /// Returns the outcome or structured failure described by
    /// [`Self::rename_with_options`].
    pub fn rename(&self, source: &Path, destination: &Path) -> LocalRenameResult {
        self.rename_with_options(source, destination, &self.defaults.rename)
    }

    /// Renames an entry using one complete explicit options value.
    ///
    /// Resolves both paths from one current-directory snapshot and preserves
    /// final symbolic links as entries. The outcome reports the achieved
    /// atomicity and durability.
    ///
    /// # Errors
    ///
    /// Returns path, type, conflict, requirement, or native rename failures.
    /// A post-publication synchronization failure can occur after the entry
    /// moved; inspect the returned rename state before retrying.
    #[allow(clippy::result_large_err)]
    pub fn rename_with_options(
        &self,
        source: &Path,
        destination: &Path,
        options: &LocalRenameOptions,
    ) -> LocalRenameResult {
        crate::local_file_system_validation::validate_rename_options(
            options,
            self.capabilities(),
            LocalFileOperation::Rename,
        )
        .map_err(|error| {
            rename_failure_unchanged(with_current_directory(
                error
                    .with_path(source.to_path_buf())
                    .with_target(destination.to_path_buf()),
                self.current_directory.virtual_path(),
            ))
        })?;
        let request_source = source;
        let request_destination = destination;
        let resolver = self
            .resolver_for_pair(request_source, request_destination, LocalFileOperation::Rename)
            .map_err(|error| rename_failure_unchanged(error.with_target(request_destination.to_path_buf())))?;
        let source = resolve_operation_path(&resolver, request_source, LocalFileOperation::Rename)
            .map_err(|error| rename_failure_unchanged(error.with_target(request_destination.to_path_buf())))?;
        let destination =
            resolve_operation_path(&resolver, request_destination, LocalFileOperation::Rename).map_err(|error| {
                rename_failure_unchanged(
                    error
                        .with_path(request_source.to_path_buf())
                        .with_target(request_destination.to_path_buf()),
                )
            })?;
        self.reject_root_operand(&source, LocalFileOperation::Rename, resolver.current_directory())
            .map_err(|error| {
                rename_failure_unchanged(error.with_target(destination.namespace_absolute().to_path_buf()))
            })?;
        self.reject_root_operand(&destination, LocalFileOperation::Rename, resolver.current_directory())
            .map_err(|error| {
                rename_failure_unchanged(
                    error
                        .with_path(source.namespace_absolute().to_path_buf())
                        .with_target(destination.namespace_absolute().to_path_buf()),
                )
            })?;
        self.validate_directory_requirement(&source, LocalFileOperation::Rename, resolver.current_directory())
            .map_err(|error| {
                rename_failure_unchanged(error.with_target(destination.namespace_absolute().to_path_buf()))
            })?;
        self.validate_directory_requirement(&destination, LocalFileOperation::Rename, resolver.current_directory())
            .map_err(|error| {
                rename_failure_unchanged(
                    error
                        .with_path(source.namespace_absolute().to_path_buf())
                        .with_target(destination.namespace_absolute().to_path_buf()),
                )
            })?;
        let result = match &self.core.namespace {
            LocalNamespace::Host => HostLocalFileSystem::rename_with_policy(
                source.authority_relative(),
                destination.authority_relative(),
                options,
                self.symlink_policy,
            ),
            LocalNamespace::Rooted(rooted) => rooted.rename(
                source.authority_relative(),
                destination.authority_relative(),
                options,
                self.symlink_policy,
            ),
        };
        result.map_err(|failure| {
            failure.remap_namespace(
                source.namespace_absolute(),
                destination.namespace_absolute(),
                resolver.current_directory(),
            )
        })
    }
}
