// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

/// Finite limits shared by a local I/O worker group and its retained resources.
///
/// All fields must be nonzero. These executor-wide ceilings cannot be raised
/// by per-operation options. Queue and waiter permits are acquired at
/// submission and held until a worker starts the job; both capacities
/// contribute to the operation group's admission capacity. Byte, handle, disk,
/// and fact limits account for resources retained by accepted operations.
/// Cleanup queue permits use a separate pool with the configured
/// `max_open_handles` capacity; they do not share the ordinary handle counter.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LocalExecutionConfig {
    /// Number of worker threads servicing ordinary I/O and cleanup jobs.
    pub worker_count: usize,
    /// Number of ordinary-job queue permits available to accepted work.
    pub queue_capacity: usize,
    /// Additional ordinary-job permits available while the queue is full.
    pub waiter_capacity: usize,
    /// Maximum combined capacity reserved for operation-owned in-memory
    /// payloads, in bytes.
    pub max_owned_bytes: usize,
    /// Maximum ordinary native handles reserved at once. The cleanup queue has
    /// a separate reservation pool of the same capacity.
    pub max_open_handles: usize,
    /// Maximum temporary disk bytes reserved at once.
    pub max_disk_bytes: u64,
    /// Maximum number of live local fact snapshots.
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
