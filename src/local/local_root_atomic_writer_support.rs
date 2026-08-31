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
#[cfg(feature = "internal-test-support")]
use super::internal::test_support;

/// Synchronizes the final parent and newly created ancestor entries.
#[cfg(unix)]
pub(super) fn sync_rooted_parent_chain(parent: &File, parent_dirs_to_sync: &[File]) -> io::Result<()> {
    #[cfg(feature = "internal-test-support")]
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

/// Adds structured atomic context to a native I/O result.
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
