// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Unix descriptor status restoration after nonblocking safety opens.
// qubit-style: allow source-test-pair
// Public APIs keep descriptors live, so native `fcntl` failures cannot be
// induced deterministically by integration fixtures.

use std::io::{
    Error,
    Result,
};
use std::os::fd::RawFd;
use std::thread;
use std::time::Duration;

/// Initial sleep after one scheduler yield for a conflicting file lease.
const INITIAL_OPEN_RETRY_DELAY: Duration = Duration::from_micros(50);
/// Maximum sleep between nonblocking open attempts.
const MAX_OPEN_RETRY_DELAY: Duration = Duration::from_millis(10);

/// Clears the nonblocking status flag from a live descriptor.
///
/// # Parameters
///
/// * `descriptor` - Live descriptor opened with `O_NONBLOCK`.
///
/// # Errors
///
/// Returns the native error from reading or updating descriptor status flags.
pub(crate) fn clear_nonblocking(descriptor: RawFd) -> Result<()> {
    // SAFETY: callers retain ownership of the live descriptor for both
    // non-retaining `fcntl` calls.
    #[cfg(coverage)]
    let flags =
        if super::coverage_fault::is_enabled("unix-clear-nonblocking-get") {
            -1
        } else {
            unsafe { libc::fcntl(descriptor, libc::F_GETFL) }
        };
    #[cfg(not(coverage))]
    let flags = unsafe { libc::fcntl(descriptor, libc::F_GETFL) };
    if flags == -1 {
        return Err(Error::last_os_error());
    }
    if flags & libc::O_NONBLOCK == 0 {
        return Ok(());
    }
    // SAFETY: the descriptor remains live and `F_SETFL` accepts status flags
    // returned by `F_GETFL` with `O_NONBLOCK` cleared.
    #[cfg(coverage)]
    let result =
        if super::coverage_fault::is_enabled("unix-clear-nonblocking-set") {
            -1
        } else {
            unsafe {
                libc::fcntl(
                    descriptor,
                    libc::F_SETFL,
                    flags & !libc::O_NONBLOCK,
                )
            }
        };
    #[cfg(not(coverage))]
    let result = unsafe {
        libc::fcntl(descriptor, libc::F_SETFL, flags & !libc::O_NONBLOCK)
    };
    if result == -1 {
        return Err(Error::last_os_error());
    }
    Ok(())
}

/// Waits before retrying a nonblocking open that conflicts with a file lease.
///
/// The first conflict yields the current time slice. Later conflicts sleep
/// with exponentially increasing delay capped at ten milliseconds. This keeps
/// normal blocking-open semantics without continuously consuming a worker CPU
/// while another process or thread retains the lease.
///
/// # Parameters
///
/// * `delay` - Current retry delay. Callers initialize it to [`Duration::ZERO`]
///   and retain the updated value for later retries.
pub(crate) fn wait_for_nonblocking_open_retry(delay: &mut Duration) {
    if delay.is_zero() {
        thread::yield_now();
        *delay = INITIAL_OPEN_RETRY_DELAY;
        return;
    }
    thread::sleep(*delay);
    *delay = delay.saturating_mul(2).min(MAX_OPEN_RETRY_DELAY);
}
