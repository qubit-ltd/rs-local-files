// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Host-path validation and symbolic-link resolution.

use std::fs;
use std::io;
use std::path::Path;
use std::path::PathBuf;

use crate::LocalFileError;
use crate::LocalFileErrorKind;
use crate::LocalFileOperation;
use crate::LocalResult;
use crate::LocalSymlinkPolicy;

/// Resolves Host path components according to a symbolic-link policy.
///
/// Final-link following is selected by the operation: readers and append
/// writers follow the final component, while metadata, delete, and rename keep
/// it as an entry. Missing final components remain unresolved so create and
/// replace operations can apply their native conflict semantics.
///
/// # Errors
///
/// Returns `LocalFileError` when binding, inspecting, or resolving a path
/// fails, or when the policy forbids a required symbolic-link traversal.
pub(crate) fn resolve_host_path(
    path: &Path,
    symlink_policy: LocalSymlinkPolicy,
    follow_final: bool,
) -> LocalResult<PathBuf> {
    let bound = bind_host_path(path)?;
    let mut components = bound.components().peekable();
    let mut resolved = PathBuf::new();
    while let Some(component) = components.next() {
        resolved.push(component.as_os_str());
        if !matches!(component, std::path::Component::Normal(_)) {
            continue;
        }
        let is_final = components.peek().is_none();
        let metadata = match fs::symlink_metadata(&resolved) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == io::ErrorKind::NotFound => continue,
            Err(error) => {
                return Err(LocalFileError::from_io(
                    LocalFileOperation::BindPath,
                    Some(bound),
                    None,
                    error,
                ));
            }
        };
        if !metadata.file_type().is_symlink() || (is_final && !follow_final) {
            continue;
        }
        if !symlink_policy.follows() {
            return Err(
                LocalFileError::new(LocalFileErrorKind::Unsupported, LocalFileOperation::BindPath)
                    .with_reason("path resolution requires following a symbolic link")
                    .with_path(bound),
            );
        }
        resolved = fs::canonicalize(&resolved)
            .map_err(|error| LocalFileError::from_io(LocalFileOperation::BindPath, Some(bound.clone()), None, error))?;
    }
    Ok(resolved)
}

/// Validates one path already resolved by the public Host facade.
///
/// # Parameters
///
/// - `path`: Native absolute Host path.
///
/// # Returns
///
/// An owned copy of the validated absolute path.
///
/// # Errors
///
/// Returns `LocalFileError` when the internal caller violates the facade's
/// absolute-path invariant.
pub(super) fn bind_host_path(path: &Path) -> LocalResult<PathBuf> {
    if !path.is_absolute() {
        return Err(
            LocalFileError::new(LocalFileErrorKind::InvalidPath, LocalFileOperation::BindPath)
                .with_reason("Host backend paths must be resolved by the public filesystem facade")
                .with_path(path.to_path_buf()),
        );
    }
    Ok(path.to_path_buf())
}

/// Validates two paths already resolved by the public Host facade.
///
/// # Parameters
///
/// - `paths`: Native Host paths to bind.
///
/// # Returns
///
/// Owned copies of both validated absolute paths.
///
/// # Errors
///
/// Returns `LocalFileError` when either internal input violates the facade's
/// absolute-path invariant.
pub(super) fn bind_host_paths(paths: [&Path; 2]) -> LocalResult<[PathBuf; 2]> {
    Ok([bind_host_path(paths[0])?, bind_host_path(paths[1])?])
}
