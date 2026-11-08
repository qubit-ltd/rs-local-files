// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use super::LocalAclPresence;

/// Unix mode, owner, and ACL presence facts observed from an object.
///
/// These facts are observational and may become stale immediately; they do not
/// guarantee that a later operation will be permitted.
///
/// Obtain this value from [`LocalSecurityObservation::access`] through
/// [`LocalObservedReader::security_observation`]. Its fields cannot be
/// constructed directly by callers.
///
/// # Examples
///
/// ```
/// use std::path::Path;
/// use qubit_local_files::LocalFileSystem;
/// use qubit_local_files::observation::{
///     LocalObservationRequirement, LocalSecurityObservationOptions,
/// };
/// use qubit_local_files::options::LocalObservedReadOptions;
///
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let fixture = tempfile::tempdir()?;
/// std::fs::write(fixture.path().join("item"), b"data")?;
/// let filesystem = LocalFileSystem::rooted(fixture.path())?;
/// let reader = filesystem.open_observed_reader(
///     Path::new("item"), &LocalObservedReadOptions::new(),
/// )?;
/// let facts = reader.security_observation(
///     &LocalSecurityObservationOptions::new()
///         .with_access(LocalObservationRequirement::Optional),
/// )?;
/// if let Some(access) = facts.access() {
///     if let Some(unix) = access.unix() {
///         let _mode = unix.mode();
///         let _acl = unix.access_acl();
///     }
/// }
/// # Ok(())
/// # }
/// ```
///
/// [`LocalSecurityObservation::access`]: super::LocalSecurityObservation::access
/// [`LocalObservedReader::security_observation`]: super::LocalObservedReader::security_observation
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LocalUnixAccessObservation {
    /// Permission and special mode bits returned by `st_mode & 0o7777`.
    mode: u32,
    /// Whether the observed owner matched the current effective user.
    owner_is_current_user: bool,
    /// Presence or support status of the access ACL at observation time.
    access_acl: LocalAclPresence,
    /// Presence or support status of the default ACL at observation time.
    default_acl: LocalAclPresence,
}

impl LocalUnixAccessObservation {
    /// Creates access facts from one Unix security query.
    ///
    /// The constructor is crate-visible because platform query code supplies
    /// the values; callers receive instances through public observation APIs.
    ///
    /// # Parameters
    ///
    /// * `mode` — permission and special mode bits from `st_mode & 0o7777`.
    /// * `owner_is_current_user` — whether the observed owner matches the
    ///   current effective user.
    /// * `access_acl` — access ACL presence or support status at observation
    ///   time.
    /// * `default_acl` — default ACL presence or support status at observation
    ///   time.
    #[cfg(unix)]
    #[allow(dead_code)] // Consumed when T03 connects Unix security queries.
    #[must_use]
    pub(crate) const fn new(
        mode: u32,
        owner_is_current_user: bool,
        access_acl: LocalAclPresence,
        default_acl: LocalAclPresence,
    ) -> Self {
        Self {
            mode,
            owner_is_current_user,
            access_acl,
            default_acl,
        }
    }
    /// Returns permission and special mode bits (`st_mode & 0o7777`).
    #[must_use]
    #[inline]
    pub const fn mode(&self) -> u32 {
        self.mode
    }
    /// Returns whether the owner matches the current effective user.
    #[must_use]
    #[inline]
    pub const fn owner_is_current_user(&self) -> bool {
        self.owner_is_current_user
    }
    /// Returns the access ACL status.
    #[must_use]
    #[inline]
    pub const fn access_acl(&self) -> LocalAclPresence {
        self.access_acl
    }
    /// Returns the default ACL status.
    #[must_use]
    #[inline]
    pub const fn default_acl(&self) -> LocalAclPresence {
        self.default_acl
    }
}
