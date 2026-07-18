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
    let flags = unsafe { libc::fcntl(descriptor, libc::F_GETFL) };
    if flags == -1 {
        return Err(Error::last_os_error());
    }
    if flags & libc::O_NONBLOCK == 0 {
        return Ok(());
    }
    // SAFETY: the descriptor remains live and `F_SETFL` accepts status flags
    // returned by `F_GETFL` with `O_NONBLOCK` cleared.
    let result = unsafe {
        libc::fcntl(descriptor, libc::F_SETFL, flags & !libc::O_NONBLOCK)
    };
    if result == -1 {
        return Err(Error::last_os_error());
    }
    Ok(())
}
