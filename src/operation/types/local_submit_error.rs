// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use super::LocalSubmitRejection;
/// Preserves the rejected request so the caller can recover its ownership.
/// # Type Parameters
/// - `R` — owned request returned after admission is rejected.
pub struct LocalSubmitError<R> {
    /// Stores the request associated with this value.
    request: R,
    /// Stores the rejection associated with this value.
    rejection: LocalSubmitRejection,
}

impl<R> LocalSubmitError<R> {
    /// Runs the new behavior while preserving the operation lifecycle
    /// invariants.
    /// # Parameters
    /// - `rejection` — supplies data or policy for this operation.
    #[allow(dead_code)]
    pub(crate) fn new(request: R, rejection: LocalSubmitRejection) -> Self {
        Self { request, rejection }
    }
    /// Returns the original request without requiring it to implement Clone or
    /// Debug.
    pub fn into_request(self) -> R {
        self.request
    }
    /// Returns the admission reason.
    pub fn rejection(&self) -> LocalSubmitRejection {
        self.rejection
    }
}

impl<R> std::fmt::Debug for LocalSubmitError<R> {
    /// Runs the fmt behavior while preserving the operation lifecycle
    /// invariants.
    /// # Parameters
    /// - `formatter` — supplies data or policy for this operation.
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("LocalSubmitError")
            .field("rejection", &self.rejection)
            .finish_non_exhaustive()
    }
}

impl<R> std::fmt::Display for LocalSubmitError<R> {
    /// Runs the fmt behavior while preserving the operation lifecycle
    /// invariants.
    /// # Parameters
    /// - `formatter` — supplies data or policy for this operation.
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "local operation rejected: {:?}", self.rejection)
    }
}

impl<R: 'static> std::error::Error for LocalSubmitError<R> {}

#[cfg(test)]
mod tests {
    use std::error::Error;
    use std::path::PathBuf;

    use super::super::LocalReadRequest;
    use super::LocalSubmitError;
    use super::LocalSubmitRejection;
    use crate::options::LocalReadAllOptions;

    #[test]
    fn test_rejection_formats_without_debugging_or_losing_the_request() {
        struct OpaqueRequest {
            identity: Box<()>,
        }

        let request = OpaqueRequest { identity: Box::new(()) };
        let identity = &*request.identity as *const ();
        let error = LocalSubmitError::new(request, LocalSubmitRejection::CapacityExceeded);

        assert_eq!(error.rejection(), LocalSubmitRejection::CapacityExceeded);
        assert_eq!(error.to_string(), "local operation rejected: CapacityExceeded");
        assert_eq!(
            format!("{error:?}"),
            "LocalSubmitError { rejection: CapacityExceeded, .. }"
        );
        assert!(Error::source(&error).is_none());
        let request = error.into_request();
        assert_eq!(&*request.identity as *const (), identity);
    }

    #[test]
    fn test_concrete_read_request_survives_error_conversion_and_recovery() {
        let request = LocalReadRequest {
            path: PathBuf::from("data/input.bin"),
            read: LocalReadAllOptions::new().with_max_bytes(64),
        };
        let error = LocalSubmitError::new(request, LocalSubmitRejection::WrongManager);
        let diagnostic = error.to_string();
        let boxed: Box<dyn Error> = Box::new(error);

        assert_eq!(diagnostic, "local operation rejected: WrongManager");
        let error = boxed
            .downcast::<LocalSubmitError<LocalReadRequest>>()
            .expect("the concrete submit error remains recoverable");
        assert_eq!(error.rejection(), LocalSubmitRejection::WrongManager);
        assert_eq!(error.into_request().path, PathBuf::from("data/input.bin"));
    }

    #[test]
    fn test_each_rejection_has_a_stable_display_diagnostic() {
        let cases = [
            (LocalSubmitRejection::Closed, "Closed"),
            (LocalSubmitRejection::CapacityExceeded, "CapacityExceeded"),
            (LocalSubmitRejection::WrongManager, "WrongManager"),
            (LocalSubmitRejection::ScopeEnded, "ScopeEnded"),
            (LocalSubmitRejection::Stopped, "Stopped"),
            (LocalSubmitRejection::AlreadyBound, "AlreadyBound"),
            (LocalSubmitRejection::AuthorityNotCaptured, "AuthorityNotCaptured"),
        ];

        for (rejection, name) in cases {
            let error = LocalSubmitError::new(PathBuf::from("requested/path"), rejection);
            assert_eq!(error.rejection(), rejection);
            assert_eq!(error.to_string(), format!("local operation rejected: {name}"));
            assert_eq!(error.into_request(), PathBuf::from("requested/path"));
        }
    }
}
