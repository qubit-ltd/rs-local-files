// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Atomic destination identity normalization for externally timed races.
// qubit-style: allow source-test-pair
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
use super::staged_file::StagedFile;

/// Verifies that a path still names its opened atomic destination.
///
/// # Parameters
///
/// * `path` - Destination path inspected immediately before replacement.
/// * `destination` - Commit-time destination handle and identity.
///
/// # Errors
///
/// Returns the native inspection error or an `InvalidInput` identity-change
/// error paired with the most precise safe destination state.
pub(crate) fn verify_atomic_destination_identity(
    operation_path: &Path,
    destination: &OpenedAtomicDestination,
    requested_path: &Path,
    staged_file: &mut StagedFile,
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
            staged_file,
        )),
        Err(error) => Err(identity_error(
            requested_path,
            error,
            LocalAtomicDestinationState::Unchanged,
            staged_file,
        )),
    }
}

/// Builds a structured identity failure with state-aware staging recovery.
fn identity_error(
    requested_path: &Path,
    source: Error,
    destination_state: LocalAtomicDestinationState,
    staged_file: &mut StagedFile,
) -> LocalAtomicWriteError {
    let temporary_path = staged_file.path().to_path_buf();
    let cleanup_error =
        if destination_state == LocalAtomicDestinationState::Unchanged {
            staged_file.cleanup().err()
        } else {
            staged_file.close();
            staged_file.disarm();
            None
        };
    LocalAtomicWriteError::new(
        LocalAtomicWriteStage::ReplaceDestination,
        requested_path.to_path_buf(),
        Some(temporary_path),
        destination_state,
        source,
    )
    .with_cleanup_error(cleanup_error)
}

/// Classifies a pre-replacement destination identity mismatch.
fn destination_mismatch_state(path: &Path) -> LocalAtomicDestinationState {
    match fs::symlink_metadata(path) {
        Err(error) if error.kind() == ErrorKind::NotFound => {
            LocalAtomicDestinationState::Missing
        }
        _ => LocalAtomicDestinationState::Unchanged,
    }
}
