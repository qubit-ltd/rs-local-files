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
pub(super) struct DestinationRemovalFailure(io::Error);

impl Display for DestinationRemovalFailure {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> std::fmt::Result {
        Display::fmt(&self.0, formatter)
    }
}

impl StdError for DestinationRemovalFailure {
    fn source(&self) -> Option<&(dyn StdError + 'static)> {
        Some(&self.0)
    }
}

pub(super) fn mark_attempted(error: io::Error) -> io::Error {
    let kind = error.kind();
    io::Error::new(kind, DestinationRemovalFailure(error))
}

pub(super) fn was_attempted(error: &io::Error) -> bool {
    error
        .get_ref()
        .and_then(|source| source.downcast_ref::<DestinationRemovalFailure>())
        .is_some()
}

#[cfg(test)]
mod tests {
    use std::error::Error;
    use std::io;

    use super::mark_attempted;
    use super::was_attempted;

    #[test]
    fn attempted_removal_marker_preserves_error_and_is_detectable() {
        let original = io::Error::new(io::ErrorKind::PermissionDenied, "remove denied");
        let marked = mark_attempted(original);

        assert_eq!(marked.kind(), io::ErrorKind::PermissionDenied);
        assert_eq!(marked.to_string(), "remove denied");
        assert!(was_attempted(&marked));
        assert!(!was_attempted(&io::Error::other("unmarked")));
        assert_eq!(marked.source().unwrap().to_string(), "remove denied");
    }
}
