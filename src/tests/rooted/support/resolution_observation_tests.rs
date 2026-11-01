// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Thread-local primitive observations used only by library test builds.

use std::cell::Cell;
use std::time::Instant;

thread_local! {
    static METADATA: Cell<usize> = const { Cell::new(0) };
    static DIRECTORY_OPEN: Cell<usize> = const { Cell::new(0) };
    static FALLBACK: Cell<usize> = const { Cell::new(0) };
    #[cfg(unix)]
    static NATIVE_METADATA: Cell<usize> = const { Cell::new(0) };
    #[cfg(unix)]
    static NATIVE_DIRECTORY_OPEN: Cell<usize> = const { Cell::new(0) };
    static DEADLINE_CHECKS: Cell<usize> = const { Cell::new(0) };
    static EXPIRE_AFTER: Cell<Option<usize>> = const { Cell::new(None) };
    static EXPIRE_AT: Cell<Option<DeadlineBoundary>> = const { Cell::new(None) };
    static FORCED_EXPIRED: Cell<bool> = const { Cell::new(false) };
    static BOUNDARY_METADATA: Cell<Option<usize>> = const { Cell::new(None) };
    static HOST_INSTALL_BOUNDARIES: Cell<usize> = const { Cell::new(0) };
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum DeadlineBoundary {
    SourceResolved,
    TargetResolved,
    FollowedLinkResolved,
    DestinationRemoved,
    HostCopyDataFinished,
    HostCopyBeforeInstall,
    HostCopyBeforeDirectoryMetadata,
    HostCopyAfterConflictRemoval,
    HostCopySecondBeforeInstall,
    HostTreeSymlinkTargetRead,
    HostTreeSymlinkDestinationRemoved,
    ObservedReaderBeforeContentResolution,
    ObservedReaderAfterMetadataBeforeContentOpen,
}

pub(crate) fn resolution_now(deadline: Option<Instant>) -> Instant {
    DEADLINE_CHECKS.set(DEADLINE_CHECKS.get() + 1);
    if FORCED_EXPIRED.get() || EXPIRE_AFTER.get().is_some_and(|limit| DEADLINE_CHECKS.get() > limit) {
        deadline.unwrap_or_else(Instant::now)
    } else {
        Instant::now()
    }
}

pub(crate) fn copy_budget_now(deadline: Option<Instant>) -> Instant {
    resolution_now(deadline)
}

#[cfg(test)]
#[test]
fn test_deadline_clock_guard_restores_hook_after_drop_and_unwind() {
    let deadline = Instant::now() + std::time::Duration::from_secs(60);
    {
        let _clock = DeadlineClockGuard::expire_after(0);
        assert_eq!(deadline, resolution_now(Some(deadline)));
    }
    assert!(resolution_now(Some(deadline)) < deadline);

    let unwind = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _clock = DeadlineClockGuard::expire_after(0);
        assert_eq!(deadline, resolution_now(Some(deadline)));
        panic!("exercise deadline clock guard unwind");
    }));
    assert!(unwind.is_err());
    assert!(resolution_now(Some(deadline)) < deadline);
}

pub(crate) struct DeadlineClockGuard {
    previous_after: Option<usize>,
    previous_boundary: Option<DeadlineBoundary>,
    previous_expired: bool,
    previous_boundary_metadata: Option<usize>,
    previous_host_install_boundaries: usize,
}

impl DeadlineClockGuard {
    pub(crate) fn expire_after(checks: usize) -> Self {
        let guard = Self::new();
        EXPIRE_AFTER.set(Some(checks));
        guard
    }

    pub(crate) fn expire_at(boundary: DeadlineBoundary) -> Self {
        let guard = Self::new();
        EXPIRE_AT.set(Some(boundary));
        guard
    }

    fn new() -> Self {
        let previous_after = EXPIRE_AFTER.replace(None);
        let previous_boundary = EXPIRE_AT.replace(None);
        let previous_expired = FORCED_EXPIRED.replace(false);
        let previous_boundary_metadata = BOUNDARY_METADATA.replace(None);
        let previous_host_install_boundaries = HOST_INSTALL_BOUNDARIES.replace(0);
        DEADLINE_CHECKS.set(0);
        Self {
            previous_after,
            previous_boundary,
            previous_expired,
            previous_boundary_metadata,
            previous_host_install_boundaries,
        }
    }
}

impl Drop for DeadlineClockGuard {
    fn drop(&mut self) {
        EXPIRE_AFTER.set(self.previous_after);
        EXPIRE_AT.set(self.previous_boundary);
        FORCED_EXPIRED.set(self.previous_expired);
        BOUNDARY_METADATA.set(self.previous_boundary_metadata);
        HOST_INSTALL_BOUNDARIES.set(self.previous_host_install_boundaries);
        DEADLINE_CHECKS.set(0);
    }
}

pub(crate) fn reach_deadline_boundary(boundary: DeadlineBoundary) {
    if boundary == DeadlineBoundary::HostCopyBeforeInstall {
        HOST_INSTALL_BOUNDARIES.set(HOST_INSTALL_BOUNDARIES.get() + 1);
    }
    let reached = EXPIRE_AT.get() == Some(boundary)
        || (boundary == DeadlineBoundary::HostCopyBeforeInstall
            && EXPIRE_AT.get() == Some(DeadlineBoundary::HostCopySecondBeforeInstall)
            && HOST_INSTALL_BOUNDARIES.get() == 2);
    if reached {
        BOUNDARY_METADATA.set(Some(METADATA.get()));
        FORCED_EXPIRED.set(true);
    }
}

pub(crate) fn metadata_count_at_boundary() -> Option<usize> {
    BOUNDARY_METADATA.get()
}

/// Records a metadata attempt at the cursor's native operation boundary.
pub(crate) fn record_metadata() {
    METADATA.set(METADATA.get() + 1);
}
/// Records a directory-open attempt at the cursor's native operation boundary.
pub(crate) fn record_directory_open() {
    DIRECTORY_OPEN.set(DIRECTORY_OPEN.get() + 1);
}
/// Records entry to the general resolver, which must not serve normal paths.
pub(crate) fn record_fallback() {
    FALLBACK.set(FALLBACK.get() + 1);
}
/// Records an actual Unix no-follow metadata syscall.
#[cfg(unix)]
pub(crate) fn record_native_metadata() {
    NATIVE_METADATA.set(NATIVE_METADATA.get() + 1);
}
/// Records an actual Unix rooted directory-open syscall.
#[cfg(unix)]
pub(crate) fn record_native_directory_open() {
    NATIVE_DIRECTORY_OPEN.set(NATIVE_DIRECTORY_OPEN.get() + 1);
}
/// Clears only this test thread's observations before a complete resolver call.
pub(crate) fn reset() {
    METADATA.set(0);
    DIRECTORY_OPEN.set(0);
    FALLBACK.set(0);
    #[cfg(unix)]
    {
        NATIVE_METADATA.set(0);
        NATIVE_DIRECTORY_OPEN.set(0);
    }
}

/// Returns deadline checks observed by the current test thread.
#[must_use]
pub(crate) fn deadline_check_count() -> usize {
    DEADLINE_CHECKS.get()
}
/// Returns metadata, directory-open, and fallback observations in that order.
#[must_use]
pub(crate) fn snapshot() -> (usize, usize, usize) {
    (METADATA.get(), DIRECTORY_OPEN.get(), FALLBACK.get())
}
/// Returns actual Unix metadata and directory-open syscall counts.
#[cfg(unix)]
#[must_use]
pub(crate) fn native_snapshot() -> (usize, usize) {
    (NATIVE_METADATA.get(), NATIVE_DIRECTORY_OPEN.get())
}
