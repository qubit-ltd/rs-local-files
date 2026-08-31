// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

// Host copy operations.
// qubit-style: allow source-test-pair

use std::time::Instant;

use super::HostLocalFileSystem;
use super::LocalCopyFailure;
use super::LocalCopyMethod;
use super::LocalCopyOptions;
use super::LocalCopyOutcome;
use super::LocalCopyResult;
use super::LocalCopyStats;
use super::LocalFileError;
use super::LocalFileErrorKind;
use super::LocalFileOperation;
use super::LocalResult;
use super::LocalSymlinkPolicy;
use super::Path;
use super::PathBuf;
use super::bind_host_paths;
use super::copy_failure_published;
use super::copy_failure_unchanged;
use super::destination_is_directory;
use super::ensure_required_directory_durability;
use super::fs;
use super::io;
use super::published_durability;
use super::resolve_host_path;
use super::sync_parent_directory;
use super::test_io_fault;
use crate::local::internal::internal_copy_options;

impl HostLocalFileSystem {
    /// Copies through a Host namespace using an explicit symbolic-link policy.
    ///
    /// # Parameters
    ///
    /// - `source`: Native source entry.
    /// - `target`: Native destination entry.
    /// - `options`: Copy conflict, metadata, and guarantee policy.
    /// - `symlink_policy`: Policy for symbolic links encountered in a tree.
    ///
    /// # Returns
    ///
    /// Structured copy statistics and achieved guarantees.
    ///
    /// # Errors
    ///
    /// Returns `LocalCopyFailure` when source inspection, copying, or required
    /// guarantees fail.
    #[allow(clippy::result_large_err)]
    pub fn copy_with_policy(
        source: &Path,
        target: &Path,
        options: &LocalCopyOptions,
        symlink_policy: LocalSymlinkPolicy,
        started_at: Instant,
    ) -> LocalCopyResult {
        Self::copy_with_policy_scoped(source, target, options, symlink_policy, started_at, None)
    }

    /// Copies through a Host namespace while constraining followed directory
    /// links to an optional canonical scope root.
    ///
    /// # Parameters
    ///
    /// - `source`: Native source entry.
    /// - `target`: Native destination entry.
    /// - `options`: Copy conflict, metadata, and guarantee policy.
    /// - `symlink_policy`: Policy for symbolic links encountered in a tree.
    /// - `scope_root`: Optional canonical root that followed links must stay
    ///   beneath.
    ///
    /// # Returns
    ///
    /// Structured copy statistics and achieved guarantees.
    ///
    /// # Errors
    ///
    /// Returns `LocalCopyFailure` when source inspection, copying, or required
    /// guarantees fail.
    #[allow(clippy::result_large_err)]
    pub fn copy_with_policy_scoped(
        source: &Path,
        target: &Path,
        options: &LocalCopyOptions,
        symlink_policy: LocalSymlinkPolicy,
        started_at: Instant,
        scope_root: Option<&Path>,
    ) -> LocalCopyResult {
        let symlink_policy = options.symlink_policy_override().unwrap_or(symlink_policy);
        let [source, target] = bind_host_paths([source, target]).map_err(copy_failure_unchanged)?;
        let source = resolve_host_path(&source, symlink_policy, false).map_err(copy_failure_unchanged)?;
        let target = resolve_host_path(&target, symlink_policy, false).map_err(copy_failure_unchanged)?;
        let mut internal_options = internal_copy_options(options, symlink_policy, started_at);
        let mut budget = crate::local::CopyBudget::new(internal_options);
        if let Err(error) = budget.check_deadline() {
            return Err(copy_failure_unchanged(copy_io_error(&source, &target, error)));
        }
        if let Err(error) = budget.charge_entry() {
            return Err(copy_failure_unchanged(copy_io_error(&source, &target, error)));
        }
        if let Some(max_entries) = internal_options.max_entries() {
            internal_options = internal_options.with_max_entries(max_entries - 1);
        }
        let implements_durability = Self::capabilities().supports_durable_file_copy();
        let implements_durability =
            implements_durability && !crate::local::test_support_enabled("local-fs-required-directory-durability");
        ensure_required_directory_durability(
            options.durability(),
            LocalFileOperation::Copy,
            &source,
            &target,
            implements_durability,
            "required directory durability is unavailable on this host",
        )
        .map_err(copy_failure_unchanged)?;
        let source_metadata = match test_io_fault("local-fs-copy-source-metadata") {
            Some(error) => Err(error),
            None => fs::symlink_metadata(&source),
        };
        let source_metadata = match source_metadata {
            Ok(metadata) => metadata,
            Err(error) => {
                return Err(copy_failure_unchanged(copy_io_error(&source, &target, error)));
            }
        };
        if source_metadata.file_type().is_symlink() {
            if options.source_mode() == crate::LocalCopySourceMode::Tree {
                return Err(copy_failure_unchanged(
                    LocalFileError::new(LocalFileErrorKind::RequirementNotMet, LocalFileOperation::Copy)
                        .with_reason("a symbolic-link entry is not a directory tree source")
                        .with_path(source)
                        .with_target(target),
                ));
            }
            return copy_symlink_entry(&source, &target, options, &mut budget);
        }
        let effective_metadata = &source_metadata;

        reject_copy_alias(&source, &target, effective_metadata).map_err(copy_failure_unchanged)?;

        let source_is_directory = effective_metadata.file_type().is_dir();
        if source_is_directory {
            if crate::local::copy_source_mode_mismatch(source_is_directory, options.source_mode()) {
                return Err(copy_failure_unchanged(
                    LocalFileError::new(LocalFileErrorKind::RequirementNotMet, LocalFileOperation::Copy)
                        .with_reason("copy source is a directory but file mode was required")
                        .with_path(source)
                        .with_target(target),
                ));
            }
            if crate::local::copy_directory_guarantee_unavailable(
                source_is_directory,
                options.atomicity(),
                options.durability(),
            ) {
                return Err(copy_failure_unchanged(
                    LocalFileError::new(LocalFileErrorKind::RequirementNotMet, LocalFileOperation::Copy)
                        .with_reason("required directory copy guarantees are unavailable on this host")
                        .with_path(source)
                        .with_target(target),
                ));
            }
            if let Err(error) = prepare_copy_parent(&target, options) {
                return Err(copy_failure_unchanged(copy_io_error(&source, &target, error)));
            }
            let stats = match scope_root {
                Some(scope_root) => {
                    crate::local::copy_dir_all_with_paths_scoped(&source, &target, internal_options, scope_root)
                }
                None => crate::local::copy_dir_all_with_paths(&source, &target, internal_options),
            };
            let stats = match stats {
                Ok(stats) => stats,
                Err(error) => return Err(copy_pipeline_failure(&source, &target, error)),
            };
            return Ok(LocalCopyOutcome::new(
                LocalCopyStats::from_internal(stats),
                LocalCopyMethod::Recursive,
                false,
                false,
                options.preserve_metadata(),
            ));
        }
        if !effective_metadata.file_type().is_file() {
            return Err(copy_failure_unchanged(
                LocalFileError::new(LocalFileErrorKind::TypeConflict, LocalFileOperation::Copy)
                    .with_path(source)
                    .with_target(target),
            ));
        }
        if crate::local::copy_source_mode_mismatch(source_is_directory, options.source_mode()) {
            return Err(copy_failure_unchanged(
                LocalFileError::new(LocalFileErrorKind::RequirementNotMet, LocalFileOperation::Copy)
                    .with_reason("copy source is a file but directory mode was required")
                    .with_path(source)
                    .with_target(target),
            ));
        }
        let target_is_directory = match destination_is_directory(&target) {
            Ok(target_is_directory) => target_is_directory,
            Err(error) => {
                return Err(copy_failure_unchanged(copy_io_error(&source, &target, error)));
            }
        };
        if crate::local::copy_file_replace_requires_atomicity(
            source_is_directory,
            options.atomicity(),
            options.type_conflict(),
            target_is_directory,
        ) {
            return Err(copy_failure_unchanged(
                LocalFileError::new(LocalFileErrorKind::RequirementNotMet, LocalFileOperation::Copy)
                    .with_reason("required atomic replacement is unavailable for this copy")
                    .with_path(source)
                    .with_target(target),
            ));
        }

        let parent_dirs_to_sync = match prepare_copy_parent(&target, options) {
            Ok(paths) => paths,
            Err(error) => {
                return Err(copy_failure_unchanged(copy_io_error(&source, &target, error)));
            }
        };

        let mut stats = crate::local::LocalCopyDirStats::default();
        if let Err(error) =
            crate::local::copy_file_with_options(&source, &target, internal_options, &mut stats, &mut budget)
        {
            return Err(copy_pipeline_failure(&source, &target, error));
        }
        let parent_durable = published_durability(
            options.durability(),
            || sync_parent_directory(&target).and_then(|()| sync_created_parent_directories(&parent_dirs_to_sync)),
            LocalFileOperation::Copy,
            &source,
            &target,
        )
        .map_err(|error| copy_failure_published(error, LocalCopyStats::from_internal(stats)))?;
        let durable = stats.files_durable() && parent_durable;
        Ok(LocalCopyOutcome::new(
            LocalCopyStats::from_internal(stats),
            LocalCopyMethod::StagedFile,
            stats.atomic_publication(),
            durable,
            options.preserve_metadata(),
        ))
    }
}

/// Creates missing copy target parents and returns directories requiring sync.
#[inline]
fn prepare_copy_parent(target: &Path, options: &LocalCopyOptions) -> io::Result<Vec<PathBuf>> {
    if options.creates_parent() {
        crate::local::ensure_parent_path_with_sync_dirs(target)
    } else {
        Ok(Vec::new())
    }
}

/// Copies a final symbolic-link entry without dereferencing it.
#[allow(clippy::result_large_err)]
fn copy_symlink_entry(
    source: &Path,
    target: &Path,
    options: &LocalCopyOptions,
    budget: &mut crate::local::CopyBudget,
) -> LocalCopyResult {
    let existing = match fs::symlink_metadata(target) {
        Ok(metadata) => Some(metadata),
        Err(error) if error.kind() == io::ErrorKind::NotFound => None,
        Err(error) => {
            return Err(copy_failure_unchanged(copy_io_error(source, target, error)));
        }
    };
    if existing.is_some() {
        if options.conflict() == crate::LocalCopyConflictPolicy::Skip {
            return Ok(LocalCopyOutcome::new(
                LocalCopyStats::skipped_one(),
                LocalCopyMethod::StagedFile,
                false,
                false,
                options.preserve_metadata(),
            ));
        }
        if options.conflict() == crate::LocalCopyConflictPolicy::Fail {
            return Err(copy_failure_unchanged(copy_io_error(
                source,
                target,
                io::Error::from(io::ErrorKind::AlreadyExists),
            )));
        }
        let existing_is_directory = match existing.as_ref() {
            Some(metadata) => metadata.file_type().is_dir(),
            None => false,
        };
        if existing_is_directory && options.type_conflict() == crate::LocalCopyTypeConflictPolicy::Fail {
            return Err(copy_failure_unchanged(copy_io_error(
                source,
                target,
                io::Error::from(io::ErrorKind::AlreadyExists),
            )));
        }
        let remove_result = if existing_is_directory {
            fs::remove_dir_all(target)
        } else {
            fs::remove_file(target)
        };
        if let Err(error) = remove_result {
            return Err(copy_failure_unchanged(copy_io_error(source, target, error)));
        }
    }
    if let Err(error) = prepare_copy_parent(target, options) {
        return Err(copy_failure_unchanged(copy_io_error(source, target, error)));
    }
    let link_target = match fs::read_link(source) {
        Ok(target) => target,
        Err(error) => {
            return Err(copy_failure_unchanged(copy_io_error(source, target, error)));
        }
    };
    if let Err(error) = budget.check_deadline() {
        return Err(copy_failure_unchanged(copy_io_error(source, target, error)));
    }
    if let Err(error) = create_symlink_entry(&link_target, source, target) {
        return Err(copy_failure_unchanged(copy_io_error(source, target, error)));
    }
    let stats = crate::local::LocalCopyDirStats {
        files: 1,
        overwritten: u64::from(existing.is_some()),
        files_durable: false,
        ..Default::default()
    };
    let public_stats = LocalCopyStats::from_internal(stats);
    let durable = match options.durability() {
        crate::LocalDurabilityRequirement::NotRequired => false,
        crate::LocalDurabilityRequirement::Preferred => sync_parent_directory(target).is_ok(),
        crate::LocalDurabilityRequirement::Required => {
            if let Err(error) = sync_parent_directory(target) {
                return Err(copy_failure_published(
                    copy_io_error(source, target, error),
                    public_stats,
                ));
            }
            true
        }
    };
    Ok(LocalCopyOutcome::new(
        public_stats,
        LocalCopyMethod::StagedFile,
        false,
        durable,
        options.preserve_metadata(),
    ))
}

/// Creates a symbolic link with the platform-specific link-kind API.
fn create_symlink_entry(link_target: &Path, _source: &Path, target: &Path) -> io::Result<()> {
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(link_target, target)
    }
    #[cfg(windows)]
    {
        if fs::metadata(_source).is_ok_and(|metadata| metadata.is_dir()) {
            std::os::windows::fs::symlink_dir(link_target, target)
        } else {
            std::os::windows::fs::symlink_file(link_target, target)
        }
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = (link_target, _source, target);
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "symbolic links are unsupported on this platform",
        ))
    }
}

/// Synchronizes newly created copy target parents from deepest to shallowest.
fn sync_created_parent_directories(paths: &[PathBuf]) -> io::Result<()> {
    #[cfg(unix)]
    {
        for path in paths.iter().rev() {
            sync_parent_directory(path)?;
        }
        Ok(())
    }
    #[cfg(not(unix))]
    {
        let _ = paths;
        Ok(())
    }
}

/// Rejects textual self-copy and native hard-link aliases.
///
/// # Parameters
///
/// - `source`: Bound source path.
/// - `target`: Bound destination path.
/// - `source_metadata`: Final-entry source metadata.
///
/// # Errors
///
/// Returns `LocalFileError` when both paths identify the same entry.
fn reject_copy_alias(source: &Path, target: &Path, source_metadata: &fs::Metadata) -> LocalResult<()> {
    if source == target {
        return Err(copy_alias_error(source, target));
    }
    let target_metadata =
        match test_io_fault("local-fs-copy-target-metadata").map_or_else(|| fs::symlink_metadata(target), Err) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == io::ErrorKind::NotFound => {
                return Ok(());
            }
            Err(error) => return Err(copy_io_error(source, target, error)),
        };
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;

        if source_metadata.dev() == target_metadata.dev() && source_metadata.ino() == target_metadata.ino() {
            return Err(copy_alias_error(source, target));
        }
    }
    #[cfg(windows)]
    {
        if source_metadata.file_type().is_dir() || target_metadata.file_type().is_dir() {
            return Ok(());
        }
        if !source_metadata.file_type().is_symlink()
            && !target_metadata.file_type().is_symlink()
            && windows_file_identity(source).map_err(|error| copy_io_error(source, target, error))?
                == windows_file_identity(target).map_err(|error| copy_io_error(source, target, error))?
        {
            return Err(copy_alias_error(source, target));
        }
    }
    Ok(())
}

/// Returns the stable Windows identity for a final filesystem entry.
///
/// # Parameters
///
/// - `path`: Entry whose identity is required.
///
/// # Returns
///
/// The volume serial number and file index reported by the opened handle.
///
/// # Errors
///
/// Returns an I/O error when the entry cannot be opened or Windows cannot
/// inspect its handle.
#[cfg(windows)]
fn windows_file_identity(path: &Path) -> io::Result<(u32, u64)> {
    use std::os::windows::fs::OpenOptionsExt;
    use std::os::windows::io::AsRawHandle;

    use windows_sys::Win32::Storage::FileSystem::BY_HANDLE_FILE_INFORMATION;
    use windows_sys::Win32::Storage::FileSystem::FILE_FLAG_BACKUP_SEMANTICS;
    use windows_sys::Win32::Storage::FileSystem::GetFileInformationByHandle;

    let file = fs::OpenOptions::new()
        .read(true)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS)
        .open(path)?;
    let mut information = BY_HANDLE_FILE_INFORMATION::default();
    // SAFETY: `file` owns a live handle and `information` is a correctly sized
    // writable buffer for `GetFileInformationByHandle`.
    if unsafe { GetFileInformationByHandle(file.as_raw_handle(), &raw mut information) } == 0 {
        return Err(io::Error::last_os_error());
    }
    let file_index = (u64::from(information.nFileIndexHigh) << 32) | u64::from(information.nFileIndexLow);
    Ok((information.dwVolumeSerialNumber, file_index))
}

/// Creates a structured source/target alias error.
///
/// # Parameters
///
/// - `source`: Bound source path.
/// - `target`: Bound destination path.
///
/// # Returns
///
/// Invalid-options copy error.
#[must_use]
#[inline]
fn copy_alias_error(source: &Path, target: &Path) -> LocalFileError {
    LocalFileError::new(LocalFileErrorKind::InvalidOptions, LocalFileOperation::Copy)
        .with_path(source.to_path_buf())
        .with_target(target.to_path_buf())
}

/// Converts a pipeline failure into a lossless public copy failure.
#[inline(always)]
fn copy_pipeline_failure(source: &Path, target: &Path, error: crate::local::LocalCopyDirError) -> LocalCopyFailure {
    LocalCopyFailure::from_copy_dir_error(source, target, error)
}

/// Adds both copy paths to a native I/O failure.
///
/// # Parameters
///
/// - `source`: Bound source path.
/// - `target`: Bound destination path.
/// - `error`: Native I/O failure.
///
/// # Returns
///
/// Structured copy error.
#[inline]
fn copy_io_error(source: &Path, target: &Path, error: io::Error) -> LocalFileError {
    LocalFileError::from_io(
        LocalFileOperation::Copy,
        Some(source.to_path_buf()),
        Some(target.to_path_buf()),
        error,
    )
}
