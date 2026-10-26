// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Error combining a native conditional operation and its receipt observation.

use std::fmt;
use std::io;

#[derive(Debug)]
pub(super) struct NativeReceiptError {
    /// Native publication failure preserved as the primary source.
    pub(super) native: io::Error,
    /// Failure to observe the namespace receipt after the native operation.
    pub(super) observation: io::Error,
}

impl fmt::Display for NativeReceiptError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{}; receipt observation failed: {}",
            self.native, self.observation
        )
    }
}

impl std::error::Error for NativeReceiptError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.native)
    }
}

impl NativeReceiptError {
    #[allow(dead_code)]
    fn observation_source(&self) -> &(dyn std::error::Error + 'static) {
        &self.observation
    }
}
// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.

//    SPDX-License-Identifier: Apache-2.0

//    Licensed under the Apache License, Version 2.0.
// =============================================================================
