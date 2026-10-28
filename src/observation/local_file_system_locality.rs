// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[non_exhaustive]
/// Classification of the filesystem containing an observed object.
///
/// `Unknown` is a successful inconclusive observation and must not be treated
/// as evidence that the filesystem is local.
pub enum LocalFileSystemLocality {
    /// The filesystem is classified as local.
    Local,
    /// The filesystem is classified as remote.
    Remote,
    /// The filesystem could not be classified; this is still a successful
    /// observation.
    Unknown,
}
