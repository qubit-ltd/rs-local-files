// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

/// Finite limits for the local worker group and its owned resources.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LocalExecutionConfig {
    /// Number of local I/O workers.
    pub worker_count: usize,
    /// Maximum queued ordinary jobs.
    pub queue_capacity: usize,
    /// Maximum jobs waiting for queue capacity.
    pub waiter_capacity: usize,
    /// Total owned in-memory payload capacity.
    pub max_owned_bytes: usize,
    /// Maximum simultaneously retained native handles.
    pub max_open_handles: usize,
    /// Maximum simultaneously reserved temporary disk bytes.
    pub max_disk_bytes: u64,
    /// Maximum live fixed-size local fact snapshots.
    pub max_fact_records: usize,
}

impl Default for LocalExecutionConfig {
    /// Runs the default behavior while preserving the operation lifecycle
    /// invariants.
    fn default() -> Self {
        Self {
            worker_count: 4,
            queue_capacity: 64,
            waiter_capacity: 64,
            max_owned_bytes: 16 * 1024 * 1024,
            max_open_handles: 256,
            max_disk_bytes: 1024 * 1024 * 1024,
            max_fact_records: 64,
        }
    }
}
