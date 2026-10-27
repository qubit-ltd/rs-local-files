// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
use crate::LocalFileMetadata;

/// Whether native identity and timestamp details are required.
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

/// Platform-specific native observation details.
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

/// Unix device, inode, and native change-time details.
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
