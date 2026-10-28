// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[must_use]
/// Exact owner permissions to apply and verify on one supported platform.
///
/// A new policy has no platform configuration and cannot be used until the
/// consuming operation supplies valid settings for its platform.
pub struct LocalOwnerAccessPolicy {
    unix_modes: Option<(u32, u32)>,
    windows_masks: Option<(u32, u32)>,
}
impl LocalOwnerAccessPolicy {
    /// Creates an unconfigured policy without implicit permission presets.
    pub const fn new() -> Self {
        Self {
            unix_modes: None,
            windows_masks: None,
        }
    }
    /// Sets exact Unix directory and file modes; only owner `rwx` bits are
    /// valid.
    pub const fn with_unix_modes(mut self, directory_mode: u32, file_mode: u32) -> Self {
        self.unix_modes = Some((directory_mode, file_mode));
        self
    }
    /// Sets exact Windows directory and file access masks.
    ///
    /// Each mask may contain only the FILE-specific rights bits `0x0000_01ff`
    /// and standard object rights `DELETE` (`0x0001_0000`), `READ_CONTROL`
    /// (`0x0002_0000`), `WRITE_DAC` (`0x0004_0000`), `WRITE_OWNER`
    /// (`0x0008_0000`), and `SYNCHRONIZE` (`0x0010_0000`), whose union is
    /// `0x001f_01ff`. `GENERIC_*`, `MAXIMUM_ALLOWED`,
    /// `ACCESS_SYSTEM_SECURITY`, and all unknown bits are rejected.
    ///
    /// This builder stores the requested values without validating them.
    /// The consuming owner-policy operation validates the masks before it
    /// mutates any native object and reports invalid values as
    /// [`crate::error::LocalFileErrorKind::InvalidOptions`].
    pub const fn with_windows_masks(mut self, directory_mask: u32, file_mask: u32) -> Self {
        self.windows_masks = Some((directory_mask, file_mask));
        self
    }
    pub(crate) const fn unix_modes(&self) -> Option<(u32, u32)> {
        self.unix_modes
    }
    #[cfg(windows)]
    #[allow(dead_code)] // Consumed by the Windows owner-policy backend in T06.
    pub(crate) const fn windows_masks(&self) -> Option<(u32, u32)> {
        self.windows_masks
    }
}
