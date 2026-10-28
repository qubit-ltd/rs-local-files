// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use super::LocalAccessObservation;
use super::LocalFileSystemObservation;

#[derive(Clone, Debug, Eq, PartialEq)]
/// Security facts observed from one retained filesystem object.
///
/// The fields may represent observations taken at different instants; this
/// value is not an atomic snapshot and does not guarantee future permissions.
pub struct LocalSecurityObservation {
    identity: Option<super::LocalFileIdentity>,
    filesystem: Option<LocalFileSystemObservation>,
    access: Option<LocalAccessObservation>,
    link_count: Option<u64>,
}
impl LocalSecurityObservation {
    #[allow(dead_code)] // Consumed when T09 connects the platform query backends.
    pub(crate) const fn new(
        identity: Option<super::LocalFileIdentity>,
        filesystem: Option<LocalFileSystemObservation>,
        access: Option<LocalAccessObservation>,
        link_count: Option<u64>,
    ) -> Self {
        Self {
            identity,
            filesystem,
            access,
            link_count,
        }
    }
    /// Returns the object's identity when the platform supplied it.
    pub const fn identity(&self) -> Option<&super::LocalFileIdentity> {
        self.identity.as_ref()
    }
    /// Returns filesystem facts when requested and available.
    pub const fn filesystem(&self) -> Option<&LocalFileSystemObservation> {
        self.filesystem.as_ref()
    }
    /// Returns access-control facts when requested and available.
    pub const fn access(&self) -> Option<&LocalAccessObservation> {
        self.access.as_ref()
    }
    /// Returns the native link count when requested and available.
    pub const fn link_count(&self) -> Option<u64> {
        self.link_count
    }
}
