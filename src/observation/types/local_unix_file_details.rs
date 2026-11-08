// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

/// Unix device, inode, and native change-time details from one handle.
///
/// Values are available through
/// [`crate::observation::LocalNativeFileDetails::unix`]. Device and
/// inode identify an object on the native filesystem, not its content version.
///
/// # Examples
///
/// ```
/// use qubit_local_files::observation::LocalUnixFileDetails;
///
/// fn object_key(details: &LocalUnixFileDetails) -> (u64, u64) {
///     (details.device(), details.inode())
/// }
/// ```
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LocalUnixFileDetails {
    /// Native device number associated with the observed object.
    device: u64,
    /// Native inode number associated with the observed object.
    inode: u64,
    /// Modification time seconds since the Unix epoch.
    modified_seconds: i64,
    /// Modification time nanoseconds within the second.
    modified_nanos: i64,
    /// Status-change time seconds since the Unix epoch.
    changed_seconds: i64,
    /// Status-change time nanoseconds within the second.
    changed_nanos: i64,
}

impl LocalUnixFileDetails {
    /// Builds details from the native stat values associated with one handle.
    ///
    /// # Parameters
    ///
    /// - `device`: native device number identifying the filesystem.
    /// - `inode`: native inode number identifying the object on that device.
    /// - `modified_seconds`: modification time seconds since the Unix epoch.
    /// - `modified_nanos`: nanoseconds within the modification-time second.
    /// - `changed_seconds`: status-change time seconds since the Unix epoch.
    /// - `changed_nanos`: nanoseconds within the status-change-time second.
    ///
    /// The nanosecond values must be in the range `0..1_000_000_000`, as
    /// validated by the caller before constructing these details.
    ///
    /// # Returns
    ///
    /// The native identity and timestamps exactly as supplied.
    #[must_use]
    #[cfg(unix)]
    #[inline]
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
    #[must_use]
    #[inline]
    pub const fn device(&self) -> u64 {
        self.device
    }
    /// Returns the native inode identifier.
    #[must_use]
    #[inline]
    pub const fn inode(&self) -> u64 {
        self.inode
    }
    /// Returns modification time seconds since the Unix epoch.
    #[must_use]
    #[inline]
    pub const fn modified_seconds(&self) -> i64 {
        self.modified_seconds
    }
    /// Returns modification time nanoseconds within the second.
    #[must_use]
    #[inline]
    pub const fn modified_nanos(&self) -> i64 {
        self.modified_nanos
    }
    /// Returns status-change time seconds since the Unix epoch.
    #[must_use]
    #[inline]
    pub const fn changed_seconds(&self) -> i64 {
        self.changed_seconds
    }
    /// Returns status-change time nanoseconds within the second.
    #[must_use]
    #[inline]
    pub const fn changed_nanos(&self) -> i64 {
        self.changed_nanos
    }
}
