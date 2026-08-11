// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Shared recovery state machine for failed atomic installations.
// qubit-style: allow source-test-pair
// Private behavior is covered through public integration tests.

use std::io;
use std::path::Path;
use std::path::PathBuf;

use super::AtomicStagingState;
use crate::LocalAtomicDestinationState;
use crate::LocalAtomicWriteError;
use crate::LocalAtomicWriteStage;

/// Context retained while recovering a failed atomic installation.
pub(crate) struct AtomicInstallRecovery<'a> {
    /// Requested destination retained for diagnostics.
    pub(crate) path: &'a Path,
    /// Staging path retained for diagnostics.
    pub(crate) temporary_path: PathBuf,
    /// Primary native installation error.
    pub(crate) source: io::Error,
    /// Known destination state after installation.
    pub(crate) destination_state: LocalAtomicDestinationState,
    /// Known staging-name state after installation.
    pub(crate) staging_state: AtomicStagingState,
}

/// Recovers or reports a failed atomic destination installation.
///
/// The callbacks retain backend-specific authority: ordinary writers use
/// path-based cleanup and synchronization, while rooted writers use opened
/// directory descriptors. This function only owns the shared state machine.
pub(crate) fn recover_atomic_install_error<S>(
    context: AtomicInstallRecovery<'_>,
    staged_file: &mut S,
    cleanup_staging: impl FnOnce(&mut S) -> io::Result<()>,
    disarm_indeterminate_staging: impl FnOnce(&mut S),
    sync_parent: impl Fn(&S) -> io::Result<()>,
) -> Result<(), LocalAtomicWriteError> {
    let AtomicInstallRecovery {
        path,
        temporary_path,
        source,
        destination_state,
        staging_state,
    } = context;
    let cleanup_error = match staging_state {
        AtomicStagingState::Present => match cleanup_staging(staged_file) {
            Ok(()) if destination_state == LocalAtomicDestinationState::Replaced => {
                return sync_parent(staged_file).map_err(|error| {
                    LocalAtomicWriteError::new(
                        LocalAtomicWriteStage::SyncParent,
                        path.to_path_buf(),
                        Some(temporary_path),
                        destination_state,
                        error,
                    )
                });
            }
            Ok(()) => None,
            Err(error) => Some(error),
        },
        AtomicStagingState::Indeterminate => {
            disarm_indeterminate_staging(staged_file);
            None
        }
    };
    let parent_sync_error = if destination_state == LocalAtomicDestinationState::Replaced {
        sync_parent(staged_file).err()
    } else {
        None
    };
    Err(LocalAtomicWriteError::new(
        LocalAtomicWriteStage::ReplaceDestination,
        path.to_path_buf(),
        Some(temporary_path),
        destination_state,
        source,
    )
    .with_cleanup_error(cleanup_error)
    .with_parent_sync_error(parent_sync_error))
}
