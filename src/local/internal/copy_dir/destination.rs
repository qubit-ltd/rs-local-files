// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Destination inspection, conflict policy, and type-stable removal.
// Private behavior is covered through public integration tests.

use std::fs;
use std::io::Error;
use std::io::ErrorKind;
use std::io::Result;
#[cfg(windows)]
use std::os::windows::fs::FileTypeExt;
use std::path::Path;
use std::time::Instant;

use super::copy_dir_result::CopyDirResult;
use super::error::copy_dir_error;
use super::namespace_race::reconcile_directory_creation;
use super::namespace_race::removable_non_directory_metadata;
use super::source::is_real_directory;
use crate::LocalCopyConflictPolicy;
use crate::LocalCopyDirStage;
use crate::LocalCopyDirStats;
use crate::LocalCopyTypeConflictPolicy;
use crate::LocalDeleteOptions;
use crate::LocalSymlinkPolicy;
use crate::error::LocalFileEffectState;
use crate::local::CopyDestinationAction;
use crate::local::decide_copy_destination;
#[cfg(windows)]
use crate::local::internal::file_move::remove_directory_symlink;
use crate::local::internal::temp_entry::create_private_dir;
#[cfg(feature = "test-support")]
use crate::local::internal::test_support;

/// Prepares a real directory destination unless conflict policy skips it.
///
/// # Parameters
///
/// * `dst` - Destination directory path.
/// * `conflict` - Same-kind conflict policy passed to the shared decision.
/// * `type_conflict` - Policy for an existing non-directory destination.
/// * `destination_removed` - Set after this call removes a conflicting entry,
///   including when subsequent directory creation fails.
///
/// # Returns
///
/// The preparation action and whether this call created the directory.
/// `Replace` preserves replacement accounting separately from creation;
/// `Merge` denotes an existing real directory, and `Skip` preserves an
/// incompatible destination. A concurrent creator can make creation false.
///
/// # Errors
///
/// Returns an I/O error when inspection, permitted removal, or creation fails.
pub(super) fn ensure_copy_destination_dir(
    dst: &Path,
    conflict: LocalCopyConflictPolicy,
    type_conflict: LocalCopyTypeConflictPolicy,
    destination_removed: &mut bool,
) -> Result<(CopyDestinationAction, bool)> {
    let action = prepare_existing_directory_destination(dst, conflict, type_conflict, destination_removed)?;
    if !matches!(action, CopyDestinationAction::Create | CopyDestinationAction::Replace) {
        return Ok((action, false));
    }
    create_copy_destination_dir(dst).map(|created| {
        let action = if !created && action == CopyDestinationAction::Create {
            CopyDestinationAction::Merge
        } else {
            action
        };
        (action, created)
    })
}

/// Applies file-conflict policy to an existing non-directory destination.
///
/// # Parameters
///
/// * `src` - Source file path.
/// * `dst` - Existing destination path.
/// * `conflict` - Existing-file conflict policy.
/// * `stats` - Statistics accumulated before preparation.
///
/// # Returns
///
/// `true` when the entry should be skipped, otherwise `false`.
///
/// # Errors
///
/// Returns a structured destination error when policy requires failure.
pub(super) fn existing_file_destination_should_be_skipped(
    src: &Path,
    dst: &Path,
    conflict: LocalCopyConflictPolicy,
    stats: &LocalCopyDirStats,
) -> CopyDirResult<bool> {
    match conflict {
        LocalCopyConflictPolicy::Fail => Err(copy_dir_error(
            LocalCopyDirStage::PrepareDestination,
            src,
            dst,
            stats,
            Error::new(
                ErrorKind::AlreadyExists,
                format!("destination already exists: {}", dst.display()),
            ),
        )),
        LocalCopyConflictPolicy::Skip => Ok(true),
        LocalCopyConflictPolicy::Overwrite => Ok(false),
    }
}

/// Reads destination metadata while treating a missing path as empty.
///
/// # Parameters
///
/// * `dst` - Destination to inspect without following its final component.
///
/// # Returns
///
/// Existing metadata, or `None` when the destination is missing.
///
/// # Errors
///
/// Returns metadata errors other than `NotFound`.
pub(super) fn destination_metadata_if_exists(dst: &Path) -> Result<Option<fs::Metadata>> {
    match inspect_destination_metadata(dst) {
        Ok(metadata) => Ok(Some(metadata)),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

/// Removes a directory destination only while it remains a real directory.
///
/// # Parameters
///
/// * `dst` - Destination previously observed as a real directory.
///
/// # Errors
///
/// Returns the I/O error reported while inspecting or removing the directory.
pub(super) fn remove_destination_directory_if_unchanged(
    dst: &Path,
    options: &LocalDeleteOptions,
    symlink_policy: LocalSymlinkPolicy,
    started_at: Instant,
) -> std::result::Result<(), (Error, Option<crate::LocalCopyFailureState>)> {
    match inspect_destination_metadata(dst) {
        Ok(metadata) if is_real_directory(&metadata) => {
            #[cfg(feature = "test-support")]
            if test_support::is_enabled("copy-destination-remove-after-child") {
                let child = fs::read_dir(dst)
                    .map_err(|error| (error, None))?
                    .next()
                    .transpose()
                    .map_err(|error| (error, None))?
                    .ok_or_else(|| {
                        (
                            Error::other("injected partial destination removal requires a child"),
                            None,
                        )
                    })?;
                if child.file_type().map_err(|error| (error, None))?.is_dir() {
                    fs::remove_dir_all(child.path()).map_err(|error| (error, None))?;
                } else {
                    fs::remove_file(child.path()).map_err(|error| (error, None))?;
                }
                return Err((
                    super::destination_removal_failure::mark_attempted(Error::other(
                        "injected failure after partial destination removal",
                    )),
                    Some(crate::LocalCopyFailureState::Indeterminate),
                ));
            }
            crate::local::internal::host_local_file_system::HostLocalFileSystem::delete_directory_with_policy(
                dst,
                options,
                symlink_policy,
                started_at,
            )
            .map(|_| ())
            .map_err(|error| {
                let state = match error.effect_state() {
                    Some(LocalFileEffectState::PartiallyApplied | LocalFileEffectState::Applied) => {
                        crate::LocalCopyFailureState::PartiallyPublished
                    }
                    Some(LocalFileEffectState::Indeterminate) | None => crate::LocalCopyFailureState::Indeterminate,
                    Some(LocalFileEffectState::Unchanged) => crate::LocalCopyFailureState::Unchanged,
                };
                (error.into_io_error(), Some(state))
            })
        }
        Ok(_) => Ok(()),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
        Err(error) => Err((error, None)),
    }
}
/// Prepares an existing entry for use as a directory destination.
///
/// # Parameters
///
/// * `dst` - Destination directory path.
/// * `conflict` - Same-kind conflict policy passed to the shared decision.
/// * `type_conflict` - Policy for an existing non-directory.
/// * `destination_removed` - Set only after a conflicting entry was removed.
///
/// # Returns
///
/// `Create` when the path is absent, `Replace` after permitted incompatible
/// entry removal, `Merge` for a real directory, or `Skip` when policy preserves
/// the entry.
///
/// # Errors
///
/// Returns an I/O error when inspection, policy enforcement, or safe removal
/// fails.
fn prepare_existing_directory_destination(
    dst: &Path,
    conflict: LocalCopyConflictPolicy,
    type_conflict: LocalCopyTypeConflictPolicy,
    destination_removed: &mut bool,
) -> Result<CopyDestinationAction> {
    let Some(metadata) = destination_metadata_if_exists(dst)? else {
        return Ok(CopyDestinationAction::Create);
    };
    let action = decide_copy_destination(true, Some(is_real_directory(&metadata)), conflict, type_conflict)
        .ok_or_else(|| {
            Error::new(
                ErrorKind::AlreadyExists,
                format!("destination type conflicts with source directory: {}", dst.display(),),
            )
        })?;
    if action == CopyDestinationAction::Replace {
        *destination_removed = remove_destination_non_directory_if_unchanged(dst)?;
    }
    Ok(action)
}

/// Creates a private directory destination.
///
/// # Parameters
///
/// * `dst` - The destination path to create or inspect after a creation race.
///
/// # Returns
///
/// `true` when this call created the directory, or `false` when a real
/// directory appeared concurrently.
///
/// # Errors
///
/// Returns an I/O error when creation fails or a racing entry is not a real
/// directory.
fn create_copy_destination_dir(dst: &Path) -> Result<bool> {
    #[cfg(feature = "test-support")]
    let race_file = test_support::is_enabled("copy-directory-race-nondirectory");
    #[cfg(feature = "test-support")]
    let race_directory = test_support::is_enabled("copy-directory-race-existing")
        || test_support::is_enabled("copy-directory-race-inspect");
    #[cfg(feature = "test-support")]
    if test_support::is_enabled("copy-directory-create-error") {
        return Err(Error::other("injected directory creation failure"));
    }
    #[cfg(feature = "test-support")]
    if race_file {
        fs::write(dst, b"racing creator")?;
    } else if race_directory {
        fs::create_dir(dst)?;
    }
    let result = create_private_dir(dst);
    reconcile_directory_creation(dst, result, |path| {
        #[cfg(feature = "test-support")]
        if test_support::is_enabled("copy-directory-race-inspect") {
            return Err(Error::other("injected directory race inspection failure"));
        }
        inspect_destination_metadata(path)
    })
}

/// Removes a non-directory destination only while its type remains stable.
///
/// # Parameters
///
/// * `dst` - The destination path to inspect and, when still appropriate,
///   remove.
///
/// # Returns
///
/// `true` after removing the entry, or `false` when the entry is absent or
/// observed as a real directory.
///
/// # Errors
///
/// Returns the I/O error reported while inspecting or removing the entry.
fn remove_destination_non_directory_if_unchanged(dst: &Path) -> Result<bool> {
    let result = inspect_destination_metadata(dst);
    #[cfg(feature = "test-support")]
    let result = if test_support::is_enabled("copy-removal-race-not-found") {
        Err(Error::new(ErrorKind::NotFound, "injected destination disappearance"))
    } else if test_support::is_enabled("copy-removal-race-inspect") {
        Err(Error::other("injected destination reinspection failure"))
    } else {
        result
    };
    removable_non_directory_metadata(result)?.map_or(Ok(false), |metadata| {
        #[cfg(windows)]
        if metadata.file_type().is_symlink_dir() {
            remove_directory_symlink(dst)?;
            return Ok(true);
        }
        #[cfg(not(windows))]
        let _ = metadata;
        fs::remove_file(dst).map(|()| true)
    })
}

/// Reads destination metadata without following the final path component.
///
/// This shared operation keeps ordinary inspection and race-only reinspection
/// on the same covered filesystem call site.
///
/// # Parameters
///
/// * `dst` - The destination path to inspect.
///
/// # Returns
///
/// Metadata for the destination entry.
///
/// # Errors
///
/// Returns the I/O error reported by `symlink_metadata`.
#[inline]
fn inspect_destination_metadata(dst: &Path) -> Result<fs::Metadata> {
    fs::symlink_metadata(dst)
}

#[cfg(test)]
mod tests {
    use super::prepare_existing_directory_destination;
    use super::remove_destination_directory_if_unchanged;
    use crate::LocalCopyConflictPolicy;
    use crate::LocalCopyTypeConflictPolicy;
    use crate::local::CopyDestinationAction;

    #[test]
    fn unchanged_real_directory_destination_can_be_removed() {
        let parent = tempfile::tempdir().expect("parent directory should be created");
        let parent_path = std::fs::canonicalize(parent.path()).expect("parent path should canonicalize");
        let absent = parent_path.join("absent");
        let mut destination_removed = false;
        assert_eq!(
            prepare_existing_directory_destination(
                &absent,
                LocalCopyConflictPolicy::Fail,
                LocalCopyTypeConflictPolicy::Fail,
                &mut destination_removed,
            )
            .expect("missing destination should be prepared"),
            CopyDestinationAction::Create
        );
        assert!(!destination_removed);
        let destination = parent_path.join("destination");
        std::fs::create_dir(&destination).expect("destination should be created");
        std::fs::write(destination.join("entry"), b"content").expect("child should be written");

        remove_destination_directory_if_unchanged(
            &destination,
            &crate::LocalDeleteOptions::new().with_recursive(),
            crate::LocalSymlinkPolicy::Reject,
            std::time::Instant::now(),
        )
        .expect("real directory should be removed");

        assert!(!destination.exists());
    }
}
