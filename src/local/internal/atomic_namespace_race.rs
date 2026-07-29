// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Atomic destination identity normalization for externally timed races.
// qubit-style: allow source-test-pair
// qubit-style: allow coverage-cfg
// Public fixtures cannot deterministically replace a destination between its
// commit-time handle open and the immediately following identity check.

use std::fs;
use std::io::{
    Error,
    ErrorKind,
};
use std::path::Path;

use crate::{
    LocalAtomicDestinationState,
    LocalAtomicWriteError,
    LocalAtomicWriteStage,
};

use super::opened_atomic_destination::{
    OpenedAtomicDestination,
    destination_identity_matches,
};

/// Verifies that a path still names its opened atomic destination.
///
/// # Parameters
///
/// * `operation_path` - Destination path inspected immediately before
///   replacement.
/// * `destination` - Commit-time destination handle and identity.
/// * `requested_path` - Caller-supplied destination retained for diagnostics.
/// * `temporary_path` - Staging path retained for recovery diagnostics.
///
/// # Errors
///
/// Returns the native inspection error or an `InvalidInput` identity-change
/// error paired with the most precise safe destination state.
pub(crate) fn verify_atomic_destination_identity(
    operation_path: &Path,
    destination: &OpenedAtomicDestination,
    requested_path: &Path,
    temporary_path: &Path,
) -> Result<(), LocalAtomicWriteError> {
    match destination_identity_matches(operation_path, destination) {
        Ok(true) => Ok(()),
        Ok(false) => Err(identity_error(
            requested_path,
            Error::new(
                ErrorKind::InvalidInput,
                "atomic write destination changed before replacement",
            ),
            destination_mismatch_state(operation_path),
            temporary_path,
        )),
        Err(error) => Err(identity_error(
            requested_path,
            error,
            LocalAtomicDestinationState::Unchanged,
            temporary_path,
        )),
    }
}

/// Builds a structured identity failure while retaining staging for recovery.
///
/// # Parameters
///
/// * `requested_path` - Caller-supplied destination retained for diagnostics.
/// * `source` - Native identity inspection failure.
/// * `destination_state` - Known destination state after the failed check.
/// * `temporary_path` - Staging path retained for recovery diagnostics.
///
/// # Returns
///
/// A structured pre-installation failure.
fn identity_error(
    requested_path: &Path,
    source: Error,
    destination_state: LocalAtomicDestinationState,
    temporary_path: &Path,
) -> LocalAtomicWriteError {
    LocalAtomicWriteError::new(
        LocalAtomicWriteStage::ReplaceDestination,
        requested_path.to_path_buf(),
        Some(temporary_path.to_path_buf()),
        destination_state,
        source,
    )
}

/// Classifies a pre-replacement destination identity mismatch.
fn destination_mismatch_state(path: &Path) -> LocalAtomicDestinationState {
    #[cfg(coverage)]
    let metadata =
        if super::coverage_fault::is_enabled("atomic-identity-missing") {
            Err(Error::from_raw_os_error(libc::ENOENT))
        } else {
            fs::symlink_metadata(path)
        };
    #[cfg(not(coverage))]
    let metadata = fs::symlink_metadata(path);
    match metadata {
        Err(error) if error.kind() == ErrorKind::NotFound => {
            LocalAtomicDestinationState::Missing
        }
        _ => LocalAtomicDestinationState::Unchanged,
    }
}
