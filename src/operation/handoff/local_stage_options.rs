// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

/// Bounds and controls for one staged payload.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LocalStageOptions {
    /// Maximum accepted payload length.
    pub max_content_bytes: usize,
    /// Disk reservation retained for the staged file.
    pub disk_reservation_bytes: u64,
    /// Privacy requested while creating the host temporary resource.
    pub privacy: crate::policy::LocalPrivacyRequirement,
    /// File synchronization requested when sealing the staged file.
    pub file_sync: crate::policy::LocalFileSyncRequirement,
    /// Explicitly permits exporting a Rooted resource's read-only handle.
    /// Defaults to false; no namespace or host path is exposed.
    pub export_read_handle: bool,
}

impl Default for LocalStageOptions {
    /// Selects bounded defaults that require private creation and file sync.
    fn default() -> Self {
        Self {
            max_content_bytes: 64 * 1024 * 1024,
            disk_reservation_bytes: 64 * 1024 * 1024,
            privacy: crate::policy::LocalPrivacyRequirement::Required,
            file_sync: crate::policy::LocalFileSyncRequirement::Required,
            export_read_handle: false,
        }
    }
}
