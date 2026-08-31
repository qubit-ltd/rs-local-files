// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Structured facts describing a local resource-limit failure.
// qubit-style: allow source-test-pair
// Covered by public error and walker integration tests.

use std::error::Error;
use std::fmt;

use super::LocalResourceKind;

/// Structured budget facts for a local resource-limit failure.
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
        }
    }

    /// Returns the exhausted resource dimension.
    #[inline]
    pub const fn resource(&self) -> LocalResourceKind {
        self.resource
    }

    /// Returns the configured resource capacity.
    #[inline]
    pub const fn limit(&self) -> usize {
        self.limit
    }

    /// Returns the capacity remaining at acquisition time.
    #[inline]
    pub const fn remaining(&self) -> usize {
        self.remaining
    }

    /// Returns the number of units requested by the operation.
    #[inline]
    pub const fn requested(&self) -> usize {
        self.requested
    }
}

impl fmt::Display for LocalResourceLimitError {
    /// Formats the complete resource-limit facts.
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{} resource limit exhausted: limit={}, remaining={}, requested={}",
            self.resource, self.limit, self.remaining, self.requested,
        )
    }
}

impl Error for LocalResourceLimitError {}
