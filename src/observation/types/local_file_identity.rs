// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
use super::internal::LocalNativeIdentity;

/// Opaque identity of a native filesystem object.
///
/// Identity distinguishes filesystem objects when the platform supplies it;
/// it is not a version of the object's contents. Rechecking identity alone
/// cannot prove that file bytes remained unchanged.
///
/// Obtain identities through
/// [`crate::observation::LocalFileObservation::identity`]
/// or [`crate::observation::LocalSecurityObservation::identity`]. Equality
/// compares the platform identity values; it does not promise uniqueness after
/// object deletion and identifier reuse.
///
/// ```
/// use std::path::Path;
/// use qubit_local_files::LocalFileSystem;
/// use qubit_local_files::options::LocalDirectoryOpenOptions;
///
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let fixture = tempfile::tempdir()?;
/// std::fs::write(fixture.path().join("item"), b"data")?;
/// let filesystem = LocalFileSystem::host()?;
/// let directory = filesystem.open_directory_with_options(
///     fixture.path(), &LocalDirectoryOpenOptions::new(),
/// )?;
/// let observation = directory.observe_entry(Path::new("item"))?;
/// if let Some(identity) = observation.identity() {
///     let _object_identity = identity;
/// }
/// # Ok(())
/// # }
/// ```
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct LocalFileIdentity {
    /// Platform identifier kept opaque to callers.
    native: LocalNativeIdentity,
}

impl LocalFileIdentity {
    /// Builds an identity from Unix device and inode values.
    #[cfg(unix)]
    pub(crate) const fn unix(device: u64, inode: u64) -> Self {
        Self {
            native: LocalNativeIdentity::Unix { device, inode },
        }
    }

    /// Builds an identity from the Windows volume serial and complete file ID.
    #[cfg(windows)]
    pub(crate) const fn windows(volume_serial: u64, file_id: [u8; 16]) -> Self {
        Self {
            native: LocalNativeIdentity::Windows { volume_serial, file_id },
        }
    }
}
