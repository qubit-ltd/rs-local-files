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
    ///
    /// # Parameters
    ///
    /// * `details` — Unix identity and timestamp values returned by that handle
    ///   query. The value is moved into this wrapper.
    ///
    /// # Returns
    ///
    /// A wrapper containing the supplied Unix details and no Windows details.
    #[cfg(unix)]
    #[inline]
    pub(crate) const fn from_unix(details: LocalUnixFileDetails) -> Self {
        Self {
            unix: Some(details),
            windows: None,
        }
    }

    /// Wraps Windows identity and timestamp details from one native handle
    /// query.
    ///
    /// # Parameters
    ///
    /// * `details` — Windows identity and timestamp values returned by that
    ///   handle query. The value is moved into this wrapper.
    ///
    /// # Returns
    ///
    /// A wrapper containing the supplied Windows details and no Unix details.
    #[cfg(any(windows, test))]
    #[inline]
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
    ///
    /// # Returns
    ///
    /// `Some` with the Unix details when present, or `None` when they are
    /// absent.
    #[inline]
    pub const fn unix(&self) -> Option<&LocalUnixFileDetails> {
        self.unix.as_ref()
    }

    /// Returns Windows-specific details when supplied by the native query.
    ///
    /// `None` means the observation did not contain Windows details.
    ///
    /// # Returns
    ///
    /// `Some` with the Windows details when present, or `None` when they are
    /// absent.
    #[inline]
    pub const fn windows(&self) -> Option<&LocalWindowsFileDetails> {
        self.windows.as_ref()
    }
}
