// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Error and synchronization helpers for rooted atomic writers.

#[cfg(unix)]
use std::fs::File;
#[cfg(any(unix, windows))]
use std::io;
#[cfg(any(unix, windows))]
use std::path::Path;
#[cfg(any(unix, windows))]
use std::path::PathBuf;

use super::LocalAtomicDestinationState;
use super::LocalAtomicWriteError;
use super::LocalAtomicWriteStage;
#[cfg(all(unix, feature = "test-support"))]
use super::internal::test_support;

/// Persists the final parent entry and any newly created ancestor entries.
///
/// The ancestor descriptors are synchronized from the closest parent toward
/// the root so each newly created directory entry is durable before its own
/// parent. Returns the first native I/O error; when test fault injection is
/// enabled, the configured synchronization scenarios return that error before
/// performing any synchronization.
#[cfg(unix)]
pub(super) fn sync_rooted_parent_chain(parent: &File, parent_dirs_to_sync: &[File]) -> io::Result<()> {
    #[cfg(feature = "test-support")]
    if test_support::is_enabled("atomic-install-unlink-recover-sync")
        || test_support::is_enabled("atomic-install-unlink-persistent-sync")
        || test_support::is_enabled("atomic-install-unlink-indeterminate-sync")
        || test_support::is_enabled("rooted-preferred-parent-sync")
    {
        return Err(crate::local::test_fault_error());
    }
    parent.sync_all()?;
    for directory in parent_dirs_to_sync.iter().rev() {
        directory.sync_all()?;
    }
    Ok(())
}

/// Converts a native I/O result into an atomic-write error with recovery
/// context.
///
/// On failure, the error retains the stage, destination path, optional staging
/// path, known destination state, and original I/O error. Successful values
/// pass through unchanged.
#[cfg(any(unix, windows))]
#[inline]
pub(super) fn map_atomic_error<T>(
    result: io::Result<T>,
    stage: LocalAtomicWriteStage,
    path: &Path,
    temporary_path: Option<PathBuf>,
    destination_state: LocalAtomicDestinationState,
) -> Result<T, LocalAtomicWriteError> {
    match result {
        Ok(value) => Ok(value),
        Err(source) => Err(LocalAtomicWriteError::new(
            stage,
            path.to_path_buf(),
            temporary_path,
            destination_state,
            source,
        )),
    }
}

/// Creates a structured unsupported rooted atomic-write error.
#[cfg(not(unix))]
#[must_use = "the unsupported atomic-write error should be returned or handled"]
#[inline]
pub(super) fn unsupported_atomic_error(path: &Path) -> LocalAtomicWriteError {
    LocalAtomicWriteError::new(
        LocalAtomicWriteStage::PrepareParent,
        path.to_path_buf(),
        None,
        LocalAtomicDestinationState::Unchanged,
        io::Error::new(
            io::ErrorKind::Unsupported,
            "secure rooted atomic writes are unsupported on this target",
        ),
    )
}
