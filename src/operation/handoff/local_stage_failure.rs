// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use super::super::temp::LocalCreateFailure;

/// Failure after a stage operation has been accepted.
#[derive(Debug)]
pub enum LocalStageFailure {
    /// Required execution capacity was unavailable before the child operation.
    Capacity,
    /// The temporary-file creation operation failed with this cause.
    Child(
        /// Creation failure returned by the child temporary-file operation.
        LocalCreateFailure,
    ),
    /// The child operation completed without returning a value or failure.
    ChildOutcome,
    /// A serialized native-resource command could not complete.
    Command(
        /// Worker scheduling or native command diagnostic.
        String,
    ),
    /// The local filesystem rejected creation, writing, or sealing.
    Local(
        /// Filesystem failure returned by a local operation.
        crate::LocalFileError,
    ),
    /// The operating system rejected an exported-handle operation.
    Io(
        /// Operating-system failure returned while cloning or writing a handle.
        std::io::Error,
    ),
    /// Sealing failed and access may have left the resource in this state.
    Seal {
        /// Native error returned by the seal transition.
        cause: crate::LocalFileError,
        /// Whether the managed file remains usable after the failure.
        access_state: crate::outcome::LocalTempAccessState,
    },
    /// Payload length exceeded the configured stage limit.
    ContentTooLarge,
    /// The operation was stopped before the stage could return a lease.
    Stopped,
    /// Rooted handle export was not explicitly enabled at staging time.
    RootedExportDenied,
    /// The resource is not in the sealed-file state required for export.
    NotSealed,
    /// Cleanup retained the resource after the operation failed.
    Retained {
        /// Resource manager's reason for retaining ownership.
        reason: qubit_operation::ResourceManageErrorKind,
        /// Stable identity of the retained resource.
        resource_id: qubit_operation::ResourceId,
    },
}
