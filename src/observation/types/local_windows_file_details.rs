// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

/// Windows volume, file ID, and native change-time details.
///
/// The timestamp accessors expose the signed native `FILETIME` count in
/// 100-nanosecond units since 1601-01-01 UTC. Values are kept as returned by
/// the operating system, including special or otherwise non-date values;
/// callers must not assume every value converts to a Unix timestamp.
///
/// # Examples
///
/// ```
/// use qubit_local_files::observation::LocalWindowsFileDetails;
///
/// fn native_id(details: &LocalWindowsFileDetails) -> (u64, &[u8; 16]) {
///     (details.volume_serial(), details.file_id())
/// }
/// ```
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LocalWindowsFileDetails {
    /// Serial identifying the volume containing the observed file.
    volume_serial: u64,
    /// Complete native 128-bit identifier for the file.
    file_id: [u8; 16],
    /// Signed native last-write `FILETIME` count in 100-nanosecond units.
    last_write_time: i64,
    /// Signed native change `FILETIME` count in 100-nanosecond units.
    change_time: i64,
}

impl LocalWindowsFileDetails {
    /// Builds details from values returned by the Windows handle query.
    #[cfg(any(windows, test))]
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
