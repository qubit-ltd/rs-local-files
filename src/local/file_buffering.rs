// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! File buffering policy.

use std::io::{
    Error,
    ErrorKind,
    Result,
};
use std::num::NonZeroUsize;

/// Buffering policy for local file readers and writers.
#[must_use = "a buffering policy has no effect unless it is used"]
#[non_exhaustive]
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
        capacity: Option<NonZeroUsize>,
    },
}

impl FileBuffering {
    /// Returns whether this policy enables buffered I/O.
    ///
    /// # Returns
    /// `true` for buffered policies.
    #[inline(always)]
    pub const fn is_buffered(&self) -> bool {
        matches!(self, Self::Buffered { .. })
    }

    /// Returns the custom buffer capacity, when configured.
    ///
    /// # Returns
    /// Custom capacity for buffered I/O, or [`None`] for unbuffered I/O and
    /// default-capacity buffering.
    #[inline(always)]
    pub const fn capacity(&self) -> Option<NonZeroUsize> {
        match self {
            Self::Buffered { capacity } => *capacity,
            Self::Unbuffered => None,
        }
    }

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
    /// - `capacity`: Buffer capacity in bytes.
    ///
    /// # Returns
    /// A buffering policy that enables buffering with a custom capacity.
    ///
    /// # Errors
    /// Returns [`ErrorKind::InvalidInput`] when `capacity` is zero.
    #[inline]
    pub fn buffered_with_capacity(capacity: usize) -> Result<Self> {
        let capacity = NonZeroUsize::new(capacity).ok_or_else(|| {
            Error::new(
                ErrorKind::InvalidInput,
                "buffer capacity must be greater than zero",
            )
        })?;
        Ok(Self::Buffered {
            capacity: Some(capacity),
        })
    }
}

impl Default for FileBuffering {
    /// Uses an unbuffered file handle by default.
    #[inline]
    fn default() -> Self {
        Self::Unbuffered
    }
}
