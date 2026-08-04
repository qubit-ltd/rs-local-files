// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
// =============================================================================
//! A best-effort native filesystem size limit.

/// A finite, unrestricted, or unavailable native filesystem limit.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[must_use]
pub enum SizeLimit {
    /// The inclusive finite maximum reported by the filesystem.
    Maximum(u64),
    /// This API has no single finite limit to report.
    Unrestricted,
    /// The filesystem, platform, or caller authority could not report it.
    Unknown,
}
