// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

// Host rename operations.
// qubit-style: allow source-test-pair

use super::HostLocalFileSystem;
use super::LocalFileError;
use super::LocalFileOperation;
use super::LocalRenameFailure;
use super::LocalRenameFailureState;
use super::LocalRenameOptions;
use super::LocalRenameOutcome;
use super::LocalRenameResult;
use super::LocalSymlinkPolicy;
use super::Path;
use super::bind_host_paths;
use super::ensure_required_directory_durability;
use super::fs;
use super::io;
use super::published_durability;
use super::rename_failure_after_native_attempt;
use super::rename_failure_renamed;
use super::rename_failure_unchanged;
use super::resolve_host_path;
use super::test_io_fault;
use crate::local::internal::sync_parent_dir;

impl HostLocalFileSystem {
    /// Renames a Host entry with explicit overwrite, guarantee, and
    /// symbolic-link policies.
    ///
    /// Both paths are bound using one current-directory snapshot.
    ///
    /// # Parameters
    ///
    /// - `source`: Existing source entry.
    /// - `target`: Destination entry.
    /// - `options`: Overwrite and durability requirements.
    /// - `symlink_policy`: Policy for intermediate symbolic links.
    ///
    /// # Returns
    ///
    /// Guarantees actually achieved by the rename.
    ///
    /// # Errors
    ///
    /// Returns `LocalRenameFailure` when source inspection, publication, or a
    /// required guarantee fails.
    pub fn rename_with_policy(
        source: &Path,
        target: &Path,
        options: &LocalRenameOptions,
        symlink_policy: LocalSymlinkPolicy,
    ) -> LocalRenameResult {
        let [source, target] = bind_host_paths([source, target]).map_err(rename_failure_unchanged)?;
        let source = resolve_host_path(&source, symlink_policy, false).map_err(rename_failure_unchanged)?;
        let target = resolve_host_path(&target, symlink_policy, false).map_err(rename_failure_unchanged)?;
        let implements_durability = Self::capabilities().supports_durable_rename();
        let implements_durability =
            implements_durability && !crate::local::test_support_enabled("local-fs-required-directory-durability");
        ensure_required_directory_durability(
            options.durability(),
            LocalFileOperation::Rename,
            &source,
            &target,
            implements_durability,
            "required directory durability is unavailable on this host",
        )
        .map_err(rename_failure_unchanged)?;
        let source_metadata = test_io_fault("local-fs-rename-source-metadata")
            .map_or_else(|| fs::symlink_metadata(&source), Err)
            .map_err(|error| rename_failure_unchanged(rename_io_error(&source, &target, error)))?;
        if crate::local::test_support_enabled("rename-native-indeterminate") {
            return Err(rename_failure_indeterminate(rename_io_error(
                &source,
                &target,
                crate::local::test_fault_error(),
            )));
        }
        let result = if let Some(error) = test_io_fault("local-fs-rename-native-error") {
            Err(error)
        } else {
            if options.overwrite() {
                if source_metadata.file_type().is_dir() {
                    fs::rename(&source, &target)
                } else {
                    crate::local::replace_file(&source, &target)
                }
            } else if source_metadata.file_type().is_dir() {
                crate::local::move_directory_without_replacing(&source, &target)
            } else {
                crate::local::move_file_without_replacing(&source, &target)
            }
        };
        result.map_err(|error| rename_failure_after_native_attempt(&source, &target, error))?;

        let durable = published_durability(
            options.durability(),
            || sync_rename_parents(&source, &target),
            LocalFileOperation::Rename,
            &source,
            &target,
        )
        .map_err(rename_failure_renamed)?;
        // The native rename path above either publishes atomically or fails
        // before publication; no fallback copy-and-delete path is used.
        Ok(LocalRenameOutcome::new(true, durable))
    }
}

/// Adds both rename paths to a native I/O failure.
///
/// # Parameters
///
/// - `source`: Bound source path.
/// - `target`: Bound destination path.
/// - `error`: Native rename failure.
///
/// # Returns
///
/// Structured rename error.
#[inline]
fn rename_io_error(source: &Path, target: &Path, error: io::Error) -> LocalFileError {
    LocalFileError::from_io(
        LocalFileOperation::Rename,
        Some(source.to_path_buf()),
        Some(target.to_path_buf()),
        error,
    )
}

/// Wraps a failure whose native rename effect cannot be proven.
#[inline(always)]
fn rename_failure_indeterminate(error: LocalFileError) -> LocalRenameFailure {
    LocalRenameFailure::new(error, LocalRenameFailureState::Indeterminate)
}

/// Synchronizes the destination parent directory where supported.
///
/// # Parameters
///
/// - `target`: Bound destination path.
///
/// # Errors
///
/// Returns native I/O errors from opening or synchronizing the parent.
pub(crate) fn sync_parent_directory(path: &Path) -> io::Result<()> {
    if crate::local::test_support_enabled("copy-parent-sync")
        || crate::local::test_support_enabled("rename-parent-sync")
    {
        return Err(crate::local::test_io_error("copy-parent-sync")
            .or_else(|| crate::local::test_io_error("rename-parent-sync"))
            .expect("selected parent-sync fault should provide an I/O error"));
    }
    sync_parent_dir(path)
}

/// Synchronizes every parent directory changed by a completed rename.
fn sync_rename_parents(source: &Path, target: &Path) -> io::Result<()> {
    sync_parent_directory(source)?;
    if source.parent() != target.parent() {
        sync_parent_directory(target)?;
    }
    Ok(())
}

/// Reports whether the final destination entry is a real directory.
pub(crate) fn destination_is_directory(path: &Path) -> io::Result<bool> {
    match fs::symlink_metadata(path) {
        Ok(metadata) => Ok(metadata.file_type().is_dir()),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error),
    }
}
