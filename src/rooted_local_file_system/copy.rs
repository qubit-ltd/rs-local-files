// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

// Rooted copy operations.

use std::time::Instant;

use super::LocalCopyFailure;
use super::LocalCopyMethod;
use super::LocalCopyOptions;
use super::LocalCopyOutcome;
use super::LocalCopyResult;
use super::LocalCopyStats;
use super::LocalFileError;
use super::LocalFileErrorKind;
use super::LocalFileOperation;
use super::LocalSymlinkPolicy;
use super::Path;
use super::RootedLocalFileSystem;
use super::copy_failure_published;
use super::copy_failure_unchanged;
use super::ensure_required_directory_durability;
use super::io;
use super::published_durability;
use super::resolve_rooted_path;
use super::rooted_destination_is_directory;
use super::rooted_io_error;
use super::sync_rooted_copy_parent_chain;

impl RootedLocalFileSystem {
    /// Copies one rooted regular file, symbolic-link entry, or directory tree.
    ///
    /// # Parameters
    ///
    /// - `source`: Validated relative source path.
    /// - `target`: Validated relative destination path.
    /// - `options`: Unified copy policy.
    /// - `symlink_policy`: Inherited policy used without an explicit override.
    /// - `started_at`: Original operation start used for deadline accounting.
    ///
    /// # Returns
    ///
    /// Structured copy statistics and achieved atomicity and durability.
    ///
    /// # Errors
    ///
    /// Returns `LocalCopyFailure` for invalid descendants, forbidden link
    /// traversal, source-mode mismatches, conflicts, exhausted budgets,
    /// unsupported required guarantees, or native copy failures. Final links
    /// are copied as entries. Failures retain recorded partial publication.
    #[allow(clippy::result_large_err)]
    pub fn copy(
        &self,
        source: &Path,
        target: &Path,
        options: &LocalCopyOptions,
        symlink_policy: LocalSymlinkPolicy,
        started_at: Instant,
    ) -> LocalCopyResult {
        ensure_required_directory_durability(
            options.durability(),
            LocalFileOperation::Copy,
            source,
            target,
            self.capabilities.supports_durable_file_copy(),
            "required directory durability is unavailable for this rooted authority",
        )
        .map_err(copy_failure_unchanged)?;
        let symlink_policy = options.symlink_policy_override().unwrap_or(symlink_policy);
        let mut internal_options = crate::local::internal_copy_options(options, symlink_policy, started_at);
        let mut budget = crate::local::CopyBudget::new(internal_options);
        budget
            .check_deadline()
            .map_err(|error| copy_failure_unchanged(rooted_io_error(LocalFileOperation::Copy, source, error)))?;
        budget
            .charge_entry()
            .map_err(|error| copy_failure_unchanged(rooted_io_error(LocalFileOperation::Copy, source, error)))?;
        if let Some(max_entries) = internal_options.max_entries() {
            internal_options = internal_options.with_max_entries(max_entries - 1);
        }
        let source_path = resolve_rooted_path(&self.root, source, symlink_policy, false, LocalFileOperation::Copy)
            .map_err(copy_failure_unchanged)?;
        let target_path = resolve_rooted_path(&self.root, target, symlink_policy, false, LocalFileOperation::Copy)
            .map_err(copy_failure_unchanged)?;
        let metadata = self
            .root
            .symlink_metadata(&source_path)
            .map_err(|error| copy_failure_unchanged(rooted_io_error(LocalFileOperation::Copy, source, error)))?;
        let source_kind = super::rooted_metadata(metadata).kind();
        let directory = source_kind == crate::LocalFileKind::Directory;
        crate::local::validate_copy_source_kind(source_kind, options.source_mode()).map_err(|kind| {
            copy_failure_unchanged(
                LocalFileError::new(kind, LocalFileOperation::Copy)
                    .with_reason("copy source kind is unsupported or does not satisfy the selected source mode")
                    .with_path(source.to_path_buf())
                    .with_target(target.to_path_buf()),
            )
        })?;
        if crate::local::copy_source_guarantee_unavailable(source_kind, options.atomicity(), options.durability()) {
            return Err(copy_failure_unchanged(
                LocalFileError::new(LocalFileErrorKind::RequirementNotMet, LocalFileOperation::Copy)
                    .with_reason("required copy guarantees are unavailable for this rooted authority")
                    .with_path(source.to_path_buf())
                    .with_target(target.to_path_buf()),
            ));
        }
        let target_is_directory = rooted_destination_is_directory(&self.root, &target_path)
            .map_err(|error| copy_failure_unchanged(rooted_io_error(LocalFileOperation::Copy, target, error)))?;
        let target_exists = self
            .root
            .symlink_metadata(&target_path)
            .map(|_| true)
            .or_else(|error| (error.kind() == io::ErrorKind::NotFound).then_some(false).ok_or(error))
            .map_err(|error| copy_failure_unchanged(rooted_io_error(LocalFileOperation::Copy, target, error)))?;
        if options.type_conflict() == crate::LocalCopyTypeConflictPolicy::Skip
            && ((directory && !target_is_directory && target_exists) || (!directory && target_is_directory))
        {
            return Ok(LocalCopyOutcome::new(
                LocalCopyStats::skipped_one(),
                if directory {
                    LocalCopyMethod::Recursive
                } else {
                    LocalCopyMethod::StagedFile
                },
                false,
                false,
                crate::LocalMetadataPreservePolicy::None,
            ));
        }
        if crate::local::copy_file_replace_requires_atomicity(
            directory,
            options.atomicity(),
            options.type_conflict(),
            target_is_directory,
        ) {
            return Err(copy_failure_unchanged(
                LocalFileError::new(LocalFileErrorKind::RequirementNotMet, LocalFileOperation::Copy)
                    .with_reason("required atomic replacement is unavailable for this copy")
                    .with_path(source.to_path_buf())
                    .with_target(target.to_path_buf()),
            ));
        }
        if options.creates_parent()
            && let Some(parent) = target_path
                .as_path()
                .parent()
                .filter(|parent| !parent.as_os_str().is_empty())
        {
            let parent =
                crate::local::LocalRelativePath::new(parent).expect("parent of a validated rooted path is valid");
            self.root
                .create_dir_all(&parent)
                .map_err(|error| copy_failure_unchanged(rooted_io_error(LocalFileOperation::Copy, target, error)))?;
        }
        let stats = self
            .root
            .copy_with_durability(&source_path, &target_path, internal_options, options.durability())
            .map_err(|error| copy_failure_from_dir_error(source, target, error, directory))?;
        if stats.was_fully_skipped() {
            return Ok(LocalCopyOutcome::new(
                LocalCopyStats::from_internal(stats),
                if directory {
                    LocalCopyMethod::Recursive
                } else {
                    LocalCopyMethod::StagedFile
                },
                false,
                false,
                crate::LocalMetadataPreservePolicy::None,
            ));
        }
        let parent_durable = published_durability(
            options.durability(),
            || {
                self.root.sync_parent(&target_path)?;
                if options.creates_parent() {
                    sync_rooted_copy_parent_chain(&self.root, &target_path)?;
                }
                Ok(())
            },
            LocalFileOperation::Copy,
            source,
            target,
        )
        .map_err(|error| copy_failure_published(error, LocalCopyStats::from_internal(stats)))?;
        let content_durable = source_kind == crate::LocalFileKind::Symlink || stats.files_durable();
        let durable = !directory && content_durable && parent_durable;
        Ok(LocalCopyOutcome::new(
            LocalCopyStats::from_internal(stats),
            if directory {
                LocalCopyMethod::Recursive
            } else {
                LocalCopyMethod::StagedFile
            },
            !directory && stats.atomic_publication(),
            durable,
            if stats.files > 0 || stats.directories > 0 || stats.overwritten > 0 {
                options.preserve_metadata()
            } else {
                crate::LocalMetadataPreservePolicy::None
            },
        ))
    }
}

fn copy_failure_from_dir_error(
    source: &Path,
    target: &Path,
    error: crate::local::LocalCopyDirError,
    directory: bool,
) -> LocalCopyFailure {
    let installed_file = error.failure_state() == Some(crate::LocalCopyFailureState::Published);
    let failure = LocalCopyFailure::from_copy_dir_error(source, target, error);
    if directory && installed_file {
        failure.with_state(crate::LocalCopyFailureState::PartiallyPublished)
    } else {
        failure
    }
}

#[cfg(test)]
mod tests {
    use std::io;
    use std::path::Path;
    use std::path::PathBuf;

    use super::copy_failure_from_dir_error;

    #[test]
    fn rooted_directory_request_downgrades_installed_child_proof() {
        let error = || {
            crate::local::LocalCopyDirError::new(
                crate::LocalCopyDirStage::CommitFile,
                PathBuf::from("/source/first"),
                PathBuf::from("/target/first"),
                crate::LocalCopyDirStats {
                    files: 1,
                    bytes: 7,
                    ..crate::LocalCopyDirStats::default()
                },
                io::Error::from(io::ErrorKind::Other),
            )
            .with_failure_state(crate::LocalCopyFailureState::Published)
        };
        let source = Path::new("source");
        let target = Path::new("target");

        let directory_failure = copy_failure_from_dir_error(source, target, error(), true);
        assert_eq!(
            crate::LocalCopyFailureState::PartiallyPublished,
            directory_failure.state()
        );
        assert_eq!(1, directory_failure.partial_stats().files());
        assert_eq!(7, directory_failure.partial_stats().bytes());
        assert_eq!(Some(Path::new("/source/first")), directory_failure.failed_source_path());
        assert_eq!(Some(Path::new("/target/first")), directory_failure.failed_target_path());

        let file_failure = copy_failure_from_dir_error(source, target, error(), false);
        assert_eq!(crate::LocalCopyFailureState::Published, file_failure.state());
    }
}
