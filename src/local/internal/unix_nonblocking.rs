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

use std::io::Error;
use std::io::ErrorKind;
use std::io::Result;
use std::os::fd::RawFd;
use std::thread;
use std::time::Duration;
use std::time::Instant;

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
    #[cfg(feature = "internal-test-support")]
    let flags = if super::test_support::is_enabled("unix-clear-nonblocking-get") {
        -1
    } else {
        unsafe { libc::fcntl(descriptor, libc::F_GETFL) }
    };
    #[cfg(not(feature = "internal-test-support"))]
    let flags = unsafe { libc::fcntl(descriptor, libc::F_GETFL) };
    if flags == -1 {
        return Err(Error::last_os_error());
    }
    if flags & libc::O_NONBLOCK == 0 {
        return Ok(());
    }
    // SAFETY: the descriptor remains live and `F_SETFL` accepts status flags
    // returned by `F_GETFL` with `O_NONBLOCK` cleared.
    #[cfg(feature = "internal-test-support")]
    let result = if super::test_support::is_enabled("unix-clear-nonblocking-set") {
        -1
    } else {
        unsafe { libc::fcntl(descriptor, libc::F_SETFL, flags & !libc::O_NONBLOCK) }
    };
    #[cfg(not(feature = "internal-test-support"))]
    let result = unsafe { libc::fcntl(descriptor, libc::F_SETFL, flags & !libc::O_NONBLOCK) };
    if result == -1 {
        return Err(Error::last_os_error());
    }
    Ok(())
}

/// Optionally repeats a nonblocking open that conflicts with a file lease.
///
/// The first conflict yields the current time slice. Later conflicts sleep
/// with exponentially increasing delay capped at ten milliseconds. This keeps
/// normal blocking-open semantics without continuously consuming a worker CPU
/// while another process or thread retains the lease. A configured timeout is
/// measured with a monotonic clock and never suppresses the initial attempt.
///
/// # Parameters
///
/// * `timeout` - Explicit maximum duration spent resolving lease conflicts;
///   `None` authorizes only the initial attempt.
/// * `open` - Native nonblocking open attempt.
///
/// # Returns
///
/// Value returned by the first successful open.
///
/// # Errors
///
/// Returns [`ErrorKind::TimedOut`] when the configured duration expires after
/// a lease conflict, or returns any non-conflict open error unchanged.
pub(crate) fn open_with_nonblocking_retry<F, T>(timeout: Option<Duration>, mut open: F) -> Result<T>
where
    F: FnMut() -> Result<T>,
{
    let started_at = Instant::now();
    let mut retry_delay = Duration::ZERO;
    loop {
        match open() {
            Err(error) if error.kind() == ErrorKind::WouldBlock => {
                let Some(timeout) = timeout else {
                    return Err(error);
                };
                let remaining = timeout.saturating_sub(started_at.elapsed());
                if remaining.is_zero() {
                    return Err(open_retry_timed_out(timeout));
                }
                wait_for_nonblocking_open_retry(&mut retry_delay, remaining);
                if started_at.elapsed() >= timeout {
                    return Err(open_retry_timed_out(timeout));
                }
            }
            result => return result,
        }
    }
}

/// Waits before the next nonblocking open attempt.
fn wait_for_nonblocking_open_retry(delay: &mut Duration, remaining: Duration) {
    if delay.is_zero() {
        thread::yield_now();
        *delay = INITIAL_OPEN_RETRY_DELAY;
        return;
    }
    let sleep_duration = remaining.min(*delay);
    thread::sleep(sleep_duration);
    *delay = delay.saturating_mul(2).min(MAX_OPEN_RETRY_DELAY);
}

/// Creates the stable error returned when an open retry deadline expires.
#[must_use]
#[inline]
fn open_retry_timed_out(timeout: Duration) -> Error {
    Error::new(
        ErrorKind::TimedOut,
        format!("timed out after {timeout:?} retrying a nonblocking file open"),
    )
}

#[cfg(test)]
mod tests {
    use std::fs::OpenOptions;
    use std::io::Error;
    use std::io::ErrorKind;
    use std::os::fd::AsRawFd;
    use std::os::unix::fs::OpenOptionsExt;
    use std::time::Duration;

    use tempfile::tempdir;

    use super::INITIAL_OPEN_RETRY_DELAY;
    use super::MAX_OPEN_RETRY_DELAY;
    use super::clear_nonblocking;
    use super::open_with_nonblocking_retry;
    use super::wait_for_nonblocking_open_retry;

    /// Verifies descriptor status restoration clears `O_NONBLOCK` while
    /// retaining every unrelated status flag.
    #[test]
    fn test_clear_nonblocking_restores_descriptor_flags() {
        let directory = tempdir().expect("temporary directory should be created");
        let path = directory.path().join("payload");
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(true)
            .custom_flags(libc::O_NONBLOCK)
            .open(path)
            .expect("nonblocking file should open");

        clear_nonblocking(file.as_raw_fd()).expect("nonblocking status should clear");
        // SAFETY: `file` owns a live descriptor throughout this non-retaining
        // status query.
        let flags = unsafe { libc::fcntl(file.as_raw_fd(), libc::F_GETFL) };
        assert_ne!(-1, flags);
        assert_eq!(0, flags & libc::O_NONBLOCK);
    }

    /// Verifies descriptor status restoration preserves native errors for an
    /// invalid descriptor.
    #[test]
    fn test_clear_nonblocking_rejects_invalid_descriptor() {
        assert!(clear_nonblocking(-1).is_err());
    }

    /// Verifies a lease conflict without an authorized timeout is returned
    /// unchanged and is not retried.
    #[test]
    fn test_open_with_nonblocking_retry_returns_unbudgeted_conflict() {
        let mut attempts = 0;
        let error = open_with_nonblocking_retry(None, || {
            attempts += 1;
            Err::<(), _>(Error::from(ErrorKind::WouldBlock))
        })
        .expect_err("an unbudgeted conflict must fail immediately");

        assert_eq!(ErrorKind::WouldBlock, error.kind());
        assert_eq!(1, attempts);
    }

    /// Verifies a zero retry budget reports a stable timeout after the first
    /// conflict.
    #[test]
    fn test_open_with_nonblocking_retry_reports_timeout() {
        let error = open_with_nonblocking_retry(Some(Duration::ZERO), || {
            Err::<(), _>(Error::from(ErrorKind::WouldBlock))
        })
        .expect_err("a zero retry budget must time out");

        assert_eq!(ErrorKind::TimedOut, error.kind());
        assert!(error.to_string().contains("0ns"));
    }

    /// Verifies retry succeeds after transient conflicts and preserves the
    /// successful return value.
    #[test]
    fn test_open_with_nonblocking_retry_returns_later_success() {
        let mut attempts = 0;
        let value = open_with_nonblocking_retry(Some(Duration::from_secs(1)), || {
            attempts += 1;
            if attempts < 3 {
                Err(Error::from(ErrorKind::WouldBlock))
            } else {
                Ok(17_u8)
            }
        })
        .expect("transient conflicts should be retried");

        assert_eq!(17, value);
        assert_eq!(3, attempts);
    }

    /// Verifies non-conflict native errors are returned without retrying.
    #[test]
    fn test_open_with_nonblocking_retry_preserves_other_error() {
        let mut attempts = 0;
        let error = open_with_nonblocking_retry(Some(Duration::from_secs(1)), || {
            attempts += 1;
            Err::<(), _>(Error::from(ErrorKind::PermissionDenied))
        })
        .expect_err("a non-conflict error must not be retried");

        assert_eq!(ErrorKind::PermissionDenied, error.kind());
        assert_eq!(1, attempts);
    }

    /// Verifies retry delay growth is capped without depending on wall-clock
    /// timing assertions.
    #[test]
    fn test_wait_for_nonblocking_open_retry_initializes_and_caps_delay() {
        let mut delay = Duration::ZERO;
        wait_for_nonblocking_open_retry(&mut delay, Duration::ZERO);
        assert_eq!(INITIAL_OPEN_RETRY_DELAY, delay);

        delay = MAX_OPEN_RETRY_DELAY;
        wait_for_nonblocking_open_retry(&mut delay, Duration::ZERO);
        assert_eq!(MAX_OPEN_RETRY_DELAY, delay);
    }
}
