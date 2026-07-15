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
    assert_eq!(FileBuffering::Unbuffered, FileBuffering::default());
    assert_eq!(
        FileBuffering::Buffered { capacity: None },
        FileBuffering::buffered()
    );
    assert_eq!(
        FileBuffering::Buffered {
            capacity: NonZeroUsize::new(32),
        },
        FileBuffering::buffered_with_capacity(32)
            .expect("positive buffer capacity should be accepted")
    );
}

#[test]
fn test_file_buffering_rejects_zero_capacity() {
    let error = FileBuffering::buffered_with_capacity(0)
        .expect_err("zero capacity should be rejected");

    assert_eq!(ErrorKind::InvalidInput, error.kind());
}
