// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Rooted atomic identity normalization for externally timed races.
// qubit-style: allow source-test-pair
// Public fixtures cannot deterministically replace a rooted destination between
// its commit-time handle open and the immediately following identity check.

use std::ffi::CString;
use std::fs::File;
use std::io::{
    Error,
    ErrorKind,
};

use crate::{
    LocalAtomicDestinationState,
    LocalAtomicWriteError,
    LocalAtomicWriteStage,
};

use super::opened_atomic_destination::{
    OpenedAtomicDestination,
    rooted_destination_identity_matches,
};
use super::rooted_atomic_write::inspect_rooted_atomic_destination;
use super::rooted_staged_file::RootedStagedFile;

/// Verifies that a rooted entry still names its opened destination.
///
/// # Parameters
///
/// * `name` - Destination entry name relative to `parent`.
/// * `destination` - Commit-time destination handle and identity.
/// * `requested_path` - Relative destination retained for diagnostics.
/// * `staged_file` - Armed staging file whose parent is authoritative.
///
/// # Errors
///
/// Returns the native inspection error or an `InvalidInput` identity-change
/// error paired with the most precise safe destination state.
pub(in crate::local) fn verify_rooted_atomic_destination_identity(
    name: &CString,
    destination: &OpenedAtomicDestination,
    requested_path: &std::path::Path,
    staged_file: &mut RootedStagedFile,
) -> Result<(), LocalAtomicWriteError> {
    match rooted_destination_identity_matches(
        staged_file.parent(),
        name,
        destination,
    ) {
        Ok(true) => Ok(()),
        Ok(false) => Err(identity_error(
            requested_path,
            Error::new(
                ErrorKind::InvalidInput,
                "rooted atomic destination changed before replacement",
            ),
            rooted_mismatch_state(staged_file.parent(), name),
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

/// Builds a structured rooted identity failure with staging recovery.
fn identity_error(
    requested_path: &std::path::Path,
    source: Error,
    destination_state: LocalAtomicDestinationState,
    staged_file: &mut RootedStagedFile,
) -> LocalAtomicWriteError {
    let temporary_path = staged_file.diagnostic_path().to_path_buf();
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

/// Classifies a rooted identity mismatch before replacement.
fn rooted_mismatch_state(
    parent: &File,
    name: &CString,
) -> LocalAtomicDestinationState {
    match inspect_rooted_atomic_destination(parent, name) {
        Ok(false) => LocalAtomicDestinationState::Missing,
        _ => LocalAtomicDestinationState::Unchanged,
    }
}
