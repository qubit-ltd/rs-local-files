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

#[test]
fn test_host_atomic_writer_vectored_adapter_publishes_the_ordered_stream() {
    let fixture = tempfile::tempdir().expect("fixture directory should exist");
    let target = fixture.path().join("target");
    let options = crate::local::LocalAtomicWriteOptions::new()
        .with_durability(crate::policy::LocalDurabilityRequirement::NotRequired);
    let mut writer = crate::local::LocalAtomicWriter::new(&target, options).expect("atomic writer should open");
    let written = writer
        .write_vectored(&[IoSlice::new(b""), IoSlice::new(b"ab"), IoSlice::new(b"cdef")])
        .expect("vectored adapter should accept an ordered prefix");
    assert!((1..=6).contains(&written));
    qubit_io::Output::write_fully(&mut writer, &b"abcdef"[written..]).expect("remaining bytes should write");
    assert!(
        !target.exists(),
        "private staging must remain unpublished before commit"
    );
    writer
        .commit_recoverable_with_durability()
        .expect("atomic stream should commit");
    assert_eq!(std::fs::read(target).expect("published stream should exist"), b"abcdef");
}

#[cfg(unix)]
#[test]
fn test_rooted_atomic_writer_vectored_adapter_publishes_the_ordered_stream() {
    let fixture = tempfile::tempdir().expect("fixture directory should exist");
    let authority = std::fs::File::open(fixture.path()).expect("directory authority should open");
    let target = crate::local::LocalRelativePath::new(std::path::Path::new("target"))
        .expect("target should be a relative descendant");
    let options = crate::local::LocalAtomicWriteOptions::new()
        .with_durability(crate::policy::LocalDurabilityRequirement::NotRequired);
    let mut writer = crate::local::LocalRootAtomicWriter::new(&authority, fixture.path(), &target, options)
        .expect("rooted atomic writer should open");
    let written = writer
        .write_vectored(&[IoSlice::new(b""), IoSlice::new(b"ab"), IoSlice::new(b"cdef")])
        .expect("vectored adapter should accept an ordered prefix");
    assert!((1..=6).contains(&written));
    qubit_io::Output::write_fully(&mut writer, &b"abcdef"[written..]).expect("remaining bytes should write");
    assert!(!fixture.path().join("target").exists());
    writer
        .commit_with_durability()
        .expect("rooted atomic stream should commit");
    assert_eq!(
        std::fs::read(fixture.path().join("target")).expect("published stream should exist"),
        b"abcdef"
    );
}

#[test]
fn test_observed_reader_vectored_reads_preserve_stream_position() {
    let fixture = tempfile::tempdir().expect("fixture directory should exist");
    let target = fixture.path().join("payload");
    std::fs::write(&target, b"abcdef").expect("payload should be written");
    let filesystem = crate::LocalFileSystem::host().expect("host filesystem should open");
    let mut reader = filesystem
        .open_observed_reader(&target, &crate::options::LocalObservedReadOptions::new())
        .expect("observed reader should open");
    let before = reader.observation().clone();
    let mut first = [0; 2];
    let mut second = [0; 4];
    let count = qubit_io::Input::read_vectored(&mut reader, &mut [&mut first[..], &mut second[..]])
        .expect("observed reader should fill an ordered prefix");
    assert!((1..=6).contains(&count));
    let observed = first.into_iter().chain(second).take(count).collect::<Vec<_>>();
    assert_eq!(observed, b"abcdef"[..count]);
    assert_eq!(
        reader.read_to_end_limited(6).expect("remaining stream should read"),
        b"abcdef"[count..]
    );
    assert_eq!(
        reader
            .observe_now()
            .expect("handle should remain observable")
            .identity(),
        before.identity()
    );
}
