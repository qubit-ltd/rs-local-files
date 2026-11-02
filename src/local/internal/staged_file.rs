// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Panic-safe ownership of an uncommitted staging file.
// Private behavior is covered through public integration tests.

use std::fs;
use std::fs::File;
use std::io::Result;
use std::path::Path;
use std::path::PathBuf;

/// Owns a staging file until its filesystem commit succeeds.
///
/// Dropping an armed guard closes the file handle and best-effort removes its
/// path. A successful commit disarms cleanup after the path has been moved.
#[must_use = "dropping the staging guard removes the uncommitted file"]
#[derive(Debug)]
pub(crate) struct StagedFile {
    /// Path removed while cleanup remains armed.
    path: Option<PathBuf>,
    /// Open staging handle closed before removal or commit.
    file: Option<File>,
    /// Identity captured from the creation handle; `None` never authorizes
    /// removal.
    identity: Option<StagedIdentity>,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum StagedIdentity {
    #[cfg(unix)]
    Unix(u64, u64),
    #[cfg(windows)]
    Windows(u32, u64),
}

fn file_identity(file: &File) -> Option<StagedIdentity> {
    #[cfg(unix)]
    {
        let metadata = file.metadata().ok()?;
        use std::os::unix::fs::MetadataExt;
        Some(StagedIdentity::Unix(metadata.dev(), metadata.ino()))
    }
    #[cfg(windows)]
    {
        windows_file_identity(file).map(|(volume, index)| StagedIdentity::Windows(volume, index))
    }
}

fn path_identity(path: &Path) -> std::io::Result<Option<StagedIdentity>> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(error) => return Err(error),
    };
    if !metadata.file_type().is_file() {
        return Ok(None);
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        Ok(Some(StagedIdentity::Unix(metadata.dev(), metadata.ino())))
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;

        use windows_sys::Win32::Storage::FileSystem::FILE_FLAG_OPEN_REPARSE_POINT;

        let file = fs::OpenOptions::new()
            .read(true)
            .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT)
            .open(path)?;
        Ok(windows_file_identity(&file)
            .map(|(volume, index)| StagedIdentity::Windows(volume, index)))
    }
}

#[cfg(windows)]
fn windows_file_identity(file: &File) -> Option<(u32, u64)> {
    use std::os::windows::io::AsRawHandle;

    use windows_sys::Win32::Storage::FileSystem::BY_HANDLE_FILE_INFORMATION;
    use windows_sys::Win32::Storage::FileSystem::GetFileInformationByHandle;

    let mut information = BY_HANDLE_FILE_INFORMATION::default();
    // SAFETY: `file` owns a live handle and `information` is a writable buffer
    // with the layout required by `GetFileInformationByHandle`.
    if unsafe { GetFileInformationByHandle(file.as_raw_handle(), &raw mut information) } == 0 {
        return None;
    }
    let index =
        (u64::from(information.nFileIndexHigh) << 32) | u64::from(information.nFileIndexLow);
    Some((information.dwVolumeSerialNumber, index))
}

impl StagedFile {
    /// Creates an armed staging-file guard.
    ///
    /// # Parameters
    /// - `path`: Path owned by the guard until commit.
    /// - `file`: Open handle for `path`.
    ///
    /// # Returns
    /// A guard that removes `path` unless disarmed.
    #[inline]
    pub(crate) fn new(path: PathBuf, file: File) -> Self {
        let identity = file_identity(&file);
        Self {
            path: Some(path),
            file: Some(file),
            identity,
        }
    }

    /// Returns the staging path while cleanup is armed.
    ///
    /// # Returns
    /// The path owned by this guard.
    ///
    /// # Panics
    /// Panics when called after cleanup has been disarmed.
    #[must_use]
    #[inline]
    pub(crate) fn path(&self) -> &Path {
        self.path.as_deref().expect("staging path has already been disarmed")
    }

    /// Returns the open staging file.
    ///
    /// # Returns
    /// A shared reference to the owned file handle.
    ///
    /// # Panics
    /// Panics when called after the handle has been closed.
    #[must_use]
    #[inline]
    pub(crate) fn file(&self) -> &File {
        self.file.as_ref().expect("staging file handle has already been closed")
    }

    /// Returns the open staging file mutably.
    ///
    /// # Returns
    /// A mutable reference to the owned file handle.
    ///
    /// # Panics
    /// Panics when called after the handle has been closed.
    #[must_use]
    #[inline]
    pub(crate) fn file_mut(&mut self) -> &mut File {
        self.file.as_mut().expect("staging file handle has already been closed")
    }

    /// Returns whether the staging data handle remains open for recovery.
    ///
    /// # Returns
    ///
    /// `true` before installation begins or explicit cleanup closes the handle.
    #[must_use]
    #[inline]
    pub(crate) const fn is_open(&self) -> bool {
        self.file.is_some()
    }

    /// Closes the staging handle while keeping path cleanup armed.
    #[inline]
    pub(crate) fn close(&mut self) {
        drop(self.file.take());
    }

    /// Closes and removes the uncommitted staging file.
    ///
    /// Cleanup remains armed when removal fails so the caller can retry.
    ///
    /// # Errors
    /// Returns the I/O error reported while removing the staging path.
    pub(crate) fn cleanup(&mut self) -> Result<()> {
        self.close();
        if let Some(path) = self.path.as_ref() {
            #[cfg(feature = "test-support")]
            if super::test_support::is_enabled("atomic-install-unlink-persistent")
                || super::test_support::is_enabled("atomic-install-unlink-persistent-sync")
                || super::test_support::is_enabled("copy-staging-copy-cleanup")
            {
                return Err(crate::local::test_fault_error());
            }
            let current = path_identity(path)?;
            if current.is_none() {
                let _ = self.path.take();
                return Ok(());
            }
            if self.identity.is_none() || current != self.identity {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidInput,
                    "staging file identity changed before cleanup",
                ));
            }
            fs::remove_file(path)?;
            let _ = self.path.take();
        }
        Ok(())
    }

    /// Disarms path cleanup after a successful filesystem commit.
    ///
    /// The staging handle is closed before the guard is disarmed.
    #[inline]
    pub(crate) fn disarm(&mut self) {
        self.close();
        let _ = self.path.take();
    }
}

impl Drop for StagedFile {
    /// Closes and best-effort removes an uncommitted staging file.
    fn drop(&mut self) {
        let _ = self.cleanup();
    }
}
