// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Budget, deadline, and frame helpers for local directory walking.

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
pub(super) fn walker_deadline(root: &Path, options: &LocalListOptions) -> LocalResult<Option<Instant>> {
    let Some(duration) = options.deadline() else {
        return Ok(None);
    };
    let Some(deadline) = Instant::now().checked_add(duration) else {
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

/// Measures the platform-native storage size of one directory-entry name.
pub(super) fn name_bytes(name: &std::ffi::OsStr) -> usize {
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
    policy == crate::LocalWalkErrorPolicy::FailFast
        || error.kind() == LocalFileErrorKind::ResourceLimit
        || error
            .io_error()
            .is_some_and(|source| source.kind() == std::io::ErrorKind::TimedOut)
}
