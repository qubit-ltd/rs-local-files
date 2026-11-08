// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Destination policies used while committing a staged regular-file copy.

use crate::LocalCopyConflictPolicy;
use crate::LocalSymlinkPolicy;

/// Carries destination policies from staging to the commit operation.
pub(super) struct StagedCopyCommitOptions {
    /// Policy applied when the destination file already exists.
    pub(super) conflict: LocalCopyConflictPolicy,
    /// Whether a conflicting destination directory may be removed at commit.
    pub(super) remove_destination_directory: bool,
    /// Policy applied to symbolic links encountered during destination work.
    pub(super) symlink_policy: LocalSymlinkPolicy,
}
