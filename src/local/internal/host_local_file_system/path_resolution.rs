// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Host-path binding and symbolic-link resolution.

use std::{
    fs,
    io,
    path::{
        Path,
        PathBuf,
    },
};

use crate::{
    LocalFileError,
    LocalFileErrorKind,
    LocalFileOperation,
    LocalPaths,
    LocalResult,
    LocalSymlinkPolicy,
};

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
    let bound = LocalPaths::bind_host_path(path)?;
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
            return Err(LocalFileError::new(
                LocalFileErrorKind::Unsupported,
                LocalFileOperation::BindPath,
            )
            .with_reason("path resolution requires following a symbolic link")
            .with_path(bound));
        }
        resolved = fs::canonicalize(&resolved).map_err(|error| {
            LocalFileError::from_io(
                LocalFileOperation::BindPath,
                Some(bound.clone()),
                None,
                error,
            )
        })?;
    }
    Ok(resolved)
}
