// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Recursive-copy failure stages.

/// Stage at which a recursive directory copy failed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LocalCopyDirStage {
    /// Inspecting or canonicalizing the source directory failed.
    InspectSource,
    /// Inspecting a source directory entry failed or found an unsupported type.
    InspectSourceEntry,
    /// Reading a source directory failed.
    ReadSourceDirectory,
    /// Preparing or creating a destination entry failed.
    PrepareDestination,
    /// Copying regular-file contents failed.
    CopyFileContents,
    /// Applying source permissions to a destination entry failed.
    PreservePermissions,
    /// Committing a staged file to its destination failed.
    CommitFile,
}
