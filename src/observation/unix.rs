//! Same-handle Unix observation helpers.

use std::fs::File;
use std::io;

use rustix::fs::Stat;
use rustix::fs::fstat;

use crate::LocalFileMetadata;
use crate::error::LocalFileError;
use crate::error::LocalFileErrorKind;
use crate::error::LocalFileOperation;
use crate::observation::LocalFileIdentity;
use crate::observation::LocalFileObservation;
use crate::observation::LocalNativeFileDetails;
use crate::observation::LocalObservationRequirement;
use crate::observation::LocalUnixFileDetails;

/// Queries normalized metadata and native identity from the same open handle.
pub(crate) fn query_native(
    file: &File,
    requirement: LocalObservationRequirement,
) -> crate::LocalResult<LocalFileObservation> {
    query_native_with(file, requirement, |file| fstat(file).map_err(io::Error::from))
}

pub(crate) fn query_native_with<F>(
    file: &File,
    requirement: LocalObservationRequirement,
    native_query: F,
) -> crate::LocalResult<LocalFileObservation>
where
    F: FnOnce(&File) -> io::Result<Stat>,
{
    let metadata = file
        .metadata()
        .map_err(|error| LocalFileError::from_io(LocalFileOperation::Metadata, None, None, error))?;
    let normalized = LocalFileMetadata::from_native(&metadata);
    let native = native_query(file);
    let (identity, details) = match native {
        Ok(stat) => {
            let seconds = stat.st_mtime;
            let nanos = stat.st_mtime_nsec;
            let changed_seconds = stat.st_ctime;
            let changed_nanos = stat.st_ctime_nsec;
            #[allow(clippy::unnecessary_cast)]
            let device = stat.st_dev as u64;
            #[allow(clippy::unnecessary_cast)]
            let inode = stat.st_ino as u64;
            let identity = Some(LocalFileIdentity::unix(device, inode));
            if valid_nanos(nanos) && valid_nanos(changed_nanos) {
                (
                    identity,
                    Some(LocalNativeFileDetails::from_unix(LocalUnixFileDetails::from_parts(
                        device,
                        inode,
                        seconds,
                        nanos as i64,
                        changed_seconds,
                        changed_nanos as i64,
                    ))),
                )
            } else {
                (identity, None)
            }
        }
        Err(error) => {
            if requirement == LocalObservationRequirement::Required {
                return Err(LocalFileError::from_io(LocalFileOperation::Metadata, None, None, error));
            }
            (None, None)
        }
    };

    if requirement == LocalObservationRequirement::Required && (identity.is_none() || details.is_none()) {
        return Err(
            LocalFileError::new(LocalFileErrorKind::RequirementNotMet, LocalFileOperation::Metadata)
                .with_reason("required Unix identity or timestamp details are unavailable"),
        );
    }

    Ok(LocalFileObservation::from_parts(normalized, identity, details))
}

const fn valid_nanos(nanos: u64) -> bool {
    nanos < 1_000_000_000
}
