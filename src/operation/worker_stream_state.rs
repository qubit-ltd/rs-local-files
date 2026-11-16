// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::collections::HashMap;

use super::worker_stream_command::WorkerCommand;

/// Outstanding completions indexed by stream request identifier.
pub(super) struct WorkerStreamState {
    /// Outstanding command completions, keyed by stable request ID.
    pub(super) commands: HashMap<u64, WorkerCommand>,
}
