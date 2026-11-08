// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use super::LocalAccessObservation;
use super::LocalFileSystemObservation;

/// Security facts observed from one retained filesystem object.
///
/// The fields may represent observations taken at different instants; this
/// value is not an atomic snapshot and does not guarantee future permissions.
/// Obtain it from `LocalDirectoryHandle::security_observation` or
/// `LocalObservedReader::security_observation`; its fields are not directly
/// constructible by callers. Optional fields are `None` only when the
/// corresponding fact is unavailable or unsupported on the platform. Optional
/// requirements still attempt the query; permission, I/O, malformed-data, or
/// identity-change failures remain errors.
///
/// # Examples
///
/// ```
/// use std::path::Path;
/// use qubit_local_files::LocalFileSystem;
/// use qubit_local_files::observation::{LocalSecurityObservationOptions, LocalObservationRequirement};
/// use qubit_local_files::options::LocalDirectoryOpenOptions;
///
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let fixture = tempfile::tempdir()?;
/// let filesystem = LocalFileSystem::host()?;
/// let directory = filesystem.open_directory_with_options(
///     fixture.path(), &LocalDirectoryOpenOptions::new(),
/// )?;
/// let options = LocalSecurityObservationOptions::new()
///     .with_filesystem(LocalObservationRequirement::Optional)
///     .with_access(LocalObservationRequirement::Optional);
/// let facts = match directory.security_observation(&options) {
///     Ok(facts) => facts,
///     Err(error) if error.kind() == qubit_local_files::error::LocalFileErrorKind::PermissionDenied => {
///         // Optional queries can still report access denial on the current object.
///         return Ok(());
///     }
///     Err(error) => return Err(error.into()),
/// };
/// if let Some(identity) = facts.identity() {
///     // Identity refers to the object, not a version of its contents.
///     let _ = identity;
/// }
/// if let Some(filesystem) = facts.filesystem() {
///     let _ = filesystem.locality();
/// }
/// if let Some(access) = facts.access() {
///     let _ = (access.unix(), access.windows());
/// }
/// if let Some(link_count) = facts.link_count() {
///     let _ = link_count;
/// }
/// # Ok(())
/// # }
/// ```
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LocalSecurityObservation {
    /// Native identity of the retained object, when the platform reports one.
    identity: Option<super::LocalFileIdentity>,
    /// Filesystem classification and native details, when the query succeeds.
    filesystem: Option<LocalFileSystemObservation>,
    /// Platform access-control facts, when the query succeeds.
    access: Option<LocalAccessObservation>,
    /// Native hard-link count, when the platform reports one.
    link_count: Option<u64>,
}
impl LocalSecurityObservation {
    /// Combines the independently obtained security facts for one retained
    /// object.
    ///
    /// Platform query backends use this constructor after checking that the
    /// retained handle still refers to the expected object. Each absent value
    /// represents an unavailable or unsupported fact; this value does not make
    /// the observations atomic or guarantee future permissions.
    ///
    /// # Parameters
    ///
    /// - `identity`: Native object identity, or `None` when unavailable.
    /// - `filesystem`: Filesystem facts, or `None` when unavailable.
    /// - `access`: Access-control facts, or `None` when unavailable.
    /// - `link_count`: Native link count, or `None` when unavailable.
    ///
    /// # Returns
    ///
    /// An observation containing the supplied facts without additional queries.
    #[inline]
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
    ///
    /// # Returns
    ///
    /// `Some(identity)` contains the native object identity; `None` means the
    /// platform did not provide one.
    #[must_use]
    #[inline]
    pub const fn identity(&self) -> Option<&super::LocalFileIdentity> {
        self.identity.as_ref()
    }
    /// Returns filesystem facts when the platform query succeeds.
    ///
    /// `None` means this fact is unsupported or unavailable; an optional
    /// requirement does not skip the query.
    ///
    /// # Returns
    ///
    /// `Some(filesystem)` contains the observed filesystem facts; `None` means
    /// they were unavailable or unsupported.
    #[must_use]
    #[inline]
    pub const fn filesystem(&self) -> Option<&LocalFileSystemObservation> {
        self.filesystem.as_ref()
    }
    /// Returns access-control facts when available on this platform.
    ///
    /// `None` means the platform does not provide these facts under the
    /// selected query requirements; permission and malformed-data failures
    /// are returned by the observation operation as errors.
    ///
    /// # Returns
    ///
    /// `Some(access)` contains observed access-control facts; `None` means the
    /// selected query did not provide them.
    #[must_use]
    #[inline]
    pub const fn access(&self) -> Option<&LocalAccessObservation> {
        self.access.as_ref()
    }
    /// Returns the native link count when the platform supplied it.
    ///
    /// `None` means the native query is unsupported or unavailable; optional
    /// requirements still attempt the query.
    ///
    /// # Returns
    ///
    /// `Some(count)` is the native link count; `None` means it was unavailable
    /// or unsupported.
    #[must_use]
    #[inline]
    pub const fn link_count(&self) -> Option<u64> {
        self.link_count
    }
}
