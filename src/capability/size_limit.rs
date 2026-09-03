// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! A best-effort native filesystem size limit.

/// A finite, path-dependent, or unavailable native filesystem limit.
///
/// # Examples
///
/// ```
/// use qubit_local_files::capability::SizeLimit;
///
/// assert_eq!(SizeLimit::Maximum(255), SizeLimit::Maximum(255));
/// assert_ne!(SizeLimit::Unknown, SizeLimit::VariesByPath);
/// ```
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[must_use]
pub enum SizeLimit {
    /// The inclusive finite maximum reported by the filesystem.
    Maximum(
        /// Inclusive native limit in the unit carried by its observation.
        u64,
    ),
    /// The limit depends on the filesystem containing the queried path.
    VariesByPath,
    /// The filesystem, platform, or caller authority could not report it.
    Unknown,
}
