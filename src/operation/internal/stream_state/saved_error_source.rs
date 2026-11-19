// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::error::Error;
use std::fmt;
use std::io;
use std::sync::Arc;

/// Error payload retaining a primary I/O failure and optional diagnostics.
pub(super) struct SavedErrorSource {
    /// Primary error whose source chain remains inspectable.
    pub(super) primary: Arc<io::Error>,
    /// Optional context appended to the primary error's display.
    pub(super) context: Option<String>,
    /// Optional earlier write failure displayed after the primary failure.
    pub(super) secondary: Option<Arc<io::Error>>,
}

impl fmt::Display for SavedErrorSource {
    /// Formats the primary error and retained diagnostic context.
    ///
    /// # Parameters
    ///
    /// * `self` — Payload containing the retained error diagnostics.
    /// * `formatter` — Formatter receiving the rendered diagnostic.
    ///
    /// # Returns
    ///
    /// The result of writing the diagnostic to the formatter.
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}", self.primary)?;
        if let Some(context) = &self.context {
            write!(formatter, " ({context})")?;
        }
        if let Some(secondary) = &self.secondary {
            write!(formatter, "; earlier write failed: {secondary}")?;
        }
        Ok(())
    }
}

impl fmt::Debug for SavedErrorSource {
    /// Formats the retained error details for debugging.
    ///
    /// # Parameters
    ///
    /// * `self` — Payload containing the retained error diagnostics.
    /// * `formatter` — Formatter receiving the debug representation.
    ///
    /// # Returns
    ///
    /// The result of writing the debug representation to the formatter.
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("SavedErrorSource")
            .field("primary", &self.primary)
            .field("context", &self.context)
            .field("secondary", &self.secondary)
            .finish()
    }
}

impl Error for SavedErrorSource {
    /// Exposes the primary error as the next standard source.
    ///
    /// # Parameters
    ///
    /// * `self` — Payload whose primary error remains the main cause.
    ///
    /// # Returns
    ///
    /// The retained primary error.
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        Some(self.primary.as_ref())
    }
}
