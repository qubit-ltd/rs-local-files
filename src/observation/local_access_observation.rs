// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use super::LocalUnixAccessObservation;
use super::LocalWindowsAccessObservation;

#[derive(Clone, Debug, Eq, PartialEq)]
/// Platform-specific access-control facts.
pub struct LocalAccessObservation {
    unix: Option<LocalUnixAccessObservation>,
    windows: Option<LocalWindowsAccessObservation>,
}
impl LocalAccessObservation {
    #[allow(dead_code)] // Constructed by the platform query backends in T03/T04.
    pub(crate) const fn new(
        unix: Option<LocalUnixAccessObservation>,
        windows: Option<LocalWindowsAccessObservation>,
    ) -> Self {
        Self { unix, windows }
    }
    /// Returns Unix access facts when observed on Unix, or `None` on other
    /// platforms where these facts are not available.
    pub const fn unix(&self) -> Option<&LocalUnixAccessObservation> {
        self.unix.as_ref()
    }
    /// Returns Windows access facts when observed on Windows, or `None` on
    /// other platforms where these facts are not available.
    pub const fn windows(&self) -> Option<&LocalWindowsAccessObservation> {
        self.windows.as_ref()
    }
}
