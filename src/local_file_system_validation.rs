// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Option and namespace validation shared by [`crate::LocalFileSystem`].

use std::path::Path;
use std::time::Instant;

use crate::LocalCopyOptions;
use crate::LocalFileError;
use crate::LocalFileErrorKind;
use crate::LocalFileOperation;
use crate::LocalFileSystemScope;
use crate::LocalListOptions;
use crate::LocalNamespacePath;
use crate::LocalResult;
use crate::LocalSymlinkPolicy;

/// Rejects a file operation whose original syntax explicitly requires a
/// directory.
pub(super) fn reject_directory_qualified_file(
    path: &LocalNamespacePath,
    operation: LocalFileOperation,
    current_directory: &Path,
) -> LocalResult<()> {
    if !path.directory_required() {
        return Ok(());
    }
    Err(LocalFileError::new(LocalFileErrorKind::InvalidPath, operation)
        .with_reason("a directory-qualified path cannot be used as a file")
        .with_path(path.namespace_absolute().to_path_buf())
        .with_current_directory(current_directory.to_path_buf()))
}

/// Validates scope-dependent symlink policy.
pub(super) fn validate_scope_symlink_policy(
    scope: LocalFileSystemScope,
    policy: LocalSymlinkPolicy,
    operation: LocalFileOperation,
    path: Option<&Path>,
) -> LocalResult<()> {
    if scope != LocalFileSystemScope::Rooted || policy != LocalSymlinkPolicy::FollowAcrossScope {
        return Ok(());
    }
    let mut error = LocalFileError::new(LocalFileErrorKind::InvalidOptions, operation)
        .with_reason("FollowAcrossScope is incompatible with a Rooted filesystem");
    if let Some(path) = path {
        error = error.with_path(path.to_path_buf());
    }
    Err(error)
}

/// Validates listing budgets and scope policy without performing I/O.
pub(super) fn validate_list_options(
    scope: LocalFileSystemScope,
    default_policy: LocalSymlinkPolicy,
    options: &LocalListOptions,
    path: Option<&Path>,
) -> LocalResult<()> {
    let operation = if path.is_some() {
        LocalFileOperation::List
    } else {
        LocalFileOperation::Configure
    };
    if options.max_open_directories() == Some(0) {
        let mut error = LocalFileError::new(LocalFileErrorKind::InvalidOptions, operation)
            .with_reason("maximum open directory count must be greater than zero");
        if let Some(path) = path {
            error = error.with_path(path.to_path_buf());
        }
        return Err(error);
    }
    validate_scope_symlink_policy(
        scope,
        options.symlink_policy().unwrap_or(default_policy),
        operation,
        path,
    )
}

/// Validates copy budgets, policy, and monotonic deadline representation.
pub(super) fn validate_copy_options(
    scope: LocalFileSystemScope,
    default_policy: LocalSymlinkPolicy,
    options: &LocalCopyOptions,
    source: Option<&Path>,
    destination: Option<&Path>,
) -> LocalResult<()> {
    let operation = if source.is_some() {
        LocalFileOperation::Copy
    } else {
        LocalFileOperation::Configure
    };
    validate_scope_symlink_policy(
        scope,
        options.symlink_policy_override().unwrap_or(default_policy),
        operation,
        source,
    )?;
    if options
        .deadline()
        .is_some_and(|duration| Instant::now().checked_add(duration).is_none())
    {
        let mut error = LocalFileError::new(LocalFileErrorKind::InvalidOptions, operation)
            .with_reason("copy deadline exceeds the monotonic clock range");
        if let Some(source) = source {
            error = error.with_path(source.to_path_buf());
        }
        if let Some(destination) = destination {
            error = error.with_target(destination.to_path_buf());
        }
        return Err(error);
    }
    Ok(())
}

/// Validates an explicit temporary-name collision budget.
pub(super) fn validate_temp_attempts(max_attempts: Option<usize>, operation: LocalFileOperation) -> LocalResult<()> {
    if max_attempts != Some(0) {
        return Ok(());
    }
    Err(LocalFileError::new(LocalFileErrorKind::InvalidOptions, operation)
        .with_reason("temporary entry attempt count must be greater than zero"))
}
