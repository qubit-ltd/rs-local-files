// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

// Rooted rename operations.
// qubit-style: allow source-test-pair

use super::LocalFileOperation;
use super::LocalRenameOptions;
use super::LocalRenameOutcome;
use super::LocalRenameResult;
use super::LocalSymlinkPolicy;
use super::Path;
use super::RootedLocalFileSystem;
use super::ensure_required_directory_durability;
use super::published_durability;
use super::rename_failure_after_native_attempt;
use super::rename_failure_renamed;
use super::rename_failure_unchanged;
use super::resolve_rooted_path;

impl RootedLocalFileSystem {
    /// Renames one rooted entry to another without leaving the authority.
    ///
    /// # Parameters
    ///
    /// - `source`: Validated relative source path.
    /// - `target`: Validated relative destination path.
    /// - `options`: Overwrite and guarantee policy.
    ///
    /// # Returns
    ///
    /// Achieved rename guarantees.
    ///
    /// # Errors
    ///
    /// Returns `LocalRenameFailure` for invalid descendants, conflicts,
    /// unsupported required durability, or native rename failures. The failure
    /// retains the strongest namespace state proven by rooted native I/O.
    pub fn rename(
        &self,
        source: &Path,
        target: &Path,
        options: &LocalRenameOptions,
        symlink_policy: LocalSymlinkPolicy,
    ) -> LocalRenameResult {
        ensure_required_directory_durability(
            options.durability(),
            LocalFileOperation::Rename,
            source,
            target,
            self.capabilities.supports_durable_rename(),
            "required directory durability is unavailable for this rooted authority",
        )
        .map_err(rename_failure_unchanged)?;
        let source_path = resolve_rooted_path(
            &self.root,
            source,
            symlink_policy,
            false,
            LocalFileOperation::Rename,
        )
        .map_err(rename_failure_unchanged)?;
        let target_path = resolve_rooted_path(
            &self.root,
            target,
            symlink_policy,
            false,
            LocalFileOperation::Rename,
        )
        .map_err(rename_failure_unchanged)?;
        let result = if options.overwrite() {
            self.root.rename(&source_path, &target_path)
        } else {
            self.root
                .rename_without_replacing(&source_path, &target_path)
        };
        result.map_err(|error| {
            rename_failure_after_native_attempt(source, target, error)
        })?;
        let durable = published_durability(
            options.durability(),
            || {
                self.root.sync_parent(&source_path)?;
                if source_path.as_path().parent()
                    != target_path.as_path().parent()
                {
                    self.root.sync_parent(&target_path)?;
                }
                Ok(())
            },
            LocalFileOperation::Rename,
            source,
            target,
        )
        .map_err(rename_failure_renamed)?;
        Ok(LocalRenameOutcome::new(true, durable))
    }
}
