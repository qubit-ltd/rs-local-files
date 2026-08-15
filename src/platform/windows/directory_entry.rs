// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Windows directory entry observations.
// qubit-style: allow type-file-name

use std::ffi::OsStr;
use std::ffi::OsString;

use super::EntryIdentity;
use crate::LocalFileKind;

/// One immediate Windows child observed through its opened handle.
#[derive(Clone, Debug, Eq, PartialEq)]
#[must_use]
pub(crate) struct PlatformDirectoryEntry {
    /// Native child name.
    name: OsString,
    /// Normalized no-follow entry kind.
    kind: LocalFileKind,
    /// Identity captured from the opened child handle.
    identity: EntryIdentity,
}

impl PlatformDirectoryEntry {
    /// Creates an entry from one opened-child observation.
    pub(super) const fn new(
        name: OsString,
        kind: LocalFileKind,
        identity: EntryIdentity,
    ) -> Self {
        Self {
            name,
            kind,
            identity,
        }
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

    /// Returns identity captured from the opened child handle.
    pub(crate) const fn identity(&self) -> &EntryIdentity {
        &self.identity
    }
}
