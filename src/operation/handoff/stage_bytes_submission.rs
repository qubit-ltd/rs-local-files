// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use qubit_operation::OperationOptions;

use super::LocalStageOptions;
use crate::operation::internal::permits::DiskPermit;
use crate::operation::internal::permits::Permit;

/// Resources transferred to the accepted byte-staging operation.
pub(super) struct StageBytesSubmission {
    /// Child operation cancellation and deadline policy.
    pub(super) operation: OperationOptions,
    /// Owned payload transferred to the worker.
    pub(super) bytes: Vec<u8>,
    /// Disk, privacy, and export policy for the stage.
    pub(super) stage: LocalStageOptions,
    /// Reservation for the input allocation.
    pub(super) owned: Permit,
    /// Reservation for staged disk capacity beyond the native owner byte.
    pub(super) disk: DiskPermit,
    /// Native temporary-file creation policy.
    pub(super) temp_options: crate::options::LocalTempFileOptions,
}
