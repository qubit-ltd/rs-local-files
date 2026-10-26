// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Budget, deadline, and frame helpers for local directory walking.

use std::ffi::OsStr;
use std::path::Path;
use std::time::Instant;

use qubit_budget::InsufficientBudgetError;
use qubit_budget::ManagedResourcePool;

use super::internal::WalkFrame;
use crate::LocalFileError;
use crate::LocalFileErrorKind;
use crate::LocalFileOperation;
use crate::LocalListOptions;
use crate::LocalResourceKind;
use crate::LocalResourceLimitError;
use crate::LocalResult;

/// Closes one host frame reader and drops its capacity permit.
pub(super) fn close_host_frame(frame: &mut WalkFrame) {
    let _ = frame.entries.take();
    let _ = frame.directory_permit.take();
}

/// Creates the established listing error for exhausted directory capacity.
pub(super) fn directory_limit_error(
    path: &Path,
    error: InsufficientBudgetError<LocalResourceKind, usize>,
) -> LocalFileError {
    let InsufficientBudgetError {
        resource,
        limit,
        remaining,
        requested,
    } = error;
    LocalFileError::from_resource_limit(
        LocalFileOperation::List,
        Some(path.to_path_buf()),
        LocalResourceLimitError::new(resource, limit, remaining, requested),
    )
}

/// Validates options that must hold before a walker can be constructed.
pub(super) fn validate_options(root: &Path, options: &LocalListOptions) -> LocalResult<()> {
    if options.max_open_directories() == Some(0) {
        return Err(
            LocalFileError::new(LocalFileErrorKind::InvalidOptions, LocalFileOperation::List)
                .with_path(root.to_path_buf())
                .with_reason("maximum open directory count must be greater than zero"),
        );
    }
    Ok(())
}

/// Converts the relative deadline into one checked monotonic instant.
pub(super) fn walker_deadline(
    root: &Path,
    options: &LocalListOptions,
    started_at: Instant,
) -> LocalResult<Option<Instant>> {
    let Some(duration) = options.deadline() else {
        return Ok(None);
    };
    let Some(deadline) = started_at.checked_add(duration) else {
        return Err(
            LocalFileError::new(LocalFileErrorKind::InvalidOptions, LocalFileOperation::List)
                .with_path(root.to_path_buf())
                .with_reason("listing deadline exceeds the monotonic clock range"),
        );
    };
    Ok(Some(deadline))
}

/// Creates the finite pool that accounts for opened directory readers.
pub(super) fn directory_pool(options: &LocalListOptions) -> Option<ManagedResourcePool<LocalResourceKind, usize>> {
    options
        .max_open_directories()
        .map(|limit| ManagedResourcePool::new(LocalResourceKind::OpenDirectory, limit))
}

/// Scripts a capacity failure while entering the selected `nested` child.
///
/// The fault attempts to reserve one additional handle at the child frame's
/// actual acquisition point, while its parent still owns the configured
/// capacity.
#[cfg(feature = "test-support")]
pub(super) fn scripted_open_directory_error(
    pool: &Option<ManagedResourcePool<LocalResourceKind, usize>>,
    path: &Path,
) -> Option<LocalFileError> {
    if path.file_name() != Some(OsStr::new("nested")) {
        return None;
    }
    if !crate::local::take_test_support("walker-enter-open-host")
        && !crate::local::take_test_support("walker-enter-open-rooted")
    {
        return None;
    }
    let pool = pool.as_ref()?;
    match pool.try_acquire(1) {
        Ok(_permit) => None,
        Err(error) => Some(directory_limit_error(path, error)),
    }
}

/// Measures the platform-native storage size of one directory-entry name.
pub(super) fn name_bytes(name: &OsStr) -> usize {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        name.as_bytes().len()
    }
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        name.encode_wide().count().saturating_mul(2)
    }
    #[cfg(not(any(unix, windows)))]
    {
        name.to_string_lossy().len()
    }
}

/// Reports whether an iterator error must terminate global traversal state.
#[must_use]
pub(super) fn is_terminal_walk_error(error: &LocalFileError, policy: crate::LocalWalkErrorPolicy) -> bool {
    if policy == crate::LocalWalkErrorPolicy::FailFast {
        return true;
    }
    let global_resource_limit = error
        .resource_limit_error()
        .is_some_and(|limit| limit.resource() != LocalResourceKind::OpenDirectory);
    global_resource_limit
        || error
            .io_error()
            .is_some_and(|source| source.kind() == std::io::ErrorKind::TimedOut)
}
