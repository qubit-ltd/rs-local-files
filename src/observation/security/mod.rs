// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::fs::File;

use crate::LocalResult;
use crate::observation::LocalSecurityObservation;
use crate::observation::LocalSecurityObservationOptions;

/// Collects optional filesystem, access, and link-count facts for an open file.
///
/// Platform-specific queries borrow the same file handle, so the observation
/// cannot silently switch to a different file if its path is replaced. Facts
/// marked as required by `options` cause an error when the platform cannot
/// provide them.
///
/// # Parameters
///
/// - `file` is the already-open file whose metadata is observed.
/// - `options` selects which observation facts are optional or required.
///
/// # Returns
///
/// Returns the facts available for the file. Unsupported optional facts are
/// represented as absent.
///
/// # Errors
///
/// Returns an error when a required fact cannot be obtained or a metadata
/// query fails.
pub(crate) fn query_security(
    file: &File,
    options: &LocalSecurityObservationOptions,
) -> LocalResult<LocalSecurityObservation> {
    query_security_with(file, options, |file, options| {
        #[cfg(unix)]
        {
            unix::query_security(file, options)
        }
        #[cfg(windows)]
        {
            windows::query_security(file, options)
        }
        #[cfg(not(any(unix, windows)))]
        {
            if options.filesystem() == crate::observation::LocalObservationRequirement::Required
                || options.access() == crate::observation::LocalObservationRequirement::Required
                || options.link_count() == crate::observation::LocalObservationRequirement::Required
            {
                return Err(crate::error::LocalFileError::new(
                    crate::error::LocalFileErrorKind::RequirementNotMet,
                    crate::error::LocalFileOperation::Metadata,
                ));
            }
            let _ = file;
            Ok(LocalSecurityObservation::new(None, None, None, None))
        }
    })
}

/// Runs a security query through an injected operation while retaining the
/// production function's file and options arguments.
///
/// This private seam lets the inline unit test verify error propagation and
/// that the query observes the original file handle without invoking native
/// security APIs.
///
/// # Parameters
///
/// - `file` and `options` are forwarded unchanged to `query`.
/// - `query` supplies the platform operation to run.
///
/// # Returns
///
/// Returns the injected operation's observation unchanged.
///
/// # Errors
///
/// Returns any error produced by the injected operation.
fn query_security_with(
    file: &File,
    options: &LocalSecurityObservationOptions,
    query: impl FnOnce(&File, &LocalSecurityObservationOptions) -> LocalResult<LocalSecurityObservation>,
) -> LocalResult<LocalSecurityObservation> {
    query(file, options)
}

#[cfg(test)]
mod tests {
    use super::query_security_with;
    use crate::error::LocalFileError;
    use crate::error::LocalFileErrorKind;
    use crate::error::LocalFileOperation;
    use crate::observation::LocalSecurityObservationOptions;

    #[test]
    fn test_injected_security_failures_preserve_kind_on_same_file_handle() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let path = directory.path().join("file");
        std::fs::write(&path, b"data").expect("create file");
        let file = std::fs::File::open(&path).expect("open file");
        for kind in [
            LocalFileErrorKind::PermissionDenied,
            LocalFileErrorKind::DataCorruption,
            LocalFileErrorKind::ChangedDuringRead,
        ] {
            let error = query_security_with(&file, &LocalSecurityObservationOptions::new(), |queried_file, _| {
                assert!(std::ptr::eq(queried_file, &file));
                Err(LocalFileError::new(kind, LocalFileOperation::Metadata))
            })
            .expect_err("injected failure should propagate");
            assert_eq!(error.kind(), kind);
        }
    }
}
#[cfg(unix)]
pub(crate) mod unix;
#[cfg(windows)]
pub(crate) mod windows;
