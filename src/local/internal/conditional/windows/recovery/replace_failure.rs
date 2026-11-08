// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Classification of partial failures reported by Windows `ReplaceFileW`.

use windows_sys::Win32::Foundation::ERROR_UNABLE_TO_MOVE_REPLACEMENT;
use windows_sys::Win32::Foundation::ERROR_UNABLE_TO_MOVE_REPLACEMENT_2;
use windows_sys::Win32::Foundation::ERROR_UNABLE_TO_REMOVE_REPLACED;

/// Identifies how far a native `ReplaceFileW` operation progressed before
/// failing.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::local::internal::conditional::windows) enum ReplaceFailure {
    /// Windows could not remove the file that was being replaced.
    UnableToRemoveReplaced,
    /// Windows could not move the replacement file into place.
    UnableToMoveReplacement,
    /// Windows could not move the replacement after creating its backup.
    UnableToMoveReplacementAfterBackup,
    /// The error code does not identify one of the documented partial failures.
    Other,
}

/// Maps a native OS error code to the corresponding `ReplaceFileW`
/// partial-failure class.
///
/// A missing or unrecognized code is classified as [`ReplaceFailure::Other`].
///
/// # Parameters
///
/// - `raw_os_error`: The native OS error code, or `None` when no code is
///   available.
///
/// # Returns
///
/// The `ReplaceFileW` partial-failure class associated with the code, or
/// [`ReplaceFailure::Other`] when the code is missing or unrecognized.
pub(in crate::local::internal::conditional::windows) fn classify_replace_failure(
    raw_os_error: Option<i32>,
) -> ReplaceFailure {
    match raw_os_error.map(|code| code as u32) {
        Some(ERROR_UNABLE_TO_REMOVE_REPLACED) => ReplaceFailure::UnableToRemoveReplaced,
        Some(ERROR_UNABLE_TO_MOVE_REPLACEMENT) => ReplaceFailure::UnableToMoveReplacement,
        Some(ERROR_UNABLE_TO_MOVE_REPLACEMENT_2) => ReplaceFailure::UnableToMoveReplacementAfterBackup,
        _ => ReplaceFailure::Other,
    }
}
