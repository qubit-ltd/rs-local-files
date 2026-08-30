// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Unix directory entry observations.
// qubit-style: allow type-file-name

use std::ffi::OsStr;
use std::ffi::OsString;

use super::EntryIdentity;
use crate::LocalFileKind;

/// One immediate child observed without following its final component.
#[derive(Clone, Debug, Eq, PartialEq)]
#[must_use]
pub(crate) struct PlatformDirectoryEntry {
    /// Native child name.
    name: OsString,
    /// Normalized no-follow entry kind.
    kind: LocalFileKind,
    /// Native identity captured by the same status lookup.
    identity: EntryIdentity,
}

#[allow(dead_code)]
impl PlatformDirectoryEntry {
    /// Creates a directory entry from one completed no-follow lookup.
    ///
    /// # Parameters
    ///
    /// - `name`: Native immediate-child name.
    /// - `kind`: Normalized entry kind.
    /// - `identity`: Identity captured with `kind`.
    pub(super) const fn new(name: OsString, kind: LocalFileKind, identity: EntryIdentity) -> Self {
        Self { name, kind, identity }
    }

    /// Returns the native immediate-child name.
    #[must_use]
    pub(crate) fn name(&self) -> &OsStr {
        &self.name
    }

    /// Returns the normalized no-follow entry kind.
    pub(crate) const fn kind(&self) -> LocalFileKind {
        self.kind
    }

    /// Returns the identity captured for this directory entry.
    pub(crate) const fn identity(&self) -> &EntryIdentity {
        &self.identity
    }
}
