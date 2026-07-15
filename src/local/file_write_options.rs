// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! File write options.

use crate::{
    FileBuffering,
    FileWriteMode,
};

/// Options used when opening a local file for writing.
///
/// Builder results must be used so that an accidentally discarded option does
/// not silently leave the original value unchanged:
///
/// ```compile_fail
/// #![deny(unused_must_use)]
/// use qubit_local_files::FileWriteOptions;
///
/// FileWriteOptions::default().with_parent();
/// ```
#[must_use = "file write options have no effect unless they are used"]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct FileWriteOptions {
    /// Whether missing parent directories should be created before opening.
    pub create_parent: bool,
    /// File creation and positioning mode.
    pub mode: FileWriteMode,
    /// Buffering policy for the returned writer.
    pub buffering: FileBuffering,
}

impl FileWriteOptions {
    /// Returns options for a specific write mode.
    ///
    /// # Parameters
    /// - `mode`: File write mode.
    ///
    /// # Returns
    /// Write options using `mode`, without parent creation and without
    /// buffering.
    #[inline]
    pub const fn new(mode: FileWriteMode) -> Self {
        Self {
            create_parent: false,
            mode,
            buffering: FileBuffering::Unbuffered,
        }
    }

    /// Enables parent directory creation.
    ///
    /// # Returns
    /// Updated options that create missing parent directories before opening.
    #[inline]
    pub const fn with_parent(mut self) -> Self {
        self.create_parent = true;
        self
    }

    /// Enables buffering with the standard-library default capacity.
    ///
    /// # Returns
    /// Updated options that return a buffered writer.
    #[inline]
    pub const fn buffered(mut self) -> Self {
        self.buffering = FileBuffering::buffered();
        self
    }

    /// Enables buffering with a custom capacity.
    ///
    /// # Parameters
    /// - `capacity`: Buffer capacity in bytes.
    ///
    /// # Returns
    /// Updated options that request a buffered writer with `capacity` bytes.
    ///
    /// # Errors
    /// Returns [`std::io::ErrorKind::InvalidInput`] when `capacity` is zero.
    #[inline]
    pub fn buffered_with_capacity(
        mut self,
        capacity: usize,
    ) -> std::io::Result<Self> {
        self.buffering = FileBuffering::buffered_with_capacity(capacity)?;
        Ok(self)
    }
}

impl Default for FileWriteOptions {
    /// Creates a missing file or truncates an existing file by default.
    #[inline]
    fn default() -> Self {
        Self::new(FileWriteMode::default())
    }
}
