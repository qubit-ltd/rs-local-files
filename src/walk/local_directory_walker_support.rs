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
use qubit_budget::ResourceBudget;

use super::internal::WalkFrame;
use crate::LocalFileError;
use crate::LocalFileErrorKind;
use crate::LocalFileOperation;
use crate::LocalListOptions;
use crate::LocalResourceKind;
use crate::LocalResourceLimitError;
use crate::LocalResult;

/// Closes one host frame reader and drops its capacity permit.
///
/// # Parameters
///
/// - `frame`: The frame whose reader and permit are released.
pub(super) fn close_host_frame(frame: &mut WalkFrame) {
    let _ = frame.entries.take();
    let _ = frame.directory_permit.take();
}

/// Creates the established listing error for exhausted directory capacity.
///
/// # Parameters
///
/// - `path`: The directory path associated with the failed reservation.
/// - `error`: The resource-pool failure describing the exhausted capacity.
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
///
/// # Parameters
///
/// - `root`: The listing root to include in any validation error.
/// - `options`: The traversal options to validate.
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
///
/// # Parameters
///
/// - `root`: The listing root to include in any deadline error.
/// - `options`: The traversal options containing the optional deadline.
/// - `started_at`: The monotonic start instant from which the deadline is
///   measured.
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
///
/// # Parameters
///
/// - `options`: The traversal options containing the optional directory limit.
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
///
/// # Parameters
///
/// - `pool`: The optional pool used to simulate the extra reservation.
/// - `path`: The child path checked for the scripted `nested` failure.
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
///
/// # Parameters
///
/// - `name`: The platform-native name whose storage size is measured.
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

/// Charges entry-count and native-name budgets for one new, depth-eligible
/// name.
///
/// The entry allowance is checked first so a full processing budget takes
/// precedence over a later name-byte or metadata error.
///
/// # Parameters
///
/// - `entry_budget`: Optional cumulative count of processed named entries.
/// - `seen_name_budget`: Optional cumulative storage allowance for names.
/// - `name`: The native name being admitted.
/// - `error_path`: Path used for any resource-limit diagnostic.
///
/// # Errors
///
/// Returns the structured resource-limit error when either allowance is
/// exhausted.
pub(super) fn consume_entry_budgets(
    entry_budget: &mut Option<ResourceBudget<LocalResourceKind, usize>>,
    seen_name_budget: &mut Option<ResourceBudget<LocalResourceKind, usize>>,
    name: &OsStr,
    error_path: &Path,
) -> LocalResult<()> {
    if let Some(budget) = entry_budget.as_mut() {
        budget
            .try_consume(1)
            .map_err(|error| directory_limit_error(error_path, error))?;
    }
    if let Some(budget) = seen_name_budget.as_mut() {
        budget
            .try_consume(name_bytes(name))
            .map_err(|error| directory_limit_error(error_path, error))?;
    }
    Ok(())
}

/// Reports whether an iterator error must terminate global traversal state.
///
/// # Parameters
///
/// - `error`: The traversal error to classify.
/// - `policy`: The configured policy for handling traversal errors.
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
