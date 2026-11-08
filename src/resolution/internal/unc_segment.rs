// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

/// Records whether one consumed Windows UNC prefix segment is a marker.
pub(in crate::resolution) struct UncSegment {
    /// Whether this segment is the `?` marker used by extended paths.
    pub(in crate::resolution) is_extended_marker: bool,
    /// Whether this segment spells `UNC`, case-insensitively.
    pub(in crate::resolution) is_unc_marker: bool,
}
