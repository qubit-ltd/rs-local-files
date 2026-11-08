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
/// let limit = SizeLimit::Maximum(255);
/// let description = match limit {
///     SizeLimit::Maximum(bytes) => format!("at most {bytes} bytes"),
///     SizeLimit::VariesByPath => String::from("depends on the path"),
///     SizeLimit::Unknown => String::from("unavailable"),
/// };
/// assert_eq!(description, "at most 255 bytes");
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
