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
    pub error: LocalFileError,
    /// Requested target.
    pub requested_target: PathBuf,
    /// Resolved target if resolution completed.
    pub resolved_target: Option<PathBuf>,
    /// Stage where persistence stopped.
    pub stage: crate::outcome::LocalPersistStage,
    /// Publication state established by the attempt.
    pub state: crate::outcome::LocalPersistFailureState,
    /// Source authority state observed at failure.
    pub source_state: crate::outcome::LocalTempSourceState,
}
