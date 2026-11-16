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
    /// Persistence failed; requested and resolved target facts remain available in the payload.
    Persist(LocalTempPersistFailure),
    /// Structured sealing failure.
    Seal {
        /// Native cause.
        /// Native sealing failure.
        error: LocalFileError,
        /// Access state after the attempted seal.
        /// Access capabilities that remain after the failed transition.
        access_state: crate::LocalTempAccessState,
    },
    /// This command cannot be applied to the managed resource kind, with the native rejection attached.
    Unsupported(LocalFileError),
    /// Worker admission or execution failed before the lifecycle result could be returned.
    Worker(LocalTempWorkerCommandFailure),
    /// The operation was stopped before the native transition began.
    Stopped,
}
