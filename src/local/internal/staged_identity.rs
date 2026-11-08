// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Platform-specific identity tokens for guarded staging-file cleanup.

/// Identifies a staging file by the operating-system identity of its inode or
/// file record, rather than by its mutable path alone.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum StagedIdentity {
    /// Unix device and inode numbers.
    #[cfg(unix)]
    Unix(
        /// Device containing the file.
        u64,
        /// Inode number within that device.
        u64,
    ),
    /// Windows volume and file index.
    #[cfg(windows)]
    Windows(
        /// Serial number of the containing volume.
        u32,
        /// File index within the volume.
        u64,
    ),
}
