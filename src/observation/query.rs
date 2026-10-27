//! Platform dispatch for same-handle observations.

use std::fs::File;

use crate::LocalResult;
use crate::observation::{LocalFileObservation, LocalObservationRequirement};

pub(crate) fn query_observation(
    file: &File,
    requirement: LocalObservationRequirement,
) -> LocalResult<LocalFileObservation> {
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
        use crate::error::{LocalFileError, LocalFileErrorKind, LocalFileOperation};

        if requirement == LocalObservationRequirement::Required {
            Err(LocalFileError::new(
                LocalFileErrorKind::RequirementNotMet,
                LocalFileOperation::Metadata,
            ))
        } else {
            let metadata = file.metadata().map_err(|error| {
                LocalFileError::from_io(LocalFileOperation::Metadata, None, None, error)
            })?;
            Ok(LocalFileObservation::from_parts(
                LocalFileMetadata::from_native(&metadata),
                None,
                None,
            ))
        }
    }
}
