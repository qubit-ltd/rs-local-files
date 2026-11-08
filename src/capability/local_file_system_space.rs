// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Dynamic native filesystem space observations.

/// Dynamic space values observed for one filesystem authority.
///
/// # Examples
///
/// ```
/// use qubit_local_files::capability::LocalFileSystemSpace;
///
/// let space = LocalFileSystemSpace::new(Some(1024), Some(768), Some(512));
/// assert_eq!(space.available_bytes(), Some(512));
/// ```
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[must_use]
pub struct LocalFileSystemSpace {
    /// Total volume capacity in bytes, when the authority reports it.
    capacity_bytes: Option<u64>,
    /// Unallocated volume capacity in bytes, including privileged reserve.
    free_bytes: Option<u64>,
    /// Unallocated capacity in bytes available to the current caller.
    available_bytes: Option<u64>,
}

impl LocalFileSystemSpace {
    /// Creates space observations from independently available values.
    ///
    /// # Parameters
    ///
    /// * `capacity_bytes` - Total filesystem capacity, or `None` when unknown.
    /// * `free_bytes` - Unallocated capacity including privileged reserve, or
    ///   `None` when unknown.
    /// * `available_bytes` - Capacity available to the caller, or `None` when
    ///   unknown.
    #[inline]
    pub const fn new(capacity_bytes: Option<u64>, free_bytes: Option<u64>, available_bytes: Option<u64>) -> Self {
        Self {
            capacity_bytes,
            free_bytes,
            available_bytes,
        }
    }

    /// Returns the total filesystem capacity when it can be observed.
    ///
    /// # Returns
    ///
    /// `Some(bytes)` when capacity is known, or `None` when it is unavailable.
    #[inline]
    pub const fn capacity_bytes(&self) -> Option<u64> {
        self.capacity_bytes
    }

    /// Returns filesystem free capacity, including reserved space, when known.
    ///
    /// # Returns
    ///
    /// `Some(bytes)` when free capacity is known, or `None` when it is
    /// unavailable.
    #[inline]
    pub const fn free_bytes(&self) -> Option<u64> {
        self.free_bytes
    }

    /// Returns capacity currently available to the calling identity when known.
    ///
    /// # Returns
    ///
    /// `Some(bytes)` when caller-available capacity is known, or `None` when it
    /// is unavailable.
    #[inline]
    pub const fn available_bytes(&self) -> Option<u64> {
        self.available_bytes
    }
}
