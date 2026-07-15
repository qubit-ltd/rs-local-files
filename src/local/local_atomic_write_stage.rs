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
/// use qubit_local_files::LocalAtomicWriteStage;
///
/// fn classify(stage: LocalAtomicWriteStage) {
///     match stage {
///         LocalAtomicWriteStage::PrepareParent => {}
///         LocalAtomicWriteStage::InspectDestination => {}
///         LocalAtomicWriteStage::CreateTemporaryFile => {}
///         LocalAtomicWriteStage::WriteTemporaryFile => {}
///         LocalAtomicWriteStage::PreservePermissions => {}
///         LocalAtomicWriteStage::SyncTemporaryFile => {}
///         LocalAtomicWriteStage::ReplaceDestination => {}
///         LocalAtomicWriteStage::SyncParentDirectory => {}
///     }
/// }
/// ```
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[non_exhaustive]
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
