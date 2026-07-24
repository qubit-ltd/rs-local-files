// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! File read options.

use crate::FileBuffering;
use std::time::Duration;

/// Options used when opening a local file for reading.
///
/// Configuration fields are private; use the constructors and getters:
///
/// ```compile_fail
/// use qubit_local_files::{FileBuffering, FileReadOptions};
///
/// let mut options = FileReadOptions::default();
/// options.buffering = FileBuffering::Unbuffered;
/// ```
#[must_use = "file read options have no effect unless they are used"]
#[non_exhaustive]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct FileReadOptions {
    /// Buffering policy for the returned reader.
    buffering: FileBuffering,
    /// Optional Unix retry deadline for a lease-conflicting open.
    open_retry_timeout: Option<Duration>,
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
            open_retry_timeout: None,
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
            open_retry_timeout: None,
        }
    }

    /// Returns options for a buffered reader using a custom capacity.
    ///
    /// # Parameters
    /// - `capacity`: Buffer capacity in bytes.
    ///
    /// # Returns
    /// Read options that request a buffered reader with `capacity` bytes.
    ///
    /// # Errors
    /// Returns [`std::io::ErrorKind::InvalidInput`] when `capacity` is zero.
    #[inline(always)]
    pub fn buffered_with_capacity(capacity: usize) -> std::io::Result<Self> {
        Ok(Self {
            buffering: FileBuffering::buffered_with_capacity(capacity)?,
            open_retry_timeout: None,
        })
    }

    /// Returns the configured buffering policy.
    ///
    /// # Returns
    /// Buffering policy used by the opened reader.
    #[inline(always)]
    pub const fn buffering(&self) -> FileBuffering {
        self.buffering
    }

    /// Returns the configured retry timeout for opening a file.
    ///
    /// On Unix, this bounds retries when an active file lease makes a
    /// nonblocking defensive open return [`std::io::ErrorKind::WouldBlock`].
    /// `None` retains the default unbounded retry behavior. On other targets,
    /// this option has no effect.
    #[must_use]
    #[inline(always)]
    pub const fn open_retry_timeout(&self) -> Option<Duration> {
        self.open_retry_timeout
    }

    /// Sets a retry timeout for opening a file.
    ///
    /// On Unix, a zero timeout reports [`std::io::ErrorKind::TimedOut`] after
    /// the first lease-conflicting open attempt. Errors other than
    /// [`std::io::ErrorKind::WouldBlock`] are never retried. On other targets,
    /// this option has no effect.
    #[inline(always)]
    pub const fn with_open_retry_timeout(mut self, timeout: Duration) -> Self {
        self.open_retry_timeout = Some(timeout);
        self
    }
}
