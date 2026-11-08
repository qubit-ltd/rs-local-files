// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
use super::LocalFileSystemLocality;
use super::LocalUnixFileSystemObservation;
use super::LocalWindowsFileSystemObservation;

/// Filesystem classification and platform-specific filesystem facts.
///
/// A successful observation may still have `Unknown` locality, and platform
/// details are absent when they are unavailable or do not apply. Obtain this
/// value from a security observation; callers cannot construct it directly.
/// The individual facts are queried from a retained filesystem object and do
/// not form an atomic snapshot.
///
/// # Examples
///
/// ```
/// use qubit_local_files::LocalFileSystem;
/// use qubit_local_files::observation::LocalFileSystemObservation;
/// use qubit_local_files::observation::LocalSecurityObservationOptions;
/// use qubit_local_files::options::LocalDirectoryOpenOptions;
///
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let fixture = tempfile::tempdir()?;
/// let filesystem = LocalFileSystem::host()?;
/// let directory = filesystem.open_directory_with_options(
///     fixture.path(), &LocalDirectoryOpenOptions::new(),
/// )?;
/// let options = LocalSecurityObservationOptions::new();
/// let security = directory.security_observation(&options)?;
/// let filesystem_observation: Option<&LocalFileSystemObservation> = security.filesystem();
/// if let Some(filesystem) = filesystem_observation {
///     let _locality = filesystem.locality();
///     let _unix_details = filesystem.unix();
///     let _windows_details = filesystem.windows();
/// }
/// # Ok(())
/// # }
/// ```
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LocalFileSystemObservation {
    /// Classification of the filesystem; `Unknown` is inconclusive, not an
    /// error or evidence that the filesystem is local.
    locality: LocalFileSystemLocality,
    /// Unix-specific details when supplied by the current platform query.
    unix: Option<LocalUnixFileSystemObservation>,
    /// Windows-specific details when supplied by the current platform query.
    windows: Option<LocalWindowsFileSystemObservation>,
}
impl LocalFileSystemObservation {
    /// Creates a filesystem observation from facts gathered by a platform
    /// query backend.
    ///
    /// Platform-specific details may be absent when they do not apply or could
    /// not be obtained.
    ///
    /// # Parameters
    /// - `locality`: Classification result, including `Unknown` when evidence
    ///   is inconclusive.
    /// - `unix`: Optional Unix-specific filesystem facts.
    /// - `windows`: Optional Windows-specific filesystem facts.
    ///
    /// # Returns
    /// The assembled filesystem observation.
    #[allow(dead_code)] // Constructed by the platform query backends in T03/T04.
    #[must_use]
    #[inline]
    pub(crate) const fn new(
        locality: LocalFileSystemLocality,
        unix: Option<LocalUnixFileSystemObservation>,
        windows: Option<LocalWindowsFileSystemObservation>,
    ) -> Self {
        Self {
            locality,
            unix,
            windows,
        }
    }
    /// Returns the classification; `Unknown` is a successful but inconclusive
    /// result and does not imply local storage.
    ///
    /// # Returns
    /// The best-effort locality classification, including `Unknown` when the
    /// available evidence cannot distinguish local from remote storage.
    pub const fn locality(&self) -> LocalFileSystemLocality {
        self.locality
    }
    /// Returns Unix filesystem details when observed on Unix, or `None` on
    /// other platforms where these details are not available. `None` also
    /// represents a platform query that could not provide these facts.
    ///
    /// # Returns
    /// Unix-specific facts when available, or `None` when they do not apply or
    /// the query could not provide them.
    #[inline]
    pub const fn unix(&self) -> Option<&LocalUnixFileSystemObservation> {
        self.unix.as_ref()
    }
    /// Returns Windows filesystem details when observed on Windows, or `None`
    /// on other platforms where these details are not available. `None` also
    /// represents a platform query that could not provide these facts.
    ///
    /// # Returns
    /// Windows-specific facts when available, or `None` when they do not apply
    /// or the query could not provide them.
    #[inline]
    pub const fn windows(&self) -> Option<&LocalWindowsFileSystemObservation> {
        self.windows.as_ref()
    }
}
