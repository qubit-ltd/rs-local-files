// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

/// Managed command failure, preserving native errors and worker admission
/// state.
#[derive(Debug)]
pub enum LocalTempLifecycleError {
    /// Structured persistence failure.
    Persist(LocalTempPersistFailure),
    /// Structured sealing failure.
    Seal {
        /// Native cause.
        error: LocalFileError,
        /// Access state after the attempted seal.
        access_state: crate::LocalTempAccessState,
    },
    /// The resource kind does not support this command.
    Unsupported(LocalFileError),
    /// The bounded local worker could not accept or complete the command.
    Worker(LocalTempWorkerCommandFailure),
    /// The operation was stopped before the native transition began.
    Stopped,
}
