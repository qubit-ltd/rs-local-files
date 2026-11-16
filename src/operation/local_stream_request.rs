// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use super::LocalStreamCommand;
/// Stable identity and owned payload for one command.
///
/// The identifier remains unchanged while a pending command is polled again.
///
/// # Examples
///
/// ```
/// use qubit_local_files::operation::stream::LocalStreamCommand;
/// use qubit_local_files::operation::stream::LocalStreamRequest;
///
/// let request = LocalStreamRequest {
///     id: 7,
///     command: LocalStreamCommand::Flush,
/// };
/// assert_eq!(request.id, 7);
/// ```
#[derive(Debug, Clone)]
pub struct LocalStreamRequest {
    /// Identifier used to correlate retries and worker completion.
    pub id: u64,
    /// Operation and owned payload sent to the worker.
    pub command: LocalStreamCommand,
}
