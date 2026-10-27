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
use crate::policy::LocalFileSyncRequirement;
use crate::writer::internal::LocalCommitSyncObserver;

/// Runs an atomic publication attempt and retains pre-publication staging.
///
/// `attempt` performs backend-specific native operations. `staging_is_open`
/// identifies whether a failed attempt can still be retried or aborted.
///
/// # Returns
///
/// The backend's achieved durability flag after a successful publication.
///
/// # Errors
///
/// Preserves the attempt's structured error and returns writer ownership only
/// while its staging handle remains open. Otherwise the writer is dropped.
pub(crate) fn commit_recoverably<W, R>(
    mut writer: W,
    attempt: impl FnOnce(&mut W) -> Result<R, LocalAtomicWriteError>,
    staging_is_open: impl FnOnce(&W) -> bool,
) -> Result<R, LocalAtomicCommitError<W>> {
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
///
/// # Returns
///
/// `true` means file synchronization succeeded. `false` means it was not
/// requested or a preferred attempt failed; parent synchronization is separate.
///
/// # Errors
///
/// Returns the mapped native synchronization error only for `Required`.
pub(crate) fn synchronize_staging_file(
    file: &File,
    durability: LocalDurabilityRequirement,
    file_sync: LocalFileSyncRequirement,
    observer: Option<&LocalCommitSyncObserver>,
    map_required_error: impl FnOnce(io::Result<()>) -> Result<(), LocalAtomicWriteError>,
) -> Result<bool, LocalAtomicWriteError> {
    let requirement = match (durability, file_sync) {
        (LocalDurabilityRequirement::Required, _) | (_, LocalFileSyncRequirement::Required) => {
            LocalFileSyncRequirement::Required
        }
        (LocalDurabilityRequirement::Preferred, _) | (_, LocalFileSyncRequirement::Preferred) => {
            LocalFileSyncRequirement::Preferred
        }
        _ => LocalFileSyncRequirement::NotRequired,
    };
    match requirement {
        LocalFileSyncRequirement::NotRequired => Ok(false),
        LocalFileSyncRequirement::Preferred => Ok(sync_file(file, observer).is_ok()),
        LocalFileSyncRequirement::Required => {
            map_required_error(sync_file(file, observer))?;
            Ok(true)
        }
    }
}

fn sync_file(file: &File, observer: Option<&LocalCommitSyncObserver>) -> io::Result<()> {
    if let Some(observer) = observer {
        observer.record_file_sync();
    }
    #[cfg(feature = "test-support")]
    if crate::local::test_support_enabled("writer-staged-file-sync") {
        return Err(crate::local::test_fault_error());
    }
    file.sync_all()
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
    use crate::policy::LocalFileSyncRequirement;
    fn write_error(state: LocalAtomicDestinationState) -> LocalAtomicWriteError {
        LocalAtomicWriteError::new(
            LocalAtomicWriteStage::ReplaceDestination,
            PathBuf::from("target"),
            Some(PathBuf::from("staging")),
            state,
            io::Error::from(io::ErrorKind::PermissionDenied),
        )
    }

    fn successful_commit(_writer: &mut u8) -> Result<bool, LocalAtomicWriteError> {
        Ok(true)
    }

    fn failed_commit(_writer: &mut u8) -> Result<bool, LocalAtomicWriteError> {
        Err(write_error(LocalAtomicDestinationState::Unchanged))
    }

    const fn staging_is_open(writer: &u8) -> bool {
        *writer != 3
    }

    fn cleanup_writer(_writer: &mut bool) -> io::Result<()> {
        Err(io::Error::other("cleanup"))
    }

    fn abandon_writer(writer: &mut bool) {
        *writer = true;
    }

    fn map_required_sync(result: io::Result<()>) -> Result<(), LocalAtomicWriteError> {
        result.map_err(|_| write_error(LocalAtomicDestinationState::Unchanged))
    }

    #[test]
    fn test_commit_recoverably_reports_success_and_retains_only_open_staging() {
        assert!(commit_recoverably(1_u8, successful_commit, staging_is_open).expect("success should be retained"));

        let recoverable = commit_recoverably(2_u8, failed_commit, staging_is_open)
            .expect_err("open staging should be returned to the caller");
        assert_eq!(Some(&2), recoverable.writer());

        let terminal = commit_recoverably(3_u8, failed_commit, staging_is_open)
            .expect_err("closed staging must not be returned to the caller");
        assert!(terminal.writer().is_none());
    }

    #[test]
    fn test_finalize_failed_commit_preserves_cleanup_only_before_publication() {
        let unchanged = finalize_failed_commit(
            false,
            write_error(LocalAtomicDestinationState::Unchanged),
            cleanup_writer,
            abandon_writer,
        );
        assert_eq!(
            Some(io::ErrorKind::Other),
            unchanged.cleanup_error().map(io::Error::kind)
        );

        let published = finalize_failed_commit(
            false,
            write_error(LocalAtomicDestinationState::Replaced),
            cleanup_writer,
            abandon_writer,
        );
        assert_eq!(LocalAtomicDestinationState::Replaced, published.destination_state());
        assert!(published.cleanup_error().is_none());
    }

    #[test]
    fn test_synchronize_staging_file_obeys_each_durability_requirement() {
        let fixture = tempfile::NamedTempFile::new().expect("staging fixture should be created");
        assert!(
            !synchronize_staging_file(
                fixture.as_file(),
                LocalDurabilityRequirement::NotRequired,
                LocalFileSyncRequirement::NotRequired,
                None,
                map_required_sync,
            )
            .expect("not-required durability should skip synchronization")
        );
        assert!(
            synchronize_staging_file(
                fixture.as_file(),
                LocalDurabilityRequirement::Preferred,
                LocalFileSyncRequirement::NotRequired,
                None,
                map_required_sync,
            )
            .expect("preferred durability should synchronize a regular file")
        );
        assert!(
            synchronize_staging_file(
                fixture.as_file(),
                LocalDurabilityRequirement::Required,
                LocalFileSyncRequirement::NotRequired,
                None,
                map_required_sync,
            )
            .expect("required durability should synchronize a regular file")
        );
    }
}
