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
        Err(error) if staging_is_open(&writer) => Err(LocalAtomicCommitError::new(error, Some(writer))),
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
    map_required_error: impl FnOnce(io::Result<()>) -> Result<(), LocalAtomicWriteError>,
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

#[cfg(test)]
mod tests {
    use std::io;
    use std::path::PathBuf;

    use super::commit_recoverably;
    use super::finalize_failed_commit;
    use super::synchronize_staging_file;
    use crate::LocalAtomicDestinationState;
    use crate::LocalAtomicWriteError;
    use crate::LocalAtomicWriteStage;
    use crate::LocalDurabilityRequirement;

    fn write_error(state: LocalAtomicDestinationState) -> LocalAtomicWriteError {
        LocalAtomicWriteError::new(
            LocalAtomicWriteStage::ReplaceDestination,
            PathBuf::from("target"),
            Some(PathBuf::from("staging")),
            state,
            io::Error::from(io::ErrorKind::PermissionDenied),
        )
    }

    #[test]
    fn test_commit_recoverably_reports_success_and_retains_only_open_staging() {
        assert!(commit_recoverably(1_u8, |_| Ok(true), |_| true).expect("success should be retained"));

        let recoverable = commit_recoverably(2_u8, |_| Err(write_error(LocalAtomicDestinationState::Unchanged)), |_| true)
            .expect_err("open staging should be returned to the caller");
        assert_eq!(Some(&2), recoverable.writer());

        let terminal = commit_recoverably(3_u8, |_| Err(write_error(LocalAtomicDestinationState::Unchanged)), |_| false)
            .expect_err("closed staging must not be returned to the caller");
        assert!(terminal.writer().is_none());
    }

    #[test]
    fn test_finalize_failed_commit_preserves_cleanup_only_before_publication() {
        let unchanged = finalize_failed_commit(
            (),
            write_error(LocalAtomicDestinationState::Unchanged),
            |_| Err(io::Error::other("cleanup")),
            |_| panic!("unchanged destination must not abandon"),
        );
        assert_eq!(Some(io::ErrorKind::Other), unchanged.cleanup_error().map(io::Error::kind));

        let published = finalize_failed_commit(
            false,
            write_error(LocalAtomicDestinationState::Replaced),
            |_| panic!("published destination must not clean up staging"),
            |writer| *writer = true,
        );
        assert_eq!(LocalAtomicDestinationState::Replaced, published.destination_state());
        assert!(published.cleanup_error().is_none());
    }

    #[test]
    fn test_synchronize_staging_file_obeys_each_durability_requirement() {
        let fixture = tempfile::NamedTempFile::new().expect("staging fixture should be created");
        assert!(!synchronize_staging_file(
            fixture.as_file(),
            LocalDurabilityRequirement::NotRequired,
            |_| unreachable!(),
        )
        .expect("not-required durability should skip synchronization"));
        assert!(synchronize_staging_file(
            fixture.as_file(),
            LocalDurabilityRequirement::Preferred,
            |_| unreachable!(),
        )
        .expect("preferred durability should synchronize a regular file"));
        assert!(synchronize_staging_file(
            fixture.as_file(),
            LocalDurabilityRequirement::Required,
            |result| result.map_err(|_| write_error(LocalAtomicDestinationState::Unchanged)),
        )
        .expect("required durability should synchronize a regular file"));
    }
}
