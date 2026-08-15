// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Shared publication-state transitions for atomic writers.

use std::fs::File;
use std::io;

use crate::LocalAtomicCommitError;
use crate::LocalAtomicDestinationState;
use crate::LocalAtomicWriteError;
use crate::LocalDurabilityRequirement;

/// Runs an atomic publication attempt and retains pre-publication staging.
///
/// `attempt` performs backend-specific native operations. `staging_is_open`
/// identifies whether a failed attempt can still be retried or aborted.
pub(crate) fn commit_recoverably<W>(
    mut writer: W,
    attempt: impl FnOnce(&mut W) -> Result<bool, LocalAtomicWriteError>,
    staging_is_open: impl FnOnce(&W) -> bool,
) -> Result<bool, LocalAtomicCommitError<W>> {
    match attempt(&mut writer) {
        Ok(durable) => Ok(durable),
        Err(error) if staging_is_open(&writer) => {
            Err(LocalAtomicCommitError::new(error, Some(writer)))
        }
        Err(error) => Err(LocalAtomicCommitError::new(error, None)),
    }
}

/// Finalizes a consuming atomic commit failure using the shared cleanup rule.
///
/// Unchanged destinations retain a cleanup error. Once publication may have
/// changed the destination, `abandon` releases backend staging state instead.
pub(crate) fn finalize_failed_commit<W>(
    mut writer: W,
    error: LocalAtomicWriteError,
    cleanup: impl FnOnce(&mut W) -> io::Result<()>,
    abandon: impl FnOnce(&mut W),
) -> LocalAtomicWriteError {
    if error.destination_state() == LocalAtomicDestinationState::Unchanged {
        error.with_cleanup_error(cleanup(&mut writer).err())
    } else {
        abandon(&mut writer);
        error
    }
}

/// Synchronizes an atomic staging file according to its durability contract.
///
/// `map_required_error` supplies backend-specific path and publication context
/// for a required synchronization failure.
pub(crate) fn synchronize_staging_file(
    file: &File,
    durability: LocalDurabilityRequirement,
    map_required_error: impl FnOnce(
        io::Result<()>,
    ) -> Result<(), LocalAtomicWriteError>,
) -> Result<bool, LocalAtomicWriteError> {
    match durability {
        LocalDurabilityRequirement::NotRequired => Ok(false),
        LocalDurabilityRequirement::Preferred => Ok(file.sync_all().is_ok()),
        LocalDurabilityRequirement::Required => {
            map_required_error(file.sync_all())?;
            Ok(true)
        }
    }
}
