// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::fmt;
use std::io;

/// Describes a directory-walk batch that exceeds the bounded memory budget.
/// The error is exposed through `io::Error` so callers can inspect its cause.
#[derive(Debug)]
pub(super) struct WalkerResourceLimit {
    /// Explanation of the directory-walk budget that was exceeded.
    message: &'static str,
}

/// Creates an out-of-memory I/O error for an over-budget directory batch.
///
/// # Parameters
///
/// * `message` — detail describing which batch limit was exceeded.
pub(super) fn resource_limit(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::OutOfMemory, WalkerResourceLimit { message })
}

impl std::fmt::Display for WalkerResourceLimit {
    /// Formats the over-budget batch reason for I/O diagnostics.
    ///
    /// # Parameters
    ///
    /// * `self` — limit error carrying the diagnostic text.
    /// * `formatter` — destination formatter supplied by the caller.
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.message)
    }
}

impl std::error::Error for WalkerResourceLimit {}
