// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Support levels for native filesystem capability snapshots.

/// Describes how strongly a local filesystem capability is known.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[must_use]
pub enum LocalFileSystemCapabilitySupport {
    /// The implementation is available, but the active mount was not probed.
    Implemented,
    /// The active authority or mount was explicitly verified at runtime.
    RuntimeVerified,
    /// The implementation cannot make a reliable support claim.
    Unknown,
}
