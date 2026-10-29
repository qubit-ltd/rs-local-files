// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
use crate::LocalFileMetadata;

/// Whether a security observation must provide the selected native facts.
///
/// `Optional` still attempts the query and returns `None` for unsupported
/// facts. `Required` turns unavailable or unsupported selected facts into
/// `RequirementNotMet`; permission, I/O, malformed-data, and identity-change
/// errors remain distinct failures.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
#[non_exhaustive]
pub enum LocalObservationRequirement {
    /// Return available native details when possible.
    #[default]
    Optional,
    /// Fail unless identity, length, and native change times are available.
    Required,
}

/// Opaque identity of a native filesystem object.
///
/// Identity distinguishes filesystem objects when the platform supplies it;
/// it is not a version of the object's contents. Rechecking identity alone
/// cannot prove that file bytes remained unchanged.
///
/// Obtain identities through [`LocalFileObservation::identity`] or
/// [`super::LocalSecurityObservation::identity`]. Equality compares the
/// platform identity values; it does not promise uniqueness after object
/// deletion and identifier reuse.
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
    native: LocalNativeIdentity,
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
enum LocalNativeIdentity {
    #[cfg(unix)]
    Unix { device: u64, inode: u64 },
    #[cfg(windows)]
    Windows { volume_serial: u64, file_id: [u8; 16] },
}

impl LocalFileIdentity {
    #[cfg(unix)]
    pub(crate) const fn unix(device: u64, inode: u64) -> Self {
        Self {
            native: LocalNativeIdentity::Unix { device, inode },
        }
    }

    #[cfg(windows)]
    pub(crate) const fn windows(volume_serial: u64, file_id: [u8; 16]) -> Self {
        Self {
            native: LocalNativeIdentity::Windows { volume_serial, file_id },
        }
    }
}

/// Platform-specific identity and timestamp details gathered from a retained
/// file handle.
///
/// Exactly one platform-specific accessor is expected to return `Some` when
/// native details are supplied; the other returns `None`. Both can be `None`
/// when the platform cannot provide these details.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LocalNativeFileDetails {
    unix: Option<LocalUnixFileDetails>,
    windows: Option<LocalWindowsFileDetails>,
}

impl LocalNativeFileDetails {
    #[cfg(unix)]
    pub(crate) const fn from_unix(details: LocalUnixFileDetails) -> Self {
        Self {
            unix: Some(details),
            windows: None,
        }
    }

    #[cfg(windows)]
    pub(crate) const fn from_windows(details: LocalWindowsFileDetails) -> Self {
        Self {
            unix: None,
            windows: Some(details),
        }
    }

    /// Returns Unix-specific details when observed on a Unix platform.
    pub const fn unix(&self) -> Option<&LocalUnixFileDetails> {
        self.unix.as_ref()
    }

    /// Returns Windows-specific details when observed on Windows.
    pub const fn windows(&self) -> Option<&LocalWindowsFileDetails> {
        self.windows.as_ref()
    }
}

/// Unix device, inode, and native change-time details from one handle.
///
/// Values are available through [`LocalNativeFileDetails::unix`]. Device and
/// inode identify an object on the native filesystem, not its content version.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LocalUnixFileDetails {
    device: u64,
    inode: u64,
    modified_seconds: i64,
    modified_nanos: i64,
    changed_seconds: i64,
    changed_nanos: i64,
}

impl LocalUnixFileDetails {
    #[cfg(unix)]
    pub(crate) const fn from_parts(
        device: u64,
        inode: u64,
        modified_seconds: i64,
        modified_nanos: i64,
        changed_seconds: i64,
        changed_nanos: i64,
    ) -> Self {
        Self {
            device,
            inode,
            modified_seconds,
            modified_nanos,
            changed_seconds,
            changed_nanos,
        }
    }

    /// Returns the native device identifier.
    pub const fn device(&self) -> u64 {
        self.device
    }
    /// Returns the native inode identifier.
    pub const fn inode(&self) -> u64 {
        self.inode
    }
    /// Returns modification time seconds since the Unix epoch.
    pub const fn modified_seconds(&self) -> i64 {
        self.modified_seconds
    }
    /// Returns modification time nanoseconds within the second.
    pub const fn modified_nanos(&self) -> i64 {
        self.modified_nanos
    }
    /// Returns status-change time seconds since the Unix epoch.
    pub const fn changed_seconds(&self) -> i64 {
        self.changed_seconds
    }
    /// Returns status-change time nanoseconds within the second.
    pub const fn changed_nanos(&self) -> i64 {
        self.changed_nanos
    }
}

/// Windows volume, file ID, and native change-time details.
///
/// The timestamp accessors expose the signed native `FILETIME` count in
/// 100-nanosecond units since 1601-01-01 UTC. Values are kept as returned by
/// the operating system, including special or otherwise non-date values;
/// callers must not assume every value converts to a Unix timestamp.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LocalWindowsFileDetails {
    volume_serial: u64,
    file_id: [u8; 16],
    last_write_time: i64,
    change_time: i64,
}

impl LocalWindowsFileDetails {
    #[cfg(windows)]
    pub(crate) const fn new(volume_serial: u64, file_id: [u8; 16], last_write_time: i64, change_time: i64) -> Self {
        Self {
            volume_serial,
            file_id,
            last_write_time,
            change_time,
        }
    }

    /// Returns the volume serial number.
    pub const fn volume_serial(&self) -> u64 {
        self.volume_serial
    }
    /// Returns the complete native 128-bit file identifier.
    pub const fn file_id(&self) -> &[u8; 16] {
        &self.file_id
    }
    /// Returns the native last-write timestamp.
    pub const fn last_write_time(&self) -> i64 {
        self.last_write_time
    }
    /// Returns the native change timestamp.
    pub const fn change_time(&self) -> i64 {
        self.change_time
    }
}

/// Metadata and native identity gathered from one retained handle.
///
/// Obtain observations from public filesystem operations such as
/// [`crate::directory::LocalDirectoryHandle::observe_entry`]. When identity is
/// available, it can be compared across observations from the same fixed
/// authority to check whether a name still refers to the same object. It does
/// not identify a content version or make separate observations atomic.
///
/// Use [`crate::directory::LocalDirectoryHandle::observe_entry`] to obtain an
/// observation. The identity and native details are optional because some
/// targets cannot provide them; metadata remains available when the operation
/// succeeds.
#[derive(Clone, Debug)]
pub struct LocalFileObservation {
    metadata: LocalFileMetadata,
    identity: Option<LocalFileIdentity>,
    native_details: Option<LocalNativeFileDetails>,
}

impl LocalFileObservation {
    pub(crate) const fn from_parts(
        metadata: LocalFileMetadata,
        identity: Option<LocalFileIdentity>,
        native_details: Option<LocalNativeFileDetails>,
    ) -> Self {
        Self {
            metadata,
            identity,
            native_details,
        }
    }

    /// Returns normalized metadata from the observed handle.
    pub const fn metadata(&self) -> &LocalFileMetadata {
        &self.metadata
    }

    /// Returns opaque object identity when the platform supplied it.
    pub const fn identity(&self) -> Option<&LocalFileIdentity> {
        self.identity.as_ref()
    }

    /// Returns same-handle native details when available.
    pub const fn native_details(&self) -> Option<&LocalNativeFileDetails> {
        self.native_details.as_ref()
    }
}

/// Result of checking whether a pathname still names its observed object.
///
/// Obtain this value from
/// [`crate::observation::LocalObservedReader::verify_path_binding`].
/// `Unchanged`, `Changed`, and `Missing` are successful verification results;
/// query, policy, and timeout failures are returned as errors. A successful
/// `Unchanged` result covers only the check that just completed and does not
/// lock the namespace against later changes.
///
/// ```
/// use std::path::Path;
/// use qubit_local_files::LocalFileSystem;
/// use qubit_local_files::observation::LocalSourceVerification;
/// use qubit_local_files::options::{LocalObservedReadOptions, LocalPathBindingOptions};
///
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let fixture = tempfile::tempdir()?;
/// std::fs::write(fixture.path().join("item"), b"data")?;
/// let filesystem = LocalFileSystem::host()?;
/// let reader = filesystem.open_observed_reader(
///     &fixture.path().join("item"),
///     &LocalObservedReadOptions::new()
///         .with_path_binding(LocalPathBindingOptions::new()),
/// )?;
/// match reader.verify_path_binding()? {
///     LocalSourceVerification::Unchanged => {}
///     LocalSourceVerification::Changed | LocalSourceVerification::Missing => {}
///     _ => {},
/// }
/// # Ok(())
/// # }
/// ```
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
#[non_exhaustive]
pub enum LocalSourceVerification {
    /// The name still resolves to the same observed object.
    Unchanged,
    /// The name or an observed binding now identifies a different object.
    Changed,
    /// The observed name no longer exists.
    Missing,
}

#[cfg(test)]
mod tests {
    use super::LocalNativeFileDetails;
    use super::LocalWindowsFileDetails;

    #[test]
    fn test_windows_native_details_preserve_full_ids_and_special_timestamps() {
        let file_id = [0x80, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 0xff];
        let details = LocalWindowsFileDetails {
            volume_serial: u64::MAX,
            file_id,
            last_write_time: i64::MIN,
            change_time: i64::MAX,
        };
        let native = LocalNativeFileDetails {
            unix: None,
            windows: Some(details),
        };
        assert!(native.unix().is_none());
        let windows = native
            .windows()
            .expect("Windows details remain readable on any platform");
        assert_eq!(windows.volume_serial(), u64::MAX);
        assert_eq!(windows.file_id(), &file_id);
        assert_eq!(windows.last_write_time(), i64::MIN);
        assert_eq!(windows.change_time(), i64::MAX);
    }
}
