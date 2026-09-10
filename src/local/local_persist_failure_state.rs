// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Namespace state known after a temporary-resource persistence failure.

use std::io;

/// Publication fact established by one failed persist or keep call.
///
/// Source ownership is independent; inspect the error's `source_state()`
/// before retrying or cleaning up its retained resource.
#[must_use]
#[non_exhaustive]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LocalPersistFailureState {
    /// This call did not publish its target; no source authority is implied.
    NotPublished,
    /// The target was published but a post-publication guarantee failed.
    Published,
    /// A native install attempt left publication state unknown.
    Indeterminate,
}

impl LocalPersistFailureState {
    /// Classifies only a completed native rename attempt's publication result.
    /// This classification never grants source ownership.
    pub(crate) const fn from_native_error(kind: io::ErrorKind) -> Self {
        match kind {
            io::ErrorKind::AlreadyExists
            | io::ErrorKind::CrossesDevices
            | io::ErrorKind::InvalidInput
            | io::ErrorKind::IsADirectory
            | io::ErrorKind::NotADirectory
            | io::ErrorKind::DirectoryNotEmpty
            | io::ErrorKind::Unsupported => Self::NotPublished,
            _ => Self::Indeterminate,
        }
    }
}
