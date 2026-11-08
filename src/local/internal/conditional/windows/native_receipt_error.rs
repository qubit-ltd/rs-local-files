// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Error combining a native conditional operation and its receipt observation.

use std::error::Error;
use std::fmt;
use std::io;

/// Preserves a native publication failure alongside a failed receipt lookup.
///
/// The native error remains the primary cause so callers observe its original
/// error kind, while the observation error is retained for diagnostics.
#[derive(Debug)]
pub(super) struct NativeReceiptError {
    /// Native publication failure preserved as the primary source.
    pub(super) native: io::Error,
    /// Failure to observe the namespace receipt after the native operation.
    pub(super) observation: io::Error,
}

impl fmt::Display for NativeReceiptError {
    /// Formats both failures while keeping the native failure first.
    ///
    /// # Parameters
    ///
    /// * `formatter` - The destination that receives the combined error text.
    ///
    /// # Returns
    ///
    /// Returns `Ok(())` when formatting succeeds.
    ///
    /// # Errors
    ///
    /// Returns the formatter's error if writing either failure fails.
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{}; receipt observation failed: {}",
            self.native, self.observation
        )
    }
}

impl Error for NativeReceiptError {
    /// Returns the native publication failure as the causal source.
    ///
    /// The receipt observation failure remains available separately for
    /// diagnostics and does not replace the native operation's cause.
    ///
    /// # Returns
    ///
    /// Always returns `Some` containing the native publication failure.
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        Some(&self.native)
    }
}

impl NativeReceiptError {
    /// Returns the receipt observation failure for diagnostic inspection.
    ///
    /// The borrowed error is owned by this wrapper and remains valid only for
    /// the lifetime of the borrow.
    #[must_use]
    #[inline]
    #[allow(dead_code)]
    fn observation_source(&self) -> &(dyn Error + 'static) {
        &self.observation
    }
}
// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.

//    SPDX-License-Identifier: Apache-2.0

//    Licensed under the Apache License, Version 2.0.
// =============================================================================
