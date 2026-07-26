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
/// use qubit_local_files::copy::Stage;
///
/// fn classify(stage: Stage) {
///     match stage {
///         Stage::InspectSource => {}
///         Stage::InspectSourceEntry => {}
///         Stage::ReadSourceDirectory => {}
///         Stage::PrepareDestination => {}
///         Stage::CopyFileContents => {}
///         Stage::PreservePermissions => {}
///         Stage::CommitFile => {}
///         Stage::CleanupTemporaryFile => {}
///         Stage::UpdateStatistics => {}
///     }
/// }
/// ```
///
/// ```compile_fail
/// #![deny(unused_must_use)]
/// use qubit_local_files::copy::Stage;
///
/// Stage::InspectSource.clone();
/// ```
#[must_use]
#[non_exhaustive]
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
    /// Removing an uncommitted staging file after a skipped copy failed.
    CleanupTemporaryFile,
    /// Updating exact recursive-copy statistics overflowed.
    UpdateStatistics,
}
