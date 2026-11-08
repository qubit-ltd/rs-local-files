// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
use std::error::Error as StdError;
use std::fmt::Display;
use std::fmt::Formatter;
use std::io;

/// Records that recursive destination removal was invoked before failing.
#[derive(Debug)]
pub(super) struct DestinationRemovalFailure(
    /// The original I/O error retained as the source of the marked error.
    io::Error,
);

impl Display for DestinationRemovalFailure {
    /// Formats the original error without changing its message.
    ///
    /// # Parameters
    ///
    /// * `formatter` - The destination for the original error's formatted
    ///   message.
    ///
    /// # Errors
    ///
    /// Returns the formatter's error if writing the original message fails.
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        Display::fmt(&self.0, formatter)
    }
}

impl StdError for DestinationRemovalFailure {
    /// Exposes the original I/O error as the source in the error chain.
    ///
    /// # Returns
    ///
    /// Always returns `Some` with a reference to the original I/O error.
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        Some(&self.0)
    }
}

/// Marks an I/O error as occurring after recursive destination removal began.
///
/// # Parameters
///
/// * `error` - The I/O error produced by the removal attempt.
///
/// # Returns
///
/// An I/O error with the same kind and display message, whose source records
/// the original error and therefore identifies that removal was attempted.
#[must_use]
pub(super) fn mark_attempted(error: io::Error) -> io::Error {
    let kind = error.kind();
    io::Error::new(kind, DestinationRemovalFailure(error))
}

#[cfg(test)]
mod tests {
    use std::error::Error;
    use std::io;

    use super::mark_attempted;

    #[test]
    fn test_attempted_removal_marker_preserves_error() {
        let original = io::Error::new(io::ErrorKind::PermissionDenied, "remove denied");
        let marked = mark_attempted(original);

        assert_eq!(marked.kind(), io::ErrorKind::PermissionDenied);
        assert_eq!(marked.to_string(), "remove denied");
        assert_eq!(
            marked
                .source()
                .expect("marked error should retain the original source")
                .to_string(),
            "remove denied"
        );
    }
}
