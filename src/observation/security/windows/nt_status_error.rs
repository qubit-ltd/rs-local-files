// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::fmt;

/// Carries an NTSTATUS value through the standard I/O error interface.
#[derive(Debug)]
pub(super) struct NtStatusError(
    /// The raw status returned by the Windows native API.
    pub(super) i32,
);

impl fmt::Display for NtStatusError {
    /// Formats the status as an unsigned, zero-padded hexadecimal value.
    ///
    /// # Parameters
    ///
    /// * `formatter` receives the formatted status.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` when formatting succeeds, or the formatter's error.
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "NtQuerySecurityObject returned NTSTATUS {:#010x}",
            self.0 as u32
        )
    }
}

impl std::error::Error for NtStatusError {}
