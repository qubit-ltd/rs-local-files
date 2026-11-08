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
/// use std::path::Path;
///
/// use qubit_local_files::LocalFileSystem;
/// use qubit_local_files::options::LocalDirectoryOpenOptions;
///
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let scratch = tempfile::tempdir()?;
/// std::fs::write(scratch.path().join("payload"), b"data")?;
/// let filesystem = LocalFileSystem::host()?;
/// let directory = filesystem.open_directory_with_options(
///     scratch.path(),
///     &LocalDirectoryOpenOptions::new(),
/// )?;
/// let observation = directory.observe_entry(Path::new("payload"))?;
/// if let Some(details) = observation
///     .native_details()
///     .and_then(|native| native.windows())
/// {
///     assert_eq!(details.file_id().len(), 16);
/// }
/// # Ok(())
/// # }
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
    ///
    /// # Parameters
    ///
    /// - `volume_serial`: serial number of the volume containing the file.
    /// - `file_id`: complete native 128-bit identifier returned for the file.
    /// - `last_write_time`: signed native last-write `FILETIME` count in
    ///   100-nanosecond units.
    /// - `change_time`: signed native change `FILETIME` count in 100-nanosecond
    ///   units.
    ///
    /// # Returns
    ///
    /// Details containing the supplied volume identifier, file identifier,
    /// and native timestamps.
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
    ///
    /// # Returns
    ///
    /// The serial number of the containing volume.
    #[must_use]
    #[inline]
    pub const fn volume_serial(&self) -> u64 {
        self.volume_serial
    }
    /// Returns the complete native 128-bit file identifier.
    ///
    /// # Returns
    ///
    /// A borrow of the full native file identifier.
    #[must_use]
    #[inline]
    pub const fn file_id(&self) -> &[u8; 16] {
        &self.file_id
    }
    /// Returns the native last-write timestamp.
    ///
    /// # Returns
    ///
    /// The signed native `FILETIME` count in 100-nanosecond units.
    #[must_use]
    #[inline]
    pub const fn last_write_time(&self) -> i64 {
        self.last_write_time
    }
    /// Returns the native change timestamp.
    ///
    /// # Returns
    ///
    /// The signed native `FILETIME` count in 100-nanosecond units.
    #[must_use]
    #[inline]
    pub const fn change_time(&self) -> i64 {
        self.change_time
    }
}
