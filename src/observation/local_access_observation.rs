// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use super::LocalUnixAccessObservation;
use super::LocalWindowsAccessObservation;

/// Platform-specific access-control facts.
///
/// Obtain this value through [`super::LocalSecurityObservation::access`] after
/// querying a [`super::LocalObservedReader`] with
/// [`super::LocalObservedReader::security_observation`]. The `unix()` and
/// `windows()` accessors return `None` when that platform's facts are not
/// available. An optional query still runs and may report permission, I/O,
/// malformed-data, or identity-change errors; unsupported facts may be absent.
/// These facts describe observations, not an authorization decision, and
/// separate native queries need not form an atomic snapshot.
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
///     match (access.unix(), access.windows()) {
///         (Some(unix), _) => { let _observed_unix = unix; }
///         (_, Some(windows)) => { let _observed_windows = windows; }
///         (None, None) => {} // No platform-specific access facts available.
///     }
/// }
/// # Ok(())
/// # }
/// ```
///
/// [`super::LocalSecurityObservation::access`]: super::LocalSecurityObservation::access
/// [`super::LocalObservedReader`]: super::LocalObservedReader
/// [`super::LocalObservedReader::security_observation`]: super::LocalObservedReader::security_observation
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LocalAccessObservation {
    unix: Option<LocalUnixAccessObservation>,
    windows: Option<LocalWindowsAccessObservation>,
}
impl LocalAccessObservation {
    #[allow(dead_code)] // Constructed by the platform query backends in T03/T04.
    pub(crate) const fn new(
        unix: Option<LocalUnixAccessObservation>,
        windows: Option<LocalWindowsAccessObservation>,
    ) -> Self {
        Self { unix, windows }
    }
    /// Returns Unix access facts when observed on Unix, or `None` on other
    /// platforms where these facts are not available.
    pub const fn unix(&self) -> Option<&LocalUnixAccessObservation> {
        self.unix.as_ref()
    }
    /// Returns Windows access facts when observed on Windows, or `None` on
    /// other platforms where these facts are not available.
    pub const fn windows(&self) -> Option<&LocalWindowsAccessObservation> {
        self.windows.as_ref()
    }
}
