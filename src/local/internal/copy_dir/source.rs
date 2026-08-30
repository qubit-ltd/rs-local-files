// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Source metadata, cycle identity, and containment checks for recursive copy.
// qubit-style: allow source-test-pair
// Private behavior is covered through public integration tests.

use std::fs;
use std::io::Error;
use std::io::ErrorKind;
use std::io::Result;
use std::path::Path;

use super::super::directory_identity::DirectoryIdentity;
use crate::LocalSymlinkPolicy;

/// Inspects a source directory before recursive copy enters it.
///
/// # Parameters
///
/// * `src` - Source directory path.
/// * `symlink_policy` - Symbolic-link policy used while inspecting the source.
/// * `destination_root` - Canonical destination root including missing tail.
///
/// # Returns
///
/// Source metadata and filesystem-object identity.
///
/// # Errors
///
/// Returns an I/O error when the source is invalid, cannot be canonicalized,
/// or would contain the destination root.
pub(super) fn inspect_copy_source_directory(
    src: &Path,
    symlink_policy: LocalSymlinkPolicy,
    destination_root: &Path,
) -> Result<(fs::Metadata, DirectoryIdentity)> {
    let source_metadata = metadata_for_copy_source(src, symlink_policy)?;
    if !source_metadata.is_dir() {
        return Err(Error::new(
            ErrorKind::InvalidInput,
            format!("source is not a directory: {}", src.display()),
        ));
    }
    let canonical_source = fs::canonicalize(src)?;
    reject_destination_inside_source(src, &canonical_source, destination_root)?;
    let source_identity = DirectoryIdentity::from_metadata(&source_metadata, &canonical_source);
    Ok((source_metadata, source_identity))
}

/// Loads source metadata according to the symbolic-link policy.
///
/// # Parameters
///
/// * `path` - Source path to inspect.
/// * `symlink_policy` - Symbolic-link policy used while inspecting the source.
///
/// # Returns
///
/// Metadata for the source entry or its allowed link target.
///
/// # Errors
///
/// Returns an I/O error when metadata cannot be loaded, or `Unsupported` when
/// a symbolic link is forbidden.
pub(super) fn metadata_for_copy_source(path: &Path, symlink_policy: LocalSymlinkPolicy) -> Result<fs::Metadata> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() {
        if symlink_policy.follows() {
            fs::metadata(path)
        } else {
            Err(Error::new(
                ErrorKind::Unsupported,
                format!("symbolic links are not followed: {}", path.display()),
            ))
        }
    } else {
        Ok(metadata)
    }
}

/// Reports whether metadata represents a real, non-link directory.
///
/// # Parameters
///
/// * `metadata` - Metadata loaded without following the final component.
///
/// # Returns
///
/// `true` only for a non-symbolic-link directory.
#[must_use]
pub(super) fn is_real_directory(metadata: &fs::Metadata) -> bool {
    metadata.is_dir() && !metadata.file_type().is_symlink()
}

/// Rejects destinations equal to or nested under a source directory.
///
/// # Parameters
///
/// * `src` - Source path retained for diagnostics.
/// * `canonical_source` - Canonical source directory.
/// * `destination` - Canonical destination root including missing tail.
///
/// # Errors
///
/// Returns `InvalidInput` when the destination is inside the source.
fn reject_destination_inside_source(src: &Path, canonical_source: &Path, destination: &Path) -> Result<()> {
    if destination == canonical_source || destination.starts_with(canonical_source) {
        return Err(Error::new(
            ErrorKind::InvalidInput,
            format!(
                "destination must not be inside source: source={}, destination={}",
                src.display(),
                destination.display(),
            ),
        ));
    }
    Ok(())
}
