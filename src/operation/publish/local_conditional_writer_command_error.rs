// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use qubit_operation::ResourceId;

use crate::LocalFileError;

#[cfg(feature = "digest")]
/// Structured conditional writer failure preserving open, stream, and commit
/// errors.
#[derive(Debug)]
pub enum LocalConditionalWriterCommandError<D: digest::Digest + Send + 'static> {
    /// Scope capacity for retryable writer cleanup was unavailable.
    ResourceCapacity(qubit_operation::ResourceCapacityError),
    /// The cleanup resource could not be committed to the operation scope.
    ResourceCommit(qubit_operation::ResourceManageErrorKind),
    /// Opening the candidate writer failed.
    Open(LocalFileError),
    /// Writing/flushing the candidate failed.
    Write {
        cause: LocalFileError,
        cleanup_error: Option<LocalFileError>,
        recovery_resource: Option<ResourceId>,
        retryable_writer: Option<crate::LocalConditionalWriter<D>>,
    },
    /// Conditional publication failed with native recovery state.
    Commit(crate::error::LocalConditionalCommitError),
}
