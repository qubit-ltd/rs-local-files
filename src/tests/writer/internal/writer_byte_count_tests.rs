// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::cell::Cell;
use std::io;

use crate::writer::internal::WriterByteCount;

#[test]
fn test_writer_byte_count_crosses_u32_and_records_actual_short_write() {
    let mut count = WriterByteCount::with_initial_value(u64::from(u32::MAX) - 1);
    count.check_request(3).expect("request fits u64");
    count.record_result(Ok(2)).expect("short write succeeds");
    assert_eq!(u64::from(u32::MAX) + 1, count.get());
}

#[test]
fn test_writer_byte_count_zero_and_error_do_not_increase() {
    let mut count = WriterByteCount::with_initial_value(9);
    count.check_request(0).expect("zero request fits");
    count.record_result(Ok(0)).expect("zero write succeeds");
    let result: io::Result<usize> = Err(io::ErrorKind::Other.into());
    assert_eq!(io::ErrorKind::Other, count.record_result(result).unwrap_err().kind());
    assert_eq!(9, count.get());
}

#[test]
fn test_writer_byte_count_rejects_u64_overflow_before_backend() {
    let count = WriterByteCount::with_initial_value(u64::MAX - 1);
    let called = Cell::new(false);
    let result = count.check_request(2);
    if result.is_ok() {
        called.set(true);
    }
    assert_eq!(io::ErrorKind::FileTooLarge, result.unwrap_err().kind());
    assert!(!called.get());
}

#[test]
fn test_writer_byte_count_rejects_vectored_length_overflow_before_backend() {
    let count = WriterByteCount::with_initial_value(u64::MAX);
    let called = Cell::new(false);
    let result = count.check_vectored_request([b"x".as_slice()]);
    if result.is_ok() {
        called.set(true);
    }
    assert_eq!(io::ErrorKind::FileTooLarge, result.unwrap_err().kind());
    assert!(!called.get());
}

#[test]
fn test_writer_byte_count_debug_and_file_too_large_kind() {
    let count = WriterByteCount::with_initial_value(7);
    assert_eq!("7", format!("{count:?}"));
}
