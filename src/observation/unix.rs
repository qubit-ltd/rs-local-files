// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
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
            #[allow(clippy::useless_conversion)]
            let nanos_valid = u64::try_from(nanos).is_ok_and(valid_nanos);
            #[allow(clippy::useless_conversion)]
            let changed_nanos_valid = u64::try_from(changed_nanos).is_ok_and(valid_nanos);
            if nanos_valid && changed_nanos_valid {
                // Some Unix targets expose nanoseconds as unsigned integers.
                // Keep these casts for those targets while avoiding a Clippy
                // error where the native type is already `i64`.
                #[allow(clippy::unnecessary_cast)]
                let nanos = nanos as i64;
                #[allow(clippy::unnecessary_cast)]
                let changed_nanos = changed_nanos as i64;
                (
                    identity,
                    Some(LocalNativeFileDetails::from_unix(LocalUnixFileDetails::from_parts(
                        device,
                        inode,
                        seconds,
                        nanos,
                        changed_seconds,
                        changed_nanos,
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
