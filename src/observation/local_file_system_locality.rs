// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

/// Classification of the filesystem containing an observed object.
///
/// `Unknown` is a successful inconclusive observation and must not be treated
/// as evidence that the filesystem is local.
///
/// # Examples
///
/// ```
/// use qubit_local_files::observation::LocalFileSystemLocality;
///
/// let locality = LocalFileSystemLocality::Unknown;
/// assert_ne!(locality, LocalFileSystemLocality::Local);
/// ```
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[non_exhaustive]
#[must_use]
pub enum LocalFileSystemLocality {
    /// The filesystem is classified as local.
    Local,
    /// The filesystem is classified as remote.
    Remote,
    /// The filesystem could not be classified; this is still a successful
    /// observation.
    Unknown,
}
