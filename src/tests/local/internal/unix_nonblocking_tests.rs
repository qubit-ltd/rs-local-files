// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Tests crate-visible Unix nonblocking descriptor and retry contracts.

use std::fs::OpenOptions;
use std::io::Error;
use std::io::ErrorKind;
use std::os::fd::AsRawFd;
use std::os::unix::fs::OpenOptionsExt;
use std::time::Duration;

use tempfile::tempdir;

use super::unix_nonblocking::clear_nonblocking;
use super::unix_nonblocking::open_with_nonblocking_retry;

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
