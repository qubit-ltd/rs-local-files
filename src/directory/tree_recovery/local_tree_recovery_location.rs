// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
use std::path::Path;
use std::path::PathBuf;

use crate::observation::LocalFileIdentity;

/// Path and identity recorded after explicitly relinquishing cleanup ownership.
///
/// This value is a diagnostic record only. Its namespace path does not remain
/// bound to the original entry and it carries no authority to access or clean
/// that entry.
///
/// # Examples
///
/// ```
/// use qubit_local_files::directory::LocalTreeRecoveryLocation;
/// use std::path::Path;
///
/// fn recorded_path(location: &LocalTreeRecoveryLocation) -> &Path {
///     location.namespace_path()
/// }
/// ```
#[derive(Clone, Debug)]
pub struct LocalTreeRecoveryLocation {
    namespace_path: PathBuf,
    identity: Option<LocalFileIdentity>,
}
impl LocalTreeRecoveryLocation {
    pub(crate) fn new(namespace_path: PathBuf, identity: Option<LocalFileIdentity>) -> Self {
        Self {
            namespace_path,
            identity,
        }
    }
    /// Returns the namespace path recorded when cleanup ownership was
    /// relinquished.
    pub fn namespace_path(&self) -> &Path {
        &self.namespace_path
    }
    /// Returns the recorded identity, when one was observed.
    pub fn identity(&self) -> Option<&LocalFileIdentity> {
        self.identity.as_ref()
    }
}
