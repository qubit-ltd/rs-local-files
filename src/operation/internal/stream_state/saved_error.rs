// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::io;

/// Stores an I/O error in cloneable form for repeated polling.
/// Reconstructing the error preserves its kind and message without sharing a
/// non-cloneable `io::Error` value.
#[derive(Clone)]
pub(super) struct SavedError {
    /// Original error category used when reconstructing the I/O failure.
    kind: io::ErrorKind,
    /// Owned diagnostic text retained across asynchronous polls.
    message: String,
}

impl SavedError {
    /// Stores the stable, cloneable parts of a native I/O error.
    ///
    /// # Parameters
    ///
    /// * `error` — native failure whose kind and message must survive polling.
    pub(super) fn capture(error: io::Error) -> Self {
        Self {
            kind: error.kind(),
            message: error.to_string(),
        }
    }

    /// Recreates an equivalent I/O error for the current poll result.
    ///
    /// # Parameters
    ///
    /// * `self` — stored error details used to create a fresh I/O error.
    pub(super) fn make(&self) -> io::Error {
        io::Error::new(self.kind, self.message.clone())
    }
}
