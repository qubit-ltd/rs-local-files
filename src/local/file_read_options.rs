/*******************************************************************************
 *
 *    Copyright (c) 2026 Haixing Hu.
 *
 *    SPDX-License-Identifier: Apache-2.0
 *
 *    Licensed under the Apache License, Version 2.0.
 *
 ******************************************************************************/
//! File read options.

use crate::FileBuffering;

/// Options used when opening a local file for reading.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct FileReadOptions {
    /// Buffering policy for the returned reader.
    pub buffering: FileBuffering,
}

impl FileReadOptions {
    /// Returns options for an unbuffered reader.
    ///
    /// # Returns
    /// Read options that return a raw file-backed reader.
    #[inline]
    pub const fn unbuffered() -> Self {
        Self {
            buffering: FileBuffering::Unbuffered,
        }
    }

    /// Returns options for a buffered reader using the default capacity.
    ///
    /// # Returns
    /// Read options that return a buffered reader.
    #[inline]
    pub const fn buffered() -> Self {
        Self {
            buffering: FileBuffering::buffered(),
        }
    }

    /// Returns options for a buffered reader using a custom capacity.
    ///
    /// # Parameters
    /// - `capacity`: Buffer capacity in bytes.
    ///
    /// # Returns
    /// Read options that request a buffered reader with `capacity` bytes.
    #[inline]
    pub const fn buffered_with_capacity(capacity: usize) -> Self {
        Self {
            buffering: FileBuffering::buffered_with_capacity(capacity),
        }
    }
}
