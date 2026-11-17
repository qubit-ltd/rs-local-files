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
    /// The operation could not reserve capacity for retaining a failed
    /// candidate writer.
    ResourceCapacity(
        /// Scope capacity admission failure for retaining candidate cleanup
        /// ownership.
        qubit_operation::ResourceCapacityError,
    ),
    /// Recovery ownership could not be registered, so the operation cannot
    /// promise scoped retry cleanup.
    ResourceCommit(
        /// Failure kind returned while registering retry cleanup in the
        /// operation scope.
        qubit_operation::ResourceManageErrorKind,
    ),
    /// The native filesystem rejected opening the destination candidate before
    /// any bytes were written.
    Open(
        /// Native failure returned while opening the destination candidate.
        LocalFileError,
    ),
    /// Writing or flushing failed; a retryable writer is retained if abort
    /// cleanup failed.
    Write {
        /// Native write or flush failure.
        cause: LocalFileError,
        /// Failure encountered while aborting the candidate, if any.
        cleanup_error: Option<Box<LocalFileError>>,
        /// Candidate writer retained when cleanup can be retried.
        retryable_writer: Option<Box<crate::LocalFileWriter>>,
        /// Operation-scope identifier for the registered recovery owner.
        recovery_resource: Option<ResourceId>,
    },
    /// Atomic publication failed; the wrapped commit error preserves
    /// destination state and retry ownership.
    Commit(
        /// Native commit failure and its publication/retry recovery facts.
        LocalFileCommitError,
    ),
}
