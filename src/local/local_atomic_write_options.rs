// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Atomic write options.

/// Options used when beginning a local atomic write.
///
/// Builder results must be used so that an accidentally discarded option does
/// not silently leave the original value unchanged.
#[must_use = "atomic write options have no effect unless they are used"]
#[non_exhaustive]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct LocalAtomicWriteOptions {
    /// Whether missing parent directories should be created before staging.
    create_parent: bool,
}

impl LocalAtomicWriteOptions {
    /// Returns atomic write options without parent creation.
    ///
    /// # Returns
    /// Default atomic write options.
    #[inline(always)]
    pub const fn new() -> Self {
        Self {
            create_parent: false,
        }
    }

    /// Returns whether missing parent directories will be created.
    ///
    /// # Returns
    /// `true` when parent creation is enabled.
    #[inline(always)]
    #[must_use]
    pub const fn creates_parent(&self) -> bool {
        self.create_parent
    }

    /// Enables parent directory creation.
    ///
    /// # Returns
    /// Updated options that create missing parent directories before staging.
    #[inline(always)]
    pub const fn with_parent(mut self) -> Self {
        self.create_parent = true;
        self
    }
}
