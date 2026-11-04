// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Platform dispatch for same-handle observations.

use std::fs::File;

use crate::LocalResult;
use crate::observation::LocalFileObservation;
use crate::observation::LocalObservationRequirement;

/// Queries metadata and available identity and timestamp facts from one handle.
///
/// On Unix, normalized metadata comes from the handle and `fstat` supplies
/// identity and timestamp details. On Windows, normalized metadata comes from
/// the handle and native handle information supplies identity and timestamps.
/// On other targets, only normalized metadata is available; `Required` therefore
/// fails with `RequirementNotMet`, while `Optional` returns an observation
/// without native identity or timestamp details.
///
/// Under `Optional`, unavailable native identity/timestamp facts may be omitted.
/// Errors from attempted native queries are returned as I/O errors unless the
/// platform explicitly classifies the error as unsupported. Failure to obtain
/// normalized metadata is always returned. `Required` returns an error when
/// required native facts cannot be queried or are unavailable.
///
/// # Parameters
///
/// - `file`: the open file handle to query without reopening its path.
/// - `requirement`: whether native identity and timestamp facts are required.
///
/// # Returns
///
/// An observation containing normalized metadata and the native facts available
/// under the selected requirement and platform.
///
/// # Errors
///
/// Returns an I/O error if normalized metadata cannot be read. Under
/// `Optional`, also returns an I/O error when a native query fails for a reason
/// other than explicit lack of support. Returns an error if required native
/// facts cannot be queried or are unavailable.
pub(crate) fn query_observation(
    file: &File,
    requirement: LocalObservationRequirement,
) -> LocalResult<LocalFileObservation> {
    #[cfg(test)]
    if let Err(source) =
        crate::tests::native_io_fault_tests::check(crate::tests::native_io_fault_tests::NativeIoPoint::Observation)
    {
        return Err(crate::error::LocalFileError::from_io(
            crate::error::LocalFileOperation::Metadata,
            None,
            None,
            source,
        ));
    }
    #[cfg(unix)]
    {
        crate::observation::unix::query_native(file, requirement)
    }
    #[cfg(windows)]
    {
        crate::observation::windows::query_native(file, requirement)
    }
    #[cfg(not(any(unix, windows)))]
    {
        use crate::LocalFileMetadata;
        use crate::error::LocalFileError;
        use crate::error::LocalFileErrorKind;
        use crate::error::LocalFileOperation;

        if requirement == LocalObservationRequirement::Required {
            Err(LocalFileError::new(
                LocalFileErrorKind::RequirementNotMet,
                LocalFileOperation::Metadata,
            ))
        } else {
            let metadata = file
                .metadata()
                .map_err(|error| LocalFileError::from_io(LocalFileOperation::Metadata, None, None, error))?;
            Ok(LocalFileObservation::from_parts(
                LocalFileMetadata::from_native(&metadata),
                None,
                None,
            ))
        }
    }
}
