// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Descriptor-relative atomic installation result normalization.
// qubit-style: allow source-test-pair
// Replacement failures after a validated live destination require externally
// timed namespace or mount failures that public fixtures cannot force.

use std::ffi::CString;
use std::io::Error;

use crate::LocalAtomicDestinationState;

use super::atomic_file_install::replacement_error_state;
use super::rooted_staged_file::RootedStagedFile;

/// Installs a rooted staging file according to its initial destination state.
///
/// # Parameters
///
/// * `staged_file` - Armed descriptor-relative staging file.
/// * `destination` - Final entry name relative to the staging parent.
/// * `destination_existed` - Whether a regular destination existed initially.
///
/// # Errors
///
/// Returns the native installation error paired with the most precise safe
/// destination state.
pub(in crate::local) fn install_rooted_atomic_file(
    staged_file: &mut RootedStagedFile,
    destination: &CString,
    destination_existed: bool,
) -> Result<(), (Error, LocalAtomicDestinationState)> {
    if destination_existed {
        match staged_file.rename_to(destination) {
            Ok(()) => Ok(()),
            Err(source) => {
                let destination_state = replacement_error_state(&source);
                Err((source, destination_state))
            }
        }
    } else {
        staged_file.install_new_to(destination)
    }
}
