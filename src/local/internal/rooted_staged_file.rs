// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Panic-safe ownership of a descriptor-relative staging file.
// qubit-style: allow source-test-pair
// Private behavior is covered through public integration tests.

use std::ffi::CString;
use std::fs::File;
use std::io::Error;
use std::io::Result;
use std::os::fd::AsRawFd;
use std::path::Path;
use std::path::PathBuf;

use log::warn;

use super::atomic_file_install::install_new_atomic_file_at;
use super::atomic_staging_state::AtomicStagingState;
use crate::LocalAtomicDestinationState;

/// Owns an uncommitted staging entry relative to its open parent directory.
///
/// The entry name and parent descriptor are the only cleanup authority. Its
/// path is retained solely for diagnostics.
#[must_use = "dropping the staging guard removes the uncommitted rooted file"]
#[derive(Debug)]
pub(in crate::local) struct RootedStagedFile {
    /// Parent directory descriptor that authorizes rename and cleanup.
    parent: File,
    /// Staging entry name while cleanup remains armed.
    name: Option<CString>,
    /// Open staging data handle until explicitly closed.
    file: Option<File>,
    /// Non-authoritative relative staging path for diagnostics.
    diagnostic_path: PathBuf,
}

impl RootedStagedFile {
    /// Creates an armed rooted staging guard.
    ///
    /// # Parameters
    ///
    /// * `parent` - Open destination parent directory.
    /// * `name` - Staging entry name within `parent`.
    /// * `file` - Open staging data handle.
    /// * `diagnostic_path` - Non-authoritative path for errors and logs.
    ///
    /// # Returns
    ///
    /// A guard that removes the staging entry unless disarmed after commit.
    #[inline]
    pub(in crate::local) fn new(parent: File, name: CString, file: File, diagnostic_path: PathBuf) -> Self {
        Self {
            parent,
            name: Some(name),
            file: Some(file),
            diagnostic_path,
        }
    }

    /// Returns the non-authoritative staging path for diagnostics.
    ///
    /// # Returns
    ///
    /// The relative staging path retained by this guard.
    #[must_use]
    #[inline(always)]
    pub(in crate::local) fn diagnostic_path(&self) -> &Path {
        &self.diagnostic_path
    }

    /// Returns the open staging data handle.
    ///
    /// # Returns
    ///
    /// The handle owned by this guard.
    ///
    /// # Panics
    ///
    /// Panics after the data handle has been closed.
    #[must_use]
    #[inline(always)]
    pub(in crate::local) fn file(&self) -> &File {
        self.file
            .as_ref()
            .expect("rooted staging file handle has already been closed")
    }

    /// Returns the open staging data handle mutably.
    ///
    /// # Returns
    ///
    /// The mutable handle owned by this guard.
    ///
    /// # Panics
    ///
    /// Panics after the data handle has been closed.
    #[must_use]
    #[inline(always)]
    pub(in crate::local) fn file_mut(&mut self) -> &mut File {
        self.file
            .as_mut()
            .expect("rooted staging file handle has already been closed")
    }

    /// Returns whether the staging data handle remains open for recovery.
    ///
    /// # Returns
    ///
    /// `true` before installation begins or explicit cleanup closes the handle.
    #[must_use]
    #[inline(always)]
    pub(in crate::local) const fn is_open(&self) -> bool {
        self.file.is_some()
    }

    /// Returns the open destination parent directory.
    ///
    /// # Returns
    ///
    /// The descriptor that authorizes staging entry operations.
    #[must_use]
    #[inline(always)]
    pub(in crate::local) fn parent(&self) -> &File {
        &self.parent
    }

    /// Closes the staging data handle while leaving cleanup armed.
    #[inline(always)]
    pub(in crate::local) fn close(&mut self) {
        drop(self.file.take());
    }

    /// Renames the staging entry over `destination` in the same parent.
    ///
    /// The data handle is closed before the namespace operation. Cleanup stays
    /// armed until the caller records successful replacement.
    ///
    /// # Parameters
    ///
    /// * `destination` - Final entry name in the same parent directory.
    ///
    /// # Errors
    ///
    /// Returns the operating-system error from `renameat`.
    ///
    /// # Panics
    ///
    /// Panics if the staging entry was already disarmed.
    pub(in crate::local) fn rename_to(&mut self, destination: &CString) -> Result<()> {
        self.close();
        let name = self
            .name
            .as_ref()
            .expect("rooted staging entry has already been disarmed");
        // SAFETY: both entry strings and the parent descriptor remain live for
        // this non-retaining same-directory rename.
        let result = unsafe {
            libc::renameat(
                self.parent.as_raw_fd(),
                name.as_ptr(),
                self.parent.as_raw_fd(),
                destination.as_ptr(),
            )
        };
        if result == -1 {
            return Err(Error::last_os_error());
        }
        Ok(())
    }

    /// Installs the staging entry only when the destination is absent.
    ///
    /// # Parameters
    ///
    /// * `destination` - Final entry name in the same parent directory.
    ///
    /// # Errors
    ///
    /// Returns the native error with the known destination and staging states.
    /// Cleanup remains armed until the caller handles those states explicitly.
    ///
    /// # Panics
    ///
    /// Panics if the staging entry was already disarmed.
    pub(in crate::local) fn install_new_to(
        &mut self,
        destination: &CString,
    ) -> std::result::Result<(), (Error, LocalAtomicDestinationState, AtomicStagingState)> {
        self.close();
        let name = self
            .name
            .as_ref()
            .expect("rooted staging entry has already been disarmed");
        install_new_atomic_file_at(self.parent.as_raw_fd(), name, self.parent.as_raw_fd(), destination)
    }

    /// Closes and removes the uncommitted staging entry.
    ///
    /// Cleanup remains armed after failure so [`Drop`] can retry and report it.
    ///
    /// # Errors
    ///
    /// Returns the operating-system error from `unlinkat`.
    pub(in crate::local) fn cleanup(&mut self) -> Result<()> {
        self.close();
        let Some(name) = self.name.as_ref() else {
            return Ok(());
        };
        #[cfg(feature = "internal-test-support")]
        if super::test_support::is_enabled("atomic-install-unlink-persistent")
            || super::test_support::is_enabled("atomic-install-unlink-persistent-sync")
            || super::test_support::is_enabled("rooted-copy-install-cleanup")
        {
            return Err(crate::local::test_fault_error());
        }
        // SAFETY: the live parent descriptor and NUL-terminated name remain
        // valid for this non-retaining unlink operation.
        let result = unsafe { libc::unlinkat(self.parent.as_raw_fd(), name.as_ptr(), 0) };
        if result == -1 {
            return Err(Error::last_os_error());
        }
        let _ = self.name.take();
        Ok(())
    }

    /// Disarms cleanup after the staging entry has been committed.
    #[inline(always)]
    pub(in crate::local) fn disarm(&mut self) {
        self.close();
        let _ = self.name.take();
    }
}

impl Drop for RootedStagedFile {
    /// Closes and best-effort removes an uncommitted rooted staging entry.
    fn drop(&mut self) {
        if let Err(error) = self.cleanup()
            && self.name.is_some()
        {
            warn!(
                "failed to remove uncommitted rooted staging file '{}': {}",
                self.diagnostic_path.display(),
                error,
            );
        }
    }
}
