// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
use super::LocalUnixFileDetails;
use super::LocalWindowsFileDetails;

/// Platform-specific identity and timestamp details gathered from a retained
/// file handle.
///
/// Exactly one platform-specific accessor is expected to return `Some` when
/// native details are supplied; the other returns `None`. Both can be `None`
/// when the platform cannot provide these details.
///
/// # Examples
///
/// ```
/// use qubit_local_files::observation::LocalNativeFileDetails;
///
/// fn has_native_times(details: &LocalNativeFileDetails) -> bool {
///     details.unix().is_some() || details.windows().is_some()
/// }
/// ```
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LocalNativeFileDetails {
    /// Unix identity and native times, or `None` when unavailable.
    unix: Option<LocalUnixFileDetails>,
    /// Windows identity and native times, or `None` when unavailable.
    windows: Option<LocalWindowsFileDetails>,
}

impl LocalNativeFileDetails {
    /// Wraps Unix identity and timestamp details from one native handle query.
    #[cfg(unix)]
    pub(crate) const fn from_unix(details: LocalUnixFileDetails) -> Self {
        Self {
            unix: Some(details),
            windows: None,
        }
    }

    /// Wraps Windows identity and timestamp details from one native handle
    /// query.
    #[cfg(any(windows, test))]
    pub(crate) const fn from_windows(details: LocalWindowsFileDetails) -> Self {
        Self {
            unix: None,
            windows: Some(details),
        }
    }

    /// Returns Unix-specific details when supplied by the native query.
    ///
    /// `None` means the observation did not contain Unix details; callers
    /// should not infer that the file is non-Unix or unchanged.
    pub const fn unix(&self) -> Option<&LocalUnixFileDetails> {
        self.unix.as_ref()
    }

    /// Returns Windows-specific details when supplied by the native query.
    ///
    /// `None` means the observation did not contain Windows details.
    pub const fn windows(&self) -> Option<&LocalWindowsFileDetails> {
        self.windows.as_ref()
    }
}

#[cfg(test)]
mod tests {
    use super::LocalNativeFileDetails;
    use crate::observation::LocalWindowsFileDetails;

    #[test]
    fn test_windows_native_details_preserve_full_ids_and_special_timestamps() {
        let file_id = [0x80, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 0xff];
        let details = LocalWindowsFileDetails::new(u64::MAX, file_id, i64::MIN, i64::MAX);
        let native = LocalNativeFileDetails::from_windows(details);
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
