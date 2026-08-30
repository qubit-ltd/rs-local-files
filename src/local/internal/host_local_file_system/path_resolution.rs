// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Host-path binding and symbolic-link resolution.

use std::env;
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

/// Binds a relative Host path to one current-working-directory snapshot.
///
/// Absolute paths are returned unchanged.
///
/// # Parameters
///
/// - `path`: Native absolute or relative Host path.
///
/// # Returns
///
/// An absolute path that remains stable if the process working directory
/// changes.
///
/// # Errors
///
/// Returns `LocalFileError` when a Windows drive-relative prefix is supplied
/// or the current directory cannot be read.
pub(super) fn bind_host_path(path: &Path) -> LocalResult<PathBuf> {
    if path.is_absolute() {
        return Ok(path.to_path_buf());
    }
    if has_native_prefix(path) {
        return Err(
            LocalFileError::new(LocalFileErrorKind::InvalidPath, LocalFileOperation::BindPath)
                .with_path(path.to_path_buf()),
        );
    }
    current_directory_for_binding("local-path-bind-cwd")
        .map(|current| current.join(path))
        .map_err(|source| LocalFileError::from_io(LocalFileOperation::BindPath, Some(path.to_path_buf()), None, source))
}

/// Binds two Host paths using exactly one current-directory snapshot.
///
/// # Parameters
///
/// - `paths`: Native Host paths to bind.
///
/// # Returns
///
/// Absolute paths bound to the same Host namespace snapshot.
///
/// # Errors
///
/// Returns `LocalFileError` when either input has a Windows drive-relative
/// prefix or the current directory cannot be read.
pub(super) fn bind_host_paths(paths: [&Path; 2]) -> LocalResult<[PathBuf; 2]> {
    if let Some(path) = paths
        .iter()
        .copied()
        .find(|path| path.is_relative() && has_native_prefix(path))
    {
        return Err(
            LocalFileError::new(LocalFileErrorKind::InvalidPath, LocalFileOperation::BindPath)
                .with_path(path.to_path_buf()),
        );
    }
    let current = if paths.iter().any(|path| path.is_relative()) {
        Some(
            current_directory_for_binding("local-paths-bind-cwd")
                .map_err(|source| LocalFileError::from_io(LocalFileOperation::BindPath, None, None, source))?,
        )
    } else {
        None
    };
    Ok(paths.map(|path| {
        current
            .as_ref()
            .map_or_else(|| path.to_path_buf(), |directory| directory.join(path))
    }))
}

/// Reports whether a Windows path carries a namespace prefix.
#[cfg(windows)]
#[must_use]
fn has_native_prefix(path: &Path) -> bool {
    matches!(path.components().next(), Some(std::path::Component::Prefix(_)))
}

/// Reports that Unix paths have no platform prefix component.
#[cfg(not(windows))]
#[must_use]
const fn has_native_prefix(_path: &Path) -> bool {
    false
}

/// Reads the Host current directory used to bind a relative path.
///
/// # Parameters
///
/// - `fault`: Test-support-only fault selector.
///
/// # Returns
///
/// The current directory snapshot.
///
/// # Errors
///
/// Returns the native current-directory error or an enabled test fault.
#[cfg(feature = "internal-test-support")]
fn current_directory_for_binding(fault: &str) -> io::Result<PathBuf> {
    if crate::local::test_support_enabled(fault) {
        return Err(crate::local::test_fault_error());
    }
    env::current_dir()
}

/// Reads the Host current directory used to bind a relative path.
///
/// # Parameters
///
/// - `fault`: Ignored when test support is disabled.
///
/// # Returns
///
/// The current directory snapshot.
///
/// # Errors
///
/// Returns the native current-directory error.
#[cfg(not(feature = "internal-test-support"))]
fn current_directory_for_binding(_fault: &str) -> io::Result<PathBuf> {
    env::current_dir()
}
