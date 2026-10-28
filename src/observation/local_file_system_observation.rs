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

#[derive(Clone, Debug, Eq, PartialEq)]
/// Filesystem classification and platform-specific filesystem facts.
pub struct LocalFileSystemObservation {
    locality: LocalFileSystemLocality,
    unix: Option<LocalUnixFileSystemObservation>,
    windows: Option<LocalWindowsFileSystemObservation>,
}
impl LocalFileSystemObservation {
    #[allow(dead_code)] // Constructed by the platform query backends in T03/T04.
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
    /// result.
    pub const fn locality(&self) -> LocalFileSystemLocality {
        self.locality
    }
    /// Returns Unix filesystem details when observed on Unix, or `None` on
    /// other platforms where these details are not available.
    pub const fn unix(&self) -> Option<&LocalUnixFileSystemObservation> {
        self.unix.as_ref()
    }
    /// Returns Windows filesystem details when observed on Windows, or `None`
    /// on other platforms where these details are not available.
    pub const fn windows(&self) -> Option<&LocalWindowsFileSystemObservation> {
        self.windows.as_ref()
    }
}
