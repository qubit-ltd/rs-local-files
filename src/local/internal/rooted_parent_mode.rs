// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Rooted parent traversal modes.
// qubit-style: allow source-test-pair
// Private behavior is covered through public integration tests.

/// Controls missing-parent creation and durability tracking during traversal.
#[must_use]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(in crate::local) enum RootedParentMode {
    /// Require every parent directory to exist.
    OpenExisting,
    /// Create missing parents without retaining synchronization descriptors.
    CreateMissing,
    /// Create missing parents and retain synchronization descriptors.
    CreateMissingAndTrackSync,
}

impl RootedParentMode {
    /// Reports whether missing parent directories should be created.
    ///
    /// # Returns
    ///
    /// `true` for either creation mode; otherwise, `false`.
    #[inline(always)]
    pub(in crate::local) const fn creates_missing(self) -> bool {
        matches!(self, Self::CreateMissing | Self::CreateMissingAndTrackSync)
    }

    /// Reports whether created parent entries require durability tracking.
    ///
    /// # Returns
    ///
    /// `true` only for creation with synchronization tracking.
    #[inline(always)]
    pub(in crate::local) const fn tracks_sync(self) -> bool {
        matches!(self, Self::CreateMissingAndTrackSync)
    }
}
