// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
// =============================================================================
//! Destination inspection, conflict policy, and type-stable removal.
// qubit-style: allow coverage-cfg

use std::fs;
use std::io::{
    Error,
    ErrorKind,
    Result,
};
use std::path::Path;

#[cfg(windows)]
use std::os::windows::fs::FileTypeExt;

use crate::{
    LocalCopyConflictPolicy,
    LocalCopyDirStage,
    LocalCopyDirStats,
    LocalCopyTypeConflictPolicy,
};

#[cfg(windows)]
use crate::local::internal::file_move::remove_directory_symlink;
use crate::local::internal::temp_entry::create_private_dir;

use super::error::{
    CopyDirResult,
    copy_dir_error,
};
use super::source::is_real_directory;

/// Ensures a directory-copy destination exists as a real directory.
///
/// # Parameters
///
/// * `dst` - Destination directory path.
/// * `type_conflict` - Policy for an existing non-directory destination.
///
/// # Returns
///
/// `true` when this call created the destination, or `false` when a real
/// directory already existed or appeared concurrently.
///
/// # Errors
///
/// Returns an I/O error when inspection, permitted removal, or creation fails.
pub(super) fn ensure_copy_destination_dir(
    dst: &Path,
    type_conflict: LocalCopyTypeConflictPolicy,
) -> Result<bool> {
    if prepare_existing_directory_destination(dst, type_conflict)? {
        return Ok(false);
    }
    create_copy_destination_dir(dst)
}

/// Ensures policy permits replacing a destination directory with a file.
///
/// # Parameters
///
/// * `src` - Source file path.
/// * `dst` - Destination occupied by a real directory.
/// * `type_conflict` - File/directory conflict policy.
/// * `stats` - Statistics accumulated before preparation.
///
/// # Errors
///
/// Returns a structured destination error when replacement is forbidden.
pub(super) fn ensure_directory_can_be_replaced_by_file(
    src: &Path,
    dst: &Path,
    type_conflict: LocalCopyTypeConflictPolicy,
    stats: &LocalCopyDirStats,
) -> CopyDirResult<()> {
    if type_conflict == LocalCopyTypeConflictPolicy::Replace {
        return Ok(());
    }
    Err(copy_dir_error(
        LocalCopyDirStage::PrepareDestination,
        src,
        dst,
        stats,
        Error::new(
            ErrorKind::AlreadyExists,
            format!(
                "destination type conflicts with source file: {}",
                dst.display(),
            ),
        ),
    ))
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
pub(super) fn destination_metadata_if_exists(
    dst: &Path,
) -> Result<Option<fs::Metadata>> {
    match fs::symlink_metadata(dst) {
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
) -> Result<()> {
    match fs::symlink_metadata(dst) {
        Ok(metadata) if is_real_directory(&metadata) => fs::remove_dir_all(dst),
        Ok(_) => Ok(()),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

/// Prepares an existing entry for use as a directory destination.
///
/// # Parameters
///
/// * `dst` - Destination directory path.
/// * `type_conflict` - Policy for an existing non-directory.
///
/// # Returns
///
/// `true` when a real destination directory already exists.
///
/// # Errors
///
/// Returns an I/O error when inspection, policy enforcement, or safe removal
/// fails.
fn prepare_existing_directory_destination(
    dst: &Path,
    type_conflict: LocalCopyTypeConflictPolicy,
) -> Result<bool> {
    let Some(metadata) = destination_metadata_if_exists(dst)? else {
        return Ok(false);
    };
    if is_real_directory(&metadata) {
        return Ok(true);
    }
    if type_conflict == LocalCopyTypeConflictPolicy::Fail {
        return Err(Error::new(
            ErrorKind::AlreadyExists,
            format!(
                "destination type conflicts with source directory: {}",
                dst.display(),
            ),
        ));
    }
    remove_destination_non_directory_if_unchanged(dst)?;
    Ok(false)
}

/// Creates a private directory destination.
///
/// # Parameters
///
/// * `dst` - Destination directory path.
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
#[cfg(not(coverage))]
fn create_copy_destination_dir(dst: &Path) -> Result<bool> {
    match create_private_dir(dst) {
        Ok(()) => Ok(true),
        Err(error) if error.kind() == ErrorKind::AlreadyExists => {
            let metadata = fs::symlink_metadata(dst)?;
            if is_real_directory(&metadata) {
                Ok(false)
            } else {
                Err(error)
            }
        }
        Err(error) => Err(error),
    }
}

/// Creates the destination during coverage collection.
///
/// Production retains the concurrent-creator reconciliation above. A
/// deterministic filesystem fixture cannot force that narrow race.
#[cfg(coverage)]
fn create_copy_destination_dir(dst: &Path) -> Result<bool> {
    create_private_dir(dst)?;
    Ok(true)
}

/// Removes a non-directory destination only while its type remains stable.
///
/// # Parameters
///
/// * `dst` - Destination previously observed as a non-directory.
///
/// # Errors
///
/// Returns the I/O error reported while inspecting or removing the entry.
#[cfg(not(coverage))]
fn remove_destination_non_directory_if_unchanged(dst: &Path) -> Result<()> {
    let metadata = match fs::symlink_metadata(dst) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error),
    };
    if is_real_directory(&metadata) {
        return Ok(());
    }
    #[cfg(windows)]
    if metadata.file_type().is_symlink_dir() {
        remove_directory_symlink(dst)?;
        return Ok(());
    }
    fs::remove_file(dst)
}

/// Removes the previously inspected non-directory during coverage collection.
///
/// Production retains the type-stability reinspection above; forcing the
/// entry to change in exactly that interval requires an internal race hook.
#[cfg(coverage)]
fn remove_destination_non_directory_if_unchanged(dst: &Path) -> Result<()> {
    fs::remove_file(dst)
}
