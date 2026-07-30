// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Descriptor-relative directory entries.

use std::ffi::{
    OsStr,
    OsString,
};

use super::Metadata;

/// One immediate child observed through an opened rooted directory.
#[derive(Clone, Debug, Eq, PartialEq)]
#[must_use]
pub struct Entry {
    /// Native name of the immediate child.
    name: OsString,
    /// Metadata captured without following the final symbolic link.
    metadata: Metadata,
}

impl Entry {
    /// Builds a rooted directory entry.
    #[must_use]
    #[cfg(any(unix, windows))]
    #[inline]
    pub(crate) const fn new(name: OsString, metadata: Metadata) -> Self {
        Self { name, metadata }
    }

    /// Returns the native name of this immediate child.
    #[must_use]
    #[inline(always)]
    pub fn name(&self) -> &OsStr {
        &self.name
    }

    /// Returns metadata captured for the final child entry.
    #[must_use]
    #[inline(always)]
    pub const fn metadata(&self) -> Metadata {
        self.metadata
    }
}
