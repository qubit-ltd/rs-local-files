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
/// A cleanup failure can provide a retained resource whose cleanup ownership
/// can be relinquished to obtain this diagnostic location.
///
/// # Examples
///
/// ```
/// use qubit_local_files::directory::LocalTreeCleanupError;
/// use qubit_local_files::directory::LocalTreeRecoveryLocation;
///
/// fn relinquish_failed_cleanup(
///     error: LocalTreeCleanupError,
/// ) -> Vec<LocalTreeRecoveryLocation> {
///     error
///         .into_retained_resources()
///         .into_iter()
///         .map(|resource| {
///             let expected_path = resource.diagnostic_path().to_path_buf();
///             let location = resource.retain_in_place();
///             assert_eq!(location.namespace_path(), expected_path);
///             location
///         })
///         .collect()
/// }
/// ```
#[derive(Clone, Debug)]
pub struct LocalTreeRecoveryLocation {
    /// Namespace path captured when the retained resource relinquished cleanup
    /// ownership; it is descriptive and does not identify a live authority.
    namespace_path: PathBuf,
    /// Identity observed for the resource, or `None` when no identity was
    /// available at the time cleanup ownership was relinquished.
    identity: Option<LocalFileIdentity>,
}
impl LocalTreeRecoveryLocation {
    /// Records the namespace path and optional observed identity.
    ///
    /// This constructor is crate-private because recovery locations are
    /// produced by relinquishing an owned retained resource.
    ///
    /// # Parameters
    ///
    /// - `namespace_path`: Diagnostic namespace coordinate captured from the
    ///   retained resource.
    /// - `identity`: Identity observed for that resource, if one was available.
    #[must_use]
    #[inline]
    pub(crate) fn new(namespace_path: PathBuf, identity: Option<LocalFileIdentity>) -> Self {
        Self {
            namespace_path,
            identity,
        }
    }
    /// Returns the namespace path recorded when cleanup ownership was
    /// relinquished.
    #[must_use]
    #[inline]
    pub fn namespace_path(&self) -> &Path {
        &self.namespace_path
    }
    /// Returns the recorded identity, when one was observed.
    ///
    /// # Returns
    ///
    /// `Some(identity)` when an identity was available at relinquishment, or
    /// `None` when no identity was observed.
    #[inline]
    pub fn identity(&self) -> Option<&LocalFileIdentity> {
        self.identity.as_ref()
    }
}
