// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::io;
use std::sync::Arc;

#[path = "saved_error_source.rs"]
mod source;

use source::SavedErrorSource;

/// Stores an I/O error in shared form so repeated polls retain its source.
#[derive(Clone)]
pub(super) struct SavedError {
    /// Original error category used when reconstructing the I/O failure.
    kind: io::ErrorKind,
    /// Original error object, including any structured source.
    error: Arc<io::Error>,
}

impl SavedError {
    /// Stores the original I/O error without formatting away its source.
    ///
    /// # Parameters
    ///
    /// * `error` — I/O failure to retain across repeated polls.
    pub(super) fn capture(error: io::Error) -> Self {
        Self {
            kind: error.kind(),
            error: Arc::new(error),
        }
    }

    /// Stores an error with additional write-progress context.
    ///
    /// # Parameters
    ///
    /// * `error` — Original I/O failure whose source must remain available.
    /// * `context` — Additional diagnostic text displayed with the failure.
    pub(super) fn capture_with_context(error: io::Error, context: impl Into<String>) -> Self {
        let kind = error.kind();
        let source = SavedErrorSource {
            primary: Arc::new(error),
            context: Some(context.into()),
            secondary: None,
        };
        Self {
            kind,
            error: Arc::new(io::Error::new(kind, source)),
        }
    }

    /// Retains the close failure as the primary source and the earlier write
    /// failure as an additional diagnostic.
    ///
    /// # Parameters
    ///
    /// * `primary` — Close failure that remains the main cause.
    /// * `secondary` — Previously saved write failure displayed as context.
    pub(super) fn capture_combined(primary: io::Error, secondary: &Self) -> Self {
        let kind = primary.kind();
        let source = SavedErrorSource {
            primary: Arc::new(primary),
            context: None,
            secondary: Some(Arc::clone(&secondary.error)),
        };
        Self {
            kind,
            error: Arc::new(io::Error::new(kind, source)),
        }
    }

    /// Recreates an I/O error that retains the original error as its payload.
    ///
    /// # Parameters
    ///
    /// * `self` — Saved error details used to create the current poll result.
    ///
    /// # Returns
    ///
    /// An I/O error with the saved category and inspectable payload.
    pub(super) fn make(&self) -> io::Error {
        if let Some(code) = self.error.raw_os_error() {
            return io::Error::from_raw_os_error(code);
        }
        io::Error::new(
            self.kind,
            SavedErrorSource {
                primary: Arc::clone(&self.error),
                context: None,
                secondary: None,
            },
        )
    }
}
