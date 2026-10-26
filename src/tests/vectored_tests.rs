// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Crate-local contract tests for vectored buffer conversion.

use std::io::IoSlice;
use std::io::IoSliceMut;

use crate::vectored::byte_slices;
use crate::vectored::byte_slices_mut;

#[test]
fn test_byte_slices_preserve_buffer_order_and_contents() {
    let first = b"first";
    let second = b"second";
    let buffers = [IoSlice::new(first), IoSlice::new(second)];

    let slices = byte_slices(&buffers).expect("immutable buffers should convert");

    assert_eq!(b"first", slices[0]);
    assert_eq!(b"second", slices[1]);
}

#[test]
fn test_byte_slices_mut_preserve_order_and_mutability() {
    let mut first = [0; 3];
    let mut second = [0; 2];
    let mut buffers = [IoSliceMut::new(&mut first), IoSliceMut::new(&mut second)];

    let mut slices = byte_slices_mut(&mut buffers).expect("mutable buffers should convert");
    slices[0].copy_from_slice(b"one");
    slices[1].copy_from_slice(b"hi");

    assert_eq!(*b"one", first);
    assert_eq!(*b"hi", second);
}
