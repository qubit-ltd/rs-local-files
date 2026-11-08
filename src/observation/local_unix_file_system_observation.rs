// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use super::LocalUnixPlatform;

/// Native Unix filesystem identity reported by a platform query.
///
/// The identifier is preserved as a zero-extended integer and may have
/// platform-specific meaning. The optional name is supplied only when the
/// operating system provides one. These observations describe the queried
/// filesystem and do not guarantee that later operations will observe the
/// same state.
///
/// # Examples
///
/// ```
/// use qubit_local_files::LocalFileSystem;
/// use qubit_local_files::observation::LocalSecurityObservationOptions;
/// use qubit_local_files::options::LocalDirectoryOpenOptions;
///
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let fixture = tempfile::tempdir()?;
/// let filesystem = LocalFileSystem::host()?;
/// let directory = filesystem.open_directory_with_options(
///     fixture.path(), &LocalDirectoryOpenOptions::new(),
/// )?;
/// let security = directory.security_observation(&LocalSecurityObservationOptions::new())?;
/// if let Some(unix) = security.filesystem().and_then(|value| value.unix()) {
///     let _platform = unix.platform();
///     let _type_id = unix.type_id();
///     let _type_name = unix.type_name();
/// }
/// # Ok(())
/// # }
/// ```
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LocalUnixFileSystemObservation {
    /// Unix family that defines the interpretation of `type_id`.
    platform: LocalUnixPlatform,
    /// Native filesystem identifier widened to `u64` without changing its
    /// numeric value.
    type_id: u64,
    /// Optional name supplied by the operating system for this filesystem.
    type_name: Option<String>,
}
impl LocalUnixFileSystemObservation {
    /// Creates a filesystem identity from values returned by a Unix query.
    ///
    /// `type_name` is absent when the platform query does not provide a name.
    /// This constructor is restricted to crate code so observations are
    /// produced by the platform query backends.
    ///
    /// # Parameters
    /// - `platform`: Unix family used to interpret the native identifier.
    /// - `type_id`: Native filesystem identifier, widened to `u64`.
    /// - `type_name`: Optional platform-provided filesystem name.
    ///
    /// # Returns
    /// The assembled Unix filesystem observation.
    #[cfg(unix)]
    #[allow(dead_code)] // Consumed when T03 connects Unix security queries.
    #[must_use]
    pub(crate) fn new(platform: LocalUnixPlatform, type_id: u64, type_name: Option<String>) -> Self {
        Self {
            platform,
            type_id,
            type_name,
        }
    }
    /// Returns the Unix platform family.
    ///
    /// # Returns
    /// The platform family used to interpret the filesystem identifier.
    #[must_use]
    #[inline]
    pub const fn platform(&self) -> LocalUnixPlatform {
        self.platform
    }
    /// Returns the native filesystem type identifier zero-extended to `u64`.
    ///
    /// # Returns
    /// The platform-specific native identifier represented as `u64`.
    #[must_use]
    #[inline]
    pub const fn type_id(&self) -> u64 {
        self.type_id
    }
    /// Returns the platform-provided filesystem type name, or `None` when the
    /// operating system did not provide one.
    ///
    /// # Returns
    /// `Some(name)` when the platform supplied a filesystem type name, or
    /// `None` when it did not. The name is borrowed from this observation.
    #[inline]
    pub fn type_name(&self) -> Option<&str> {
        self.type_name.as_deref()
    }
}
