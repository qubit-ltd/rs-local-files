// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::path::PathBuf;

use crate::LocalFileError;

/// Lossless persistence failure facts; the owner remains in the same managed
/// resource.
#[derive(Debug)]
pub struct LocalTempPersistFailure {
    /// Native cause.
    /// Native failure that stopped persistence.
    pub error: LocalFileError,
    /// Requested target.
    /// Destination supplied by the caller before authority resolution.
    pub requested_target: PathBuf,
    /// Resolved target if resolution completed.
    /// Destination resolved under captured authority, when resolution
    /// succeeded.
    pub resolved_target: Option<PathBuf>,
    /// Stage where persistence stopped.
    /// Persistence phase active when the failure occurred.
    pub stage: crate::outcome::LocalPersistStage,
    /// Publication state established by the attempt.
    /// Publication state observed after the failed attempt.
    pub state: crate::outcome::LocalPersistFailureState,
    /// Source authority state observed at failure.
    pub source_state: crate::outcome::LocalTempSourceState,
}
