// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use qubit_operation::ResourceId;

use crate::LocalFileCommitError;
use crate::LocalFileError;

/// Structured errors retain the native writer's original errors and recovery
/// owner.
#[derive(Debug)]
pub enum LocalWriterCommandError {
    /// Scope capacity for retryable writer cleanup was unavailable.
    ResourceCapacity(qubit_operation::ResourceCapacityError),
    /// The cleanup resource could not be committed to the operation scope.
    ResourceCommit(qubit_operation::ResourceManageErrorKind),
    /// Opening the writer failed before any write was attempted.
    Open(LocalFileError),
    /// Writing or flushing failed; a retryable writer is retained if abort
    /// cleanup failed.
    Write {
        cause: LocalFileError,
        cleanup_error: Option<LocalFileError>,
        retryable_writer: Option<crate::LocalFileWriter>,
        recovery_resource: Option<ResourceId>,
    },
    /// Commit failed, retaining its publication state and retryable writer.
    Commit(LocalFileCommitError),
}
