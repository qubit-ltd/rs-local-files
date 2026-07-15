// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Panic-safe ownership of an uncommitted staging file.

use std::fs::{
    self,
    File,
};
use std::path::{
    Path,
    PathBuf,
};

/// Owns a staging file until its filesystem commit succeeds.
///
/// Dropping an armed guard closes the file handle and best-effort removes its
/// path. A successful commit disarms cleanup after the path has been moved.
#[derive(Debug)]
pub(crate) struct StagedFile {
    /// Path removed while cleanup remains armed.
    path: Option<PathBuf>,
    /// Open staging handle closed before removal or commit.
    file: Option<File>,
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
        Self {
            path: Some(path),
            file: Some(file),
        }
    }

    /// Returns the staging path while cleanup is armed.
    ///
    /// # Returns
    /// The path owned by this guard.
    ///
    /// # Panics
    /// Panics when called after cleanup has been disarmed.
    #[inline(always)]
    pub(crate) fn path(&self) -> &Path {
        self.path
            .as_deref()
            .expect("staging path has already been disarmed")
    }

    /// Returns the open staging file.
    ///
    /// # Returns
    /// A shared reference to the owned file handle.
    ///
    /// # Panics
    /// Panics when called after the handle has been closed.
    #[inline(always)]
    pub(crate) fn file(&self) -> &File {
        self.file
            .as_ref()
            .expect("staging file handle has already been closed")
    }

    /// Returns the open staging file mutably.
    ///
    /// # Returns
    /// A mutable reference to the owned file handle.
    ///
    /// # Panics
    /// Panics when called after the handle has been closed.
    #[inline(always)]
    pub(crate) fn file_mut(&mut self) -> &mut File {
        self.file
            .as_mut()
            .expect("staging file handle has already been closed")
    }

    /// Closes the staging handle while keeping path cleanup armed.
    #[inline(always)]
    pub(crate) fn close(&mut self) {
        drop(self.file.take());
    }

    /// Disarms path cleanup after a successful filesystem commit.
    ///
    /// The staging handle is closed before the guard is disarmed.
    #[inline(always)]
    pub(crate) fn disarm(mut self) {
        self.close();
        let _ = self.path.take();
    }
}

impl Drop for StagedFile {
    /// Closes and best-effort removes an uncommitted staging file.
    fn drop(&mut self) {
        self.close();
        if let Some(path) = self.path.take() {
            drop(fs::remove_file(path));
        }
    }
}
