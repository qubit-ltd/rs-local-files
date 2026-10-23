// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Native ReplaceFileW failure classes used alongside observed receipts.

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum ReplaceFailure {
    UnableToRemoveReplaced,
    UnableToMoveReplacement,
    UnableToMoveReplacementAfterBackup,
    Other,
}

pub(super) fn classify_replace_failure(raw_os_error: Option<i32>) -> ReplaceFailure {
    match raw_os_error.map(|code| code as u32) {
        Some(windows_sys::Win32::Foundation::ERROR_UNABLE_TO_REMOVE_REPLACED) => ReplaceFailure::UnableToRemoveReplaced,
        Some(windows_sys::Win32::Foundation::ERROR_UNABLE_TO_MOVE_REPLACEMENT) => {
            ReplaceFailure::UnableToMoveReplacement
        }
        Some(windows_sys::Win32::Foundation::ERROR_UNABLE_TO_MOVE_REPLACEMENT_2) => {
            ReplaceFailure::UnableToMoveReplacementAfterBackup
        }
        _ => ReplaceFailure::Other,
    }
}
