// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use super::LocalExecutorConfigError;
/// Failure to create the local worker group.
#[derive(Debug)]
pub enum LocalExecutorCreateError {
    /// The supplied configuration contains a zero capacity.
    InvalidConfig(
        /// Configuration validation failure containing the rejected limit.
        LocalExecutorConfigError,
    ),
    /// A worker thread could not be started.
    WorkerStart(
        /// Operating-system error returned while spawning a worker thread.
        std::io::Error,
    ),
}

impl std::fmt::Display for LocalExecutorCreateError {
    /// Runs the fmt behavior while preserving the operation lifecycle
    /// invariants.
    /// # Parameters
    /// - `formatter` — supplies data or policy for this operation.
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidConfig(error) => {
                write!(formatter, "invalid local executor config: {error:?}")
            }
            Self::WorkerStart(error) => {
                write!(formatter, "failed to start local I/O worker: {error}")
            }
        }
    }
}

impl std::error::Error for LocalExecutorCreateError {
    /// Runs the source behavior while preserving the operation lifecycle
    /// invariants.
    ///
    /// # Parameters
    /// - `std` — input value used by this operation.
    /// - `error` — input value used by this operation.
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::InvalidConfig(_) => None,
            Self::WorkerStart(error) => Some(error),
        }
    }
}
