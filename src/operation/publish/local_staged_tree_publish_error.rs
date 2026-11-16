// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use crate::LocalFileError;

use super::LocalStagedTreeRecovery;
use super::LocalStagedTreeReservedError;

/// Errors keep the stage or native recovery owners attached for retry/cleanup.
#[derive(Debug)]
pub enum LocalStagedTreePublishError {
    /// Parent authority could not be opened.
    Parent(LocalFileError),
    /// Stage creation failed with its native cleanup ownership.
    Create(LocalStagedTreeReservedError<crate::directory::LocalTreeCreateError>),
    /// A staged file could not be created; the stage remains available for
    /// cleanup.
    Write {
        /// Failure while creating or writing a staged file.
        cause: LocalFileError,
        /// Unpublished stage retained for caller-directed cleanup.
        stage: LocalStagedTreeRecovery,
    },
    /// Verification or publication failed with the existing native recovery
    /// report.
    Publish(LocalStagedTreeReservedError<crate::directory::LocalTreePublishError>),
    /// A cancellation cleanup failed and retains the cleanup owner.
    Cleanup(LocalStagedTreeReservedError<crate::directory::LocalTreeCleanupError>),
}
