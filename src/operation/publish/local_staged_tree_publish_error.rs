// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use super::LocalStagedTreeRecovery;
use super::LocalStagedTreeReservedError;
use crate::LocalFileError;

/// Errors keep the stage or native recovery owners attached for retry/cleanup.
#[derive(Debug)]
pub enum LocalStagedTreePublishError {
    /// Opening the requested parent under captured authority failed before a stage was created.
    Parent(LocalFileError),
    /// Native stage creation failed and returns its cleanup owner with the disk reservation.
    Create(LocalStagedTreeReservedError<crate::directory::LocalTreeCreateError>),
    /// A staged file could not be created; the stage remains available for
    /// cleanup.
    Write {
        /// Failure while creating or writing a staged file.
        cause: LocalFileError,
        /// Unpublished stage retained for caller-directed cleanup.
        stage: LocalStagedTreeRecovery,
    },
    /// Native verification or atomic installation failed; its report retains the exact recovery state.
    Publish(LocalStagedTreeReservedError<crate::directory::LocalTreePublishError>),
    /// Cleanup after cancellation failed, leaving a native cleanup owner for retry.
    Cleanup(LocalStagedTreeReservedError<crate::directory::LocalTreeCleanupError>),
}
