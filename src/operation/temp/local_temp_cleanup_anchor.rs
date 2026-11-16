// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

/// Cleanup-only owner retained when a consuming File or Directory transition
/// unwinds. Each variant preserves its native Host/Rooted identity authority.
pub(crate) enum LocalTempCleanupAnchor {
    /// Cleanup authority for a file disarmed during a consuming transition.
    File(
        /// Disarmed native file cleanup authority.
        crate::LocalTempFile,
    ),
    /// Cleanup authority for a directory disarmed during a consuming
    /// transition.
    Directory(
        /// Disarmed native directory cleanup authority.
        crate::LocalTempDirectory,
    ),
}
