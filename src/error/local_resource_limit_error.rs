// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Structured facts describing a local resource-limit failure.
// Covered by public error and walker integration tests.

use std::error::Error;
use std::fmt;

use super::LocalResourceKind;

/// Structured budget facts for a local resource-limit failure.
///
/// # Examples
///
/// ```
/// use qubit_local_files::error::LocalResourceKind;
/// use qubit_local_files::error::LocalResourceLimitError;
///
/// let error = LocalResourceLimitError::new(LocalResourceKind::Entry, 4, 1, 2);
/// assert_eq!(error.requested(), 2);
/// ```
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[must_use]
pub struct LocalResourceLimitError {
    /// Resource dimension that could not be acquired.
    resource: LocalResourceKind,
    /// Configured resource capacity.
    limit: usize,
    /// Capacity remaining when acquisition was attempted.
    remaining: usize,
    /// Number of units requested by the operation.
    requested: usize,
    /// Exact measured amount when it fits in `usize`.
    observed: Option<usize>,
    /// Whether measurement overflowed `usize`.
    overflowed: bool,
}

impl LocalResourceLimitError {
    /// Creates a resource-limit error from complete budget facts.
    ///
    /// # Parameters
    ///
    /// - `resource`: Resource dimension that could not be acquired.
    /// - `limit`: Configured resource capacity.
    /// - `remaining`: Capacity remaining at acquisition time.
    /// - `requested`: Number of units requested by the operation.
    ///
    /// # Returns
    ///
    /// A structured resource-limit error.
    #[inline]
    pub const fn new(resource: LocalResourceKind, limit: usize, remaining: usize, requested: usize) -> Self {
        Self {
            resource,
            limit,
            remaining,
            requested,
            observed: None,
            overflowed: false,
        }
    }

    pub(crate) const fn with_observed(mut self, observed: usize) -> Self {
        self.observed = Some(observed);
        self.overflowed = false;
        self
    }

    /// Marks a measurement failure caused by integer overflow.
    pub(crate) const fn with_overflow(mut self) -> Self {
        self.observed = None;
        self.overflowed = true;
        self
    }

    /// Returns the exhausted resource dimension.
    #[must_use = "the exhausted resource dimension should be inspected"]
    #[inline]
    pub const fn resource(&self) -> LocalResourceKind {
        self.resource
    }

    /// Returns the configured resource capacity.
    #[must_use]
    #[inline]
    pub const fn limit(&self) -> usize {
        self.limit
    }

    /// Returns the capacity remaining at acquisition time.
    #[must_use]
    #[inline]
    pub const fn remaining(&self) -> usize {
        self.remaining
    }

    /// Returns the number of units requested by the operation.
    #[must_use]
    #[inline]
    pub const fn requested(&self) -> usize {
        self.requested
    }
    /// Returns the exact measured value when representable.
    pub const fn observed(&self) -> Option<usize> {
        self.observed
    }
    /// Returns whether checked arithmetic overflowed while measuring.
    pub const fn overflowed(&self) -> bool {
        self.overflowed
    }
}

impl fmt::Display for LocalResourceLimitError {
    /// Formats the complete resource-limit facts.
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{} resource limit exhausted: limit={}, remaining={}, requested={}, observed={:?}, overflowed={}",
            self.resource,
            self.limit,
            self.remaining,
            self.requested,
            self.observed,
            self.overflowed,
        )
    }
}

impl Error for LocalResourceLimitError {}
