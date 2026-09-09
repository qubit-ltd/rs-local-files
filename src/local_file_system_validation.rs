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

use crate::LocalAtomicityRequirement;
use crate::LocalCopyOptions;
use crate::LocalCopySourceMode;
use crate::LocalDurabilityRequirement;
use crate::LocalFileError;
use crate::LocalFileErrorKind;
use crate::LocalFileOperation;
use crate::LocalFileSystemCapabilities;
use crate::LocalFileSystemScope;
use crate::LocalListOptions;
use crate::LocalNamespacePath;
use crate::LocalRenameOptions;
use crate::LocalResult;
use crate::LocalSymlinkPolicy;
use crate::LocalWriteMode;
use crate::LocalWriteOptions;

/// Rejects a file operation whose original syntax explicitly requires a
/// directory.
/// Returns `InvalidPath` with the namespace path and optional PWD snapshot;
/// operands without the directory requirement succeed without I/O.
pub(super) fn reject_directory_qualified_file(
    path: &LocalNamespacePath,
    operation: LocalFileOperation,
    current_directory: Option<&Path>,
) -> LocalResult<()> {
    if !path.directory_required() {
        return Ok(());
    }
    let error = LocalFileError::new(LocalFileErrorKind::InvalidPath, operation)
        .with_reason("a directory-qualified path cannot be used as a file")
        .with_path(path.namespace_absolute().to_path_buf());
    match current_directory {
        Some(current_directory) => Err(error.with_current_directory(current_directory.to_path_buf())),
        None => Err(error),
    }
}

/// Validates scope-dependent symlink policy.
///
/// Returns `InvalidOptions` only for `FollowAcrossScope` in Rooted scope,
/// retaining the supplied operation and optional path for diagnostics.
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
///
/// Returns `InvalidOptions` for a zero open-directory limit or a forbidden
/// scope policy. An absent path identifies configuration-time validation.
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
    if options
        .deadline()
        .is_some_and(|duration| Instant::now().checked_add(duration).is_none())
    {
        let mut error = LocalFileError::new(LocalFileErrorKind::InvalidOptions, operation)
            .with_reason("listing deadline exceeds the monotonic clock range");
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

/// Validates copy scope policy and monotonic deadline representation.
///
/// Returns `InvalidOptions` for a forbidden scope policy or an unrepresentable
/// deadline. Other budget values and source-dependent requirements are checked
/// during execution. An absent source identifies configuration-time validation.
pub(super) fn validate_copy_options(
    scope: LocalFileSystemScope,
    default_policy: LocalSymlinkPolicy,
    options: &LocalCopyOptions,
    capabilities: LocalFileSystemCapabilities,
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
    let tree = options.source_mode() == LocalCopySourceMode::Tree;
    if (tree && options.atomicity() == LocalAtomicityRequirement::Required)
        || (options.durability() == LocalDurabilityRequirement::Required
            && (tree || !capabilities.supports_durable_file_copy()))
    {
        let mut error = LocalFileError::new(LocalFileErrorKind::RequirementNotMet, operation)
            .with_reason("the requested copy guarantee is unavailable for these options");
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
///
/// Returns `InvalidOptions` for `Some(0)`; `None` permits unbounded retries.
pub(super) fn validate_temp_attempts(max_attempts: Option<usize>, operation: LocalFileOperation) -> LocalResult<()> {
    if max_attempts != Some(0) {
        return Ok(());
    }
    Err(LocalFileError::new(LocalFileErrorKind::InvalidOptions, operation)
        .with_reason("temporary entry attempt count must be greater than zero"))
}

/// Validates writer guarantees without resolving paths or touching the
/// filesystem.
///
/// Returns `RequirementNotMet` with `operation` when append requires atomicity
/// or this build cannot provide the requested staging-write durability.
pub(super) fn validate_write_options(
    options: &LocalWriteOptions,
    capabilities: LocalFileSystemCapabilities,
    operation: LocalFileOperation,
) -> LocalResult<()> {
    let append = options.mode() == LocalWriteMode::Append;

    if (append && options.atomicity() == LocalAtomicityRequirement::Required)
        || (!append
            && options.durability() == LocalDurabilityRequirement::Required
            && !capabilities.supports_durable_write())
    {
        return Err(LocalFileError::new(LocalFileErrorKind::RequirementNotMet, operation)
            .with_reason("the requested writer guarantee is unavailable for these options"));
    }
    Ok(())
}

/// Validates build-level rename guarantees before path resolution.
///
/// Returns `RequirementNotMet` with `operation` if native rename or requested
/// durability is unavailable. Runtime mount restrictions remain operation
/// errors.
pub(super) fn validate_rename_options(
    options: &LocalRenameOptions,
    capabilities: LocalFileSystemCapabilities,
    operation: LocalFileOperation,
) -> LocalResult<()> {
    if !capabilities.supports_atomic_rename()
        || (options.durability() == LocalDurabilityRequirement::Required && !capabilities.supports_durable_rename())
    {
        return Err(LocalFileError::new(LocalFileErrorKind::RequirementNotMet, operation)
            .with_reason("the requested rename guarantee is unavailable on this build"));
    }
    Ok(())
}

/// Validates temporary naming and collision options without filesystem access.
///
/// `prefix` and `suffix` are optional filename fragments; `max_attempts` is
/// the collision budget. Returns `InvalidOptions` with `operation` for invalid
/// fragments or a zero budget, retaining the native validation cause.
pub(super) fn validate_temp_options(
    prefix: Option<&str>,
    suffix: Option<&str>,
    max_attempts: Option<usize>,
    operation: LocalFileOperation,
) -> LocalResult<()> {
    validate_temp_attempts(max_attempts, operation)?;
    crate::local::validate_temp_affixes(prefix, suffix).map_err(|error| {
        LocalFileError::from_io(operation, None, None, error).with_kind(LocalFileErrorKind::InvalidOptions)
    })
}
