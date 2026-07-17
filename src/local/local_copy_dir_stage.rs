// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Recursive-copy failure stages.

/// Stage at which a recursive directory copy failed.
///
/// Additional stages may be added as copy diagnostics evolve. Downstream
/// matches must retain a wildcard arm.
///
/// ```compile_fail
/// use qubit_local_files::LocalCopyDirStage;
///
/// fn classify(stage: LocalCopyDirStage) {
///     match stage {
///         LocalCopyDirStage::InspectSource => {}
///         LocalCopyDirStage::InspectSourceEntry => {}
///         LocalCopyDirStage::ReadSourceDirectory => {}
///         LocalCopyDirStage::PrepareDestination => {}
///         LocalCopyDirStage::CopyFileContents => {}
///         LocalCopyDirStage::PreservePermissions => {}
///         LocalCopyDirStage::CommitFile => {}
///         LocalCopyDirStage::CleanupTemporaryFile => {}
///         LocalCopyDirStage::UpdateStatistics => {}
///     }
/// }
/// ```
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
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
    /// Removing an uncommitted staging file after a skipped copy failed.
    CleanupTemporaryFile,
    /// Updating exact recursive-copy statistics overflowed.
    UpdateStatistics,
}
