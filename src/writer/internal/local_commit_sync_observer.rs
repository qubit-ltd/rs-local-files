// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;

/// Counts file and parent-directory synchronization operations for local
/// commits.
#[derive(Debug, Default)]
pub(crate) struct LocalCommitSyncObserver {
    /// Number of file synchronization operations recorded.
    file_sync_calls: AtomicUsize,
    /// Number of parent-directory synchronization operations recorded.
    parent_sync_calls: AtomicUsize,
}

impl LocalCommitSyncObserver {
    /// Returns the number of recorded file synchronization operations.
    #[cfg(test)]
    pub(crate) fn file_sync_calls(&self) -> usize {
        self.file_sync_calls.load(Ordering::Relaxed)
    }

    /// Returns the number of recorded parent-directory synchronization
    /// operations.
    #[cfg(test)]
    pub(crate) fn parent_sync_calls(&self) -> usize {
        self.parent_sync_calls.load(Ordering::Relaxed)
    }

    /// Records one file synchronization operation.
    #[inline]
    pub(crate) fn record_file_sync(&self) {
        self.file_sync_calls.fetch_add(1, Ordering::Relaxed);
    }

    /// Records one parent-directory synchronization operation.
    #[inline]
    pub(crate) fn record_parent_sync(&self) {
        self.parent_sync_calls.fetch_add(1, Ordering::Relaxed);
    }
}
