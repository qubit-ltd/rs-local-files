// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::path::Path;
use std::time::Duration;
use std::time::Instant;

use crate::LocalResult;
use crate::error::LocalFileError;
use crate::error::LocalFileErrorKind;
use crate::error::LocalFileOperation;

/// Converts a configured path-binding duration without silently dropping
/// overflow.
///
/// Returns `Ok(None)` when no duration is configured. An unrepresentable
/// duration returns `InvalidOptions` and retains the supplied operation and
/// optional path in the error context.
///
/// # Parameters
///
/// - `started_at`: Monotonic instant from which the duration is measured.
/// - `duration`: Optional maximum duration; `None` leaves the deadline unset.
/// - `operation`: Operation recorded if the deadline cannot be represented.
/// - `path`: Optional path recorded with an overflow error.
///
/// # Returns
///
/// Returns `Ok(None)` when `duration` is `None`, or `Ok(Some(deadline))` when
/// the duration can be added to `started_at`.
///
/// # Errors
///
/// Returns an `InvalidOptions` error when the monotonic clock cannot represent
/// the resulting deadline. The error retains `operation` and `path`.
pub(crate) fn checked_path_binding_deadline(
    started_at: Instant,
    duration: Option<Duration>,
    operation: LocalFileOperation,
    path: Option<&Path>,
) -> LocalResult<Option<Instant>> {
    duration
        .map(|duration| {
            started_at.checked_add(duration).ok_or_else(|| {
                let error = LocalFileError::new(LocalFileErrorKind::InvalidOptions, operation)
                    .with_reason("path binding deadline exceeds the monotonic clock range");
                match path {
                    Some(path) => error.with_path(path.to_path_buf()),
                    None => error,
                }
            })
        })
        .transpose()
}
