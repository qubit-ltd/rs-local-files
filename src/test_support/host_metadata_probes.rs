// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Thread-local library probe counts, not operating-system syscall counts.

use std::cell::Cell;

thread_local! {
    /// Prefix inspections and final metadata queries on the current thread.
    static COUNTS: Cell<(usize, usize)> = const { Cell::new((0, 0)) };
}

/// Clears only the calling thread's Host metadata probe counters.
pub fn reset_host_metadata_probe_counts() {
    COUNTS.set((0, 0));
}

/// Returns this thread's `(prefix probes, final metadata queries)`.
///
/// Counts library query boundaries, including failed queries. Native OS work
/// within `std::fs` is not counted. Other threads and Rooted operations do not
/// contribute to these counters.
pub fn host_metadata_probe_counts() -> (usize, usize) {
    COUNTS.get()
}

/// Records one Host policy-driven prefix inspection on the current thread.
pub(crate) fn record_host_prefix_probe() {
    let (prefix, final_queries) = COUNTS.get();
    COUNTS.set((prefix + 1, final_queries));
}

/// Records one final Host metadata query on the current thread.
pub(crate) fn record_host_metadata_query() {
    let (prefix, final_queries) = COUNTS.get();
    COUNTS.set((prefix, final_queries + 1));
}
