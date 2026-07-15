// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::io::ErrorKind;
use std::num::NonZeroUsize;

use qubit_local_files::FileBuffering;

#[test]
fn test_file_buffering_constructors_are_explicit() {
    let unbuffered = FileBuffering::default();
    let buffered = FileBuffering::buffered();
    let custom = FileBuffering::buffered_with_capacity(32)
        .expect("positive buffer capacity should be accepted");

    assert_eq!(FileBuffering::Unbuffered, unbuffered);
    assert_eq!(FileBuffering::Buffered { capacity: None }, buffered);
    assert_eq!(
        FileBuffering::Buffered {
            capacity: NonZeroUsize::new(32),
        },
        custom
    );
    assert!(!unbuffered.is_buffered());
    assert!(buffered.is_buffered());
    assert!(custom.is_buffered());
    assert_eq!(None, unbuffered.capacity());
    assert_eq!(None, buffered.capacity());
    assert_eq!(NonZeroUsize::new(32), custom.capacity());
}

#[test]
fn test_file_buffering_rejects_zero_capacity() {
    let error = FileBuffering::buffered_with_capacity(0)
        .expect_err("zero capacity should be rejected");

    assert_eq!(ErrorKind::InvalidInput, error.kind());
}
