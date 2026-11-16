// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use qubit_operation::ResourceId;

use crate::LocalFileError;

/// Structured conditional writer failure preserving open, stream, and commit
/// errors.
///
/// # Type Parameters
/// * `D` — digest algorithm used by the retryable conditional writer.
#[cfg(feature = "digest")]
#[derive(Debug)]
pub enum LocalConditionalWriterCommandError<D: digest::Digest + Send + 'static> {
    /// The operation could not reserve capacity for retaining a failed candidate writer.
    ResourceCapacity(
        /// Scope capacity admission failure for retaining candidate cleanup ownership.
        qubit_operation::ResourceCapacityError,
    ),
    /// Recovery ownership could not be registered, so the operation cannot promise scoped retry cleanup.
    ResourceCommit(
        /// Failure kind returned while registering retry cleanup in the operation scope.
        qubit_operation::ResourceManageErrorKind,
    ),
    /// The native filesystem rejected opening the candidate before any candidate bytes were written.
    Open(
        /// Native failure returned while opening the conditional candidate.
        LocalFileError,
    ),
    /// Writing/flushing the candidate failed.
    Write {
        /// Native write or flush failure.
        cause: LocalFileError,
        /// Failure encountered while aborting the candidate, if any.
        cleanup_error: Option<LocalFileError>,
        /// Operation-scope resource identifier for retryable cleanup, if registered.
        recovery_resource: Option<ResourceId>,
        /// Candidate writer retained when cleanup can be retried.
        retryable_writer: Option<crate::LocalConditionalWriter<D>>,
    },
    /// The native conditional commit rejected the precondition or failed during atomic replacement.
    Commit(
        /// Native conditional commit result and its publication recovery state.
        crate::error::LocalConditionalCommitError,
    ),
}
