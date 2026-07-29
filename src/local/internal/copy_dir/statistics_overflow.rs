// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Overflow errors for recursive-copy statistics beyond finite fixtures.
// qubit-style: allow source-test-pair
// Portable integration fixtures cannot force counters beyond `u64::MAX`.

use std::io::{Error, ErrorKind};

/// Creates the overflow error for the directory counter.
///
/// # Returns
///
/// An `InvalidData` error identifying the directory counter.
#[inline(always)]
pub(super) fn directory_statistics_overflow_error() -> Error {
    statistics_overflow_error("directories")
}

/// Creates the overflow error for the skipped-file counter.
///
/// # Returns
///
/// An `InvalidData` error identifying the skipped-file counter.
#[inline(always)]
pub(super) fn skipped_statistics_overflow_error() -> Error {
    statistics_overflow_error("skipped")
}

/// Creates the overflow error for the copied-file counter.
///
/// # Returns
///
/// An `InvalidData` error identifying the copied-file counter.
#[inline(always)]
pub(super) fn file_statistics_overflow_error() -> Error {
    statistics_overflow_error("files")
}

/// Creates the overflow error for the copied-byte counter.
///
/// # Returns
///
/// An `InvalidData` error identifying the copied-byte counter.
#[inline(always)]
pub(super) fn byte_statistics_overflow_error() -> Error {
    statistics_overflow_error("bytes")
}

/// Creates an exact recursive-copy statistics overflow error.
///
/// # Parameters
///
/// * `field` - The statistics field whose checked update overflowed.
///
/// # Returns
///
/// An `InvalidData` error naming the overflowing field.
#[inline]
fn statistics_overflow_error(field: &str) -> Error {
    Error::new(
        ErrorKind::InvalidData,
        format!("recursive-copy {field} statistics overflow"),
    )
}
