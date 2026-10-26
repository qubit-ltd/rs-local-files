// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Shared recovery state machine for failed atomic installations.
// Private behavior is covered through public integration tests.

use std::io;
use std::path::Path;
use std::path::PathBuf;

use super::AtomicStagingState;
use crate::LocalAtomicDestinationState;
use crate::LocalAtomicWriteError;
use crate::LocalAtomicWriteStage;
use crate::LocalDurabilityRequirement;

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
///
/// `S` is the backend's retained staging state. Known-present staging is
/// cleaned through `cleanup_staging`; indeterminate staging is disarmed
/// instead. Parent synchronization is attempted only when the destination is
/// known replaced.
///
/// # Parameters
///
/// * `context` - Installation error and the filesystem states known after the
///   failed installation.
/// * `durability` - Parent synchronization requirement for a published
///   destination.
/// * `staged_file` - Backend-owned staging state used by the recovery
///   callbacks.
/// * `cleanup_staging` - Removes a staging entry known to remain present.
/// * `disarm_indeterminate_staging` - Prevents automatic cleanup when the
///   staging entry's state cannot be determined.
/// * `sync_parent` - Synchronizes the destination's parent directory after a
///   known replacement.
///
/// # Returns
///
/// Returns whether the parent directory chain was synchronized when staging
/// cleanup recovered a known replacement. Otherwise returns the original
/// installation error with any cleanup or synchronization error attached.
///
/// # Errors
///
/// Retains the installation error and secondary cleanup/synchronization errors.
/// When publication already succeeded and staging cleanup recovers, a required
/// synchronization failure is reported at `SyncParent` with the destination
/// still known replaced. Preferred synchronization failures return `Ok(false)`.
pub(crate) fn recover_atomic_install_error<S>(
    context: AtomicInstallRecovery<'_>,
    staged_file: &mut S,
    durability: LocalDurabilityRequirement,
    cleanup_staging: impl FnOnce(&mut S) -> io::Result<()>,
    disarm_indeterminate_staging: impl FnOnce(&mut S),
    sync_parent: impl Fn(&S) -> io::Result<()>,
) -> Result<bool, LocalAtomicWriteError> {
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
                return match durability {
                    LocalDurabilityRequirement::NotRequired => Ok(false),
                    LocalDurabilityRequirement::Preferred => Ok(sync_parent(staged_file).is_ok()),
                    LocalDurabilityRequirement::Required => sync_parent(staged_file).map(|()| true).map_err(|error| {
                        LocalAtomicWriteError::new(
                            LocalAtomicWriteStage::SyncParent,
                            path.to_path_buf(),
                            Some(temporary_path),
                            destination_state,
                            error,
                        )
                    }),
                };
            }
            Ok(()) => None,
            Err(error) => Some(error),
        },
        AtomicStagingState::Indeterminate => {
            disarm_indeterminate_staging(staged_file);
            None
        }
    };
    let parent_sync_error = if destination_state == LocalAtomicDestinationState::Replaced
        && durability != LocalDurabilityRequirement::NotRequired
    {
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

#[cfg(test)]
mod tests {
    use std::cell::Cell;
    use std::io;
    use std::path::Path;
    use std::path::PathBuf;

    use super::AtomicInstallRecovery;
    use super::recover_atomic_install_error;
    use crate::LocalAtomicDestinationState;
    use crate::LocalAtomicWriteError;
    use crate::LocalAtomicWriteStage;
    use crate::LocalDurabilityRequirement;
    use crate::local::internal::AtomicStagingState;

    fn run_recovery(
        durability: LocalDurabilityRequirement,
        cleanup_succeeds: bool,
        staging_state: AtomicStagingState,
        sync_succeeds: bool,
    ) -> (Result<bool, LocalAtomicWriteError>, usize, usize) {
        let cleanup_calls = Cell::new(0);
        let sync_calls = Cell::new(0);
        let mut staged = ();
        let result = recover_atomic_install_error(
            AtomicInstallRecovery {
                path: Path::new("destination"),
                temporary_path: PathBuf::from("staging"),
                source: io::Error::other("install failed"),
                destination_state: LocalAtomicDestinationState::Replaced,
                staging_state,
            },
            &mut staged,
            durability,
            |_| {
                cleanup_calls.set(cleanup_calls.get() + 1);
                if cleanup_succeeds {
                    Ok(())
                } else {
                    Err(io::Error::other("cleanup failed"))
                }
            },
            |_| {},
            |_| {
                sync_calls.set(sync_calls.get() + 1);
                if sync_succeeds {
                    Ok(())
                } else {
                    Err(io::Error::other("sync failed"))
                }
            },
        );
        (result, cleanup_calls.get(), sync_calls.get())
    }

    #[test]
    fn recovery_durability_matrix_tracks_cleanup_and_sync() {
        for (durability, cleanup, state, sync, expected, cleanup_calls, sync_calls) in [
            (
                LocalDurabilityRequirement::NotRequired,
                true,
                AtomicStagingState::Present,
                false,
                Some(false),
                1,
                0,
            ),
            (
                LocalDurabilityRequirement::Preferred,
                true,
                AtomicStagingState::Present,
                true,
                Some(true),
                1,
                1,
            ),
            (
                LocalDurabilityRequirement::Preferred,
                true,
                AtomicStagingState::Present,
                false,
                Some(false),
                1,
                1,
            ),
            (
                LocalDurabilityRequirement::Required,
                true,
                AtomicStagingState::Present,
                true,
                Some(true),
                1,
                1,
            ),
            (
                LocalDurabilityRequirement::Required,
                true,
                AtomicStagingState::Present,
                false,
                None,
                1,
                1,
            ),
            (
                LocalDurabilityRequirement::Preferred,
                false,
                AtomicStagingState::Present,
                true,
                None,
                1,
                1,
            ),
            (
                LocalDurabilityRequirement::Preferred,
                false,
                AtomicStagingState::Present,
                false,
                None,
                1,
                1,
            ),
            (
                LocalDurabilityRequirement::Required,
                false,
                AtomicStagingState::Present,
                true,
                None,
                1,
                1,
            ),
            (
                LocalDurabilityRequirement::Required,
                false,
                AtomicStagingState::Present,
                false,
                None,
                1,
                1,
            ),
            (
                LocalDurabilityRequirement::NotRequired,
                false,
                AtomicStagingState::Present,
                true,
                None,
                1,
                0,
            ),
            (
                LocalDurabilityRequirement::Preferred,
                true,
                AtomicStagingState::Indeterminate,
                true,
                None,
                0,
                1,
            ),
            (
                LocalDurabilityRequirement::Required,
                true,
                AtomicStagingState::Indeterminate,
                false,
                None,
                0,
                1,
            ),
            (
                LocalDurabilityRequirement::NotRequired,
                true,
                AtomicStagingState::Indeterminate,
                true,
                None,
                0,
                0,
            ),
        ] {
            let (result, actual_cleanup_calls, actual_sync_calls) = run_recovery(durability, cleanup, state, sync);
            assert_eq!(cleanup_calls, actual_cleanup_calls);
            assert_eq!(sync_calls, actual_sync_calls);
            match expected {
                Some(durable) => assert_eq!(Some(durable), result.ok()),
                None => {
                    let error = result.expect_err("recovery must retain installation failure");
                    assert_eq!(LocalAtomicDestinationState::Replaced, error.destination_state());
                    if cleanup
                        && state == AtomicStagingState::Present
                        && durability == LocalDurabilityRequirement::Required
                    {
                        assert_eq!(LocalAtomicWriteStage::SyncParent, error.stage());
                        assert!(error.cleanup_error().is_none());
                        assert!(error.source_error().to_string().contains("sync failed"));
                    } else {
                        assert_eq!(LocalAtomicWriteStage::ReplaceDestination, error.stage());
                        assert_eq!(cleanup, error.cleanup_error().is_none());
                        assert_eq!(
                            durability == LocalDurabilityRequirement::NotRequired || sync,
                            error.parent_sync_error().is_none()
                        );
                        assert!(error.source_error().to_string().contains("install failed"));
                    }
                }
            }
        }
    }
}
