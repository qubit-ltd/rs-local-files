/*******************************************************************************
 *
 *    Copyright (c) 2026 Haixing Hu.
 *
 *    SPDX-License-Identifier: Apache-2.0
 *
 *    Licensed under the Apache License, Version 2.0.
 *
 ******************************************************************************/
//! File write options.

use crate::{
    FileBuffering,
    FileWriteMode,
};

/// Options used when opening a local file for writing.
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
    #[inline]
    pub const fn buffered_with_capacity(mut self, capacity: usize) -> Self {
        self.buffering = FileBuffering::buffered_with_capacity(capacity);
        self
    }
}

impl Default for FileWriteOptions {
    /// Creates a missing file or truncates an existing file by default.
    #[inline]
    fn default() -> Self {
        Self::new(FileWriteMode::default())
    }
}
