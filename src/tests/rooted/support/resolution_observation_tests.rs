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
}

pub(crate) fn resolution_now(deadline: Option<Instant>) -> Instant {
    DEADLINE_CHECKS.set(DEADLINE_CHECKS.get() + 1);
    if EXPIRE_AFTER.get().is_some_and(|limit| DEADLINE_CHECKS.get() > limit) {
        deadline.unwrap_or_else(Instant::now)
    } else {
        Instant::now()
    }
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

pub(crate) struct DeadlineClockGuard(Option<usize>);

impl DeadlineClockGuard {
    pub(crate) fn expire_after(checks: usize) -> Self {
        let previous = EXPIRE_AFTER.replace(Some(checks));
        DEADLINE_CHECKS.set(0);
        Self(previous)
    }
}

impl Drop for DeadlineClockGuard {
    fn drop(&mut self) {
        EXPIRE_AFTER.set(self.0);
        DEADLINE_CHECKS.set(0);
    }
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
