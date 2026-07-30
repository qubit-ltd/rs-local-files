// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
// qubit-style: allow source-test-pair
// Covered by writer integration tests.

use std::time::Duration;

use super::{
    LocalAtomicityRequirement,
    LocalDurabilityRequirement,
    LocalWriteMode,
};

/// Options fixed for a local file writer session.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LocalWriteOptions {
    /// Destination publication mode.
    mode: LocalWriteMode,
    /// Whether missing parent directories should be created.
    create_parent: bool,
    /// Required atomicity.
    atomicity: LocalAtomicityRequirement,
    /// Required durability.
    durability: LocalDurabilityRequirement,
    /// Optional maximum time spent retrying Unix lease conflicts.
    open_retry_timeout: Option<Duration>,
}

impl LocalWriteOptions {
    /// Creates writer options for the specified publication mode.
    ///
    /// # Parameters
    ///
    /// - `mode`: Destination publication mode.
    #[must_use]
    pub const fn new(mode: LocalWriteMode) -> Self {
        Self {
            mode,
            create_parent: false,
            atomicity: LocalAtomicityRequirement::Preferred,
            durability: LocalDurabilityRequirement::NotRequired,
            open_retry_timeout: None,
        }
    }

    /// Returns the publication mode.
    #[must_use]
    #[inline(always)]
    pub const fn mode(&self) -> LocalWriteMode {
        self.mode
    }

    /// Reports whether missing parent directories are created.
    #[must_use]
    #[inline(always)]
    pub const fn creates_parent(&self) -> bool {
        self.create_parent
    }

    /// Returns the required atomicity.
    #[must_use]
    #[inline(always)]
    pub const fn atomicity(&self) -> LocalAtomicityRequirement {
        self.atomicity
    }

    /// Returns the required durability.
    #[must_use]
    #[inline(always)]
    pub const fn durability(&self) -> LocalDurabilityRequirement {
        self.durability
    }

    /// Returns the configured Unix open retry timeout.
    #[must_use]
    #[inline(always)]
    pub const fn open_retry_timeout(&self) -> Option<Duration> {
        self.open_retry_timeout
    }

    /// Enables creation of missing parent directories.
    #[must_use]
    #[inline(always)]
    pub const fn with_parent(mut self) -> Self {
        self.create_parent = true;
        self
    }

    /// Sets the required atomicity.
    #[must_use]
    #[inline(always)]
    pub const fn with_atomicity(
        mut self,
        atomicity: LocalAtomicityRequirement,
    ) -> Self {
        self.atomicity = atomicity;
        self
    }

    /// Sets the required durability.
    #[must_use]
    #[inline(always)]
    pub const fn with_durability(
        mut self,
        durability: LocalDurabilityRequirement,
    ) -> Self {
        self.durability = durability;
        self
    }

    /// Sets the maximum time spent retrying Unix lease conflicts.
    #[must_use]
    #[inline(always)]
    pub const fn with_open_retry_timeout(mut self, timeout: Duration) -> Self {
        self.open_retry_timeout = Some(timeout);
        self
    }
}
