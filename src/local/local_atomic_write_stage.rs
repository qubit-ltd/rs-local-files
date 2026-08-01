// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Atomic-write failure stages.

/// Stage at which an atomic write failed.
///
/// Additional stages may be added as durability behavior evolves. Downstream
/// matches must retain a wildcard arm.
///
/// ```compile_fail
/// use qubit_local_files::atomic::Stage;
///
/// fn classify(stage: Stage) {
///     match stage {
///         Stage::PrepareParent => {}
///         Stage::InspectDestination => {}
///         Stage::CreateTemporaryFile => {}
///         Stage::WriteTemporaryFile => {}
///         Stage::ReadDestinationMetadata => {}
///         Stage::ApplyDestinationMetadata => {}
///         Stage::SyncTemporaryFile => {}
///         Stage::ReplaceDestination => {}
///         Stage::CleanupTemporaryFile => {}
///         Stage::SyncParent => {}
///     }
/// }
/// ```
///
/// ```compile_fail
/// #![deny(unused_must_use)]
/// use qubit_local_files::atomic::Stage;
///
/// Stage::InspectDestination.clone();
/// ```
#[must_use]
#[non_exhaustive]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[allow(dead_code)]
pub enum LocalAtomicWriteStage {
    /// Creating destination parent directories failed.
    PrepareParent,
    /// Inspecting an existing destination failed.
    InspectDestination,
    /// Creating the same-directory temporary file failed.
    CreateTemporaryFile,
    /// Caller-provided temporary-file writing failed.
    WriteTemporaryFile,
    /// Reading metadata from an existing destination failed.
    ReadDestinationMetadata,
    /// Applying existing destination metadata to staging failed.
    ApplyDestinationMetadata,
    /// Synchronizing the temporary file failed.
    SyncTemporaryFile,
    /// Replacing the destination failed.
    ReplaceDestination,
    /// Explicitly removing an aborted temporary file failed.
    CleanupTemporaryFile,
    /// Synchronizing the parent directory after replacement failed.
    SyncParent,
}
