/*******************************************************************************
 *
 *    Copyright (c) 2026 Haixing Hu.
 *
 *    SPDX-License-Identifier: Apache-2.0
 *
 *    Licensed under the Apache License, Version 2.0.
 *
 ******************************************************************************/
//! File buffering policy.

/// Buffering policy for local file readers and writers.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FileBuffering {
    /// Use the raw file handle without an additional standard-library buffer.
    Unbuffered,
    /// Wrap the file handle in a standard-library buffer.
    Buffered {
        /// Optional buffer capacity in bytes.
        ///
        /// When this value is [`None`], [`std::io::BufReader`] or
        /// [`std::io::BufWriter`] uses its default capacity.
        capacity: Option<usize>,
    },
}

impl FileBuffering {
    /// Returns buffered I/O using the standard-library default capacity.
    ///
    /// # Returns
    /// A buffering policy that enables buffering without a custom capacity.
    #[inline]
    pub const fn buffered() -> Self {
        Self::Buffered { capacity: None }
    }

    /// Returns buffered I/O using a caller-provided capacity.
    ///
    /// # Parameters
    /// - `capacity`: Buffer capacity in bytes. A zero capacity is accepted by
    ///   this constructor but rejected when opening the file, where an I/O
    ///   error can be returned.
    ///
    /// # Returns
    /// A buffering policy that enables buffering with a custom capacity.
    #[inline]
    pub const fn buffered_with_capacity(capacity: usize) -> Self {
        Self::Buffered {
            capacity: Some(capacity),
        }
    }
}

impl Default for FileBuffering {
    /// Uses an unbuffered file handle by default.
    #[inline]
    fn default() -> Self {
        Self::Unbuffered
    }
}
