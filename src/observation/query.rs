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
