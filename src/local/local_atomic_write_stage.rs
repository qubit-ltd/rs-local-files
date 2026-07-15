// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Atomic-write failure stages.

/// Stage at which an atomic write failed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LocalAtomicWriteStage {
    /// Creating destination parent directories failed.
    PrepareParent,
    /// Inspecting an existing destination failed.
    InspectDestination,
    /// Creating the same-directory temporary file failed.
    CreateTemporaryFile,
    /// Caller-provided temporary-file writing failed.
    WriteTemporaryFile,
    /// Applying existing destination permissions failed.
    PreservePermissions,
    /// Synchronizing the temporary file failed.
    SyncTemporaryFile,
    /// Replacing the destination failed.
    ReplaceDestination,
    /// Synchronizing the parent directory after replacement failed.
    SyncParentDirectory,
}
