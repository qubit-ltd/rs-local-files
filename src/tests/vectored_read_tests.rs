// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::collections::VecDeque;
use std::io;
use std::io::ErrorKind;
use std::io::IoSliceMut;
use std::io::Read;

use crate::read::read_vectored_fallback;

enum ScriptedAction {
    Bytes(Vec<u8>),
    Error(ErrorKind),
}

struct ScriptedReader {
    actions: VecDeque<ScriptedAction>,
}

impl ScriptedReader {
    fn new(actions: impl IntoIterator<Item = ScriptedAction>) -> Self {
        Self {
            actions: actions.into_iter().collect(),
        }
    }
}

impl Read for ScriptedReader {
    fn read(&mut self, buffer: &mut [u8]) -> io::Result<usize> {
        let action = self.actions.pop_front().unwrap_or(ScriptedAction::Bytes(Vec::new()));
        match action {
            ScriptedAction::Bytes(bytes) => {
                let count = bytes.len().min(buffer.len());
                buffer[..count].copy_from_slice(&bytes[..count]);
                if count < bytes.len() {
                    self.actions.push_front(ScriptedAction::Bytes(bytes[count..].to_vec()));
                }
                Ok(count)
            }
            ScriptedAction::Error(kind) => Err(io::Error::from(kind)),
        }
    }
}

/// Verifies the fallback returns bytes read before a later error.
#[test]
fn test_vectored_fallback_returns_progress_before_later_error() {
    let mut reader = ScriptedReader::new([
        ScriptedAction::Bytes(b"ab".to_vec()),
        ScriptedAction::Error(ErrorKind::PermissionDenied),
    ]);
    let mut first = [0_u8; 2];
    let mut second = [0_u8; 2];
    let mut buffers = [IoSliceMut::new(&mut first), IoSliceMut::new(&mut second)];

    let count =
        read_vectored_fallback(&mut reader, &mut buffers).expect("progress before a later error should be returned");

    assert_eq!(count, 2);
    assert_eq!(&first, b"ab");
    assert_eq!(&second, &[0_u8; 2]);
}

/// Verifies the fallback returns the first error when no buffer was read.
#[test]
fn test_vectored_fallback_returns_first_error_without_progress() {
    let mut reader = ScriptedReader::new([ScriptedAction::Error(ErrorKind::PermissionDenied)]);
    let mut buffer = [0_u8; 2];
    let mut buffers = [IoSliceMut::new(&mut buffer)];

    let error = read_vectored_fallback(&mut reader, &mut buffers).expect_err("the first error should be preserved");

    assert_eq!(error.kind(), ErrorKind::PermissionDenied);
    assert_eq!(&buffer, &[0_u8; 2]);
}

/// Verifies empty buffers are skipped before reading the next buffer.
#[test]
fn test_vectored_fallback_skips_empty_buffers() {
    let mut reader = ScriptedReader::new([ScriptedAction::Bytes(b"xy".to_vec())]);
    let mut empty = [];
    let mut buffer = [0_u8; 2];
    let mut buffers = [IoSliceMut::new(&mut empty), IoSliceMut::new(&mut buffer)];

    let count = read_vectored_fallback(&mut reader, &mut buffers)
        .expect("a non-empty buffer after an empty buffer should be read");

    assert_eq!(count, 2);
    assert_eq!(&buffer, b"xy");
}

/// Verifies a short read stops before later buffers are consumed.
#[test]
fn test_vectored_fallback_stops_after_short_read() {
    let mut reader = ScriptedReader::new([
        ScriptedAction::Bytes(b"a".to_vec()),
        ScriptedAction::Bytes(b"bc".to_vec()),
    ]);
    let mut first = [0_u8; 2];
    let mut second = [0_u8; 2];
    let mut buffers = [IoSliceMut::new(&mut first), IoSliceMut::new(&mut second)];

    let count =
        read_vectored_fallback(&mut reader, &mut buffers).expect("a short read should complete the vectored operation");

    assert_eq!(count, 1);
    assert_eq!(&first, &[b'a', 0]);
    assert_eq!(&second, &[0_u8; 2]);
}

/// Verifies EOF stops before later buffers are consumed.
#[test]
fn test_vectored_fallback_stops_at_eof() {
    let mut reader = ScriptedReader::new([
        ScriptedAction::Bytes(Vec::new()),
        ScriptedAction::Bytes(b"unused".to_vec()),
    ]);
    let mut first = [0_u8; 2];
    let mut second = [0_u8; 2];
    let mut buffers = [IoSliceMut::new(&mut first), IoSliceMut::new(&mut second)];

    let count = read_vectored_fallback(&mut reader, &mut buffers).expect("EOF should complete the vectored operation");

    assert_eq!(count, 0);
    assert_eq!(&first, &[0_u8; 2]);
    assert_eq!(&second, &[0_u8; 2]);
}

/// Verifies an initial and a later interrupted read follow the fallback error
/// rule.
#[test]
fn test_vectored_fallback_preserves_initial_interrupt_and_later_progress() {
    let mut initial_reader = ScriptedReader::new([ScriptedAction::Error(ErrorKind::Interrupted)]);
    let mut initial_buffer = [0_u8; 2];
    let mut initial_buffers = [IoSliceMut::new(&mut initial_buffer)];
    let initial_error = read_vectored_fallback(&mut initial_reader, &mut initial_buffers)
        .expect_err("an initial interrupt should be returned");
    assert_eq!(initial_error.kind(), ErrorKind::Interrupted);

    let mut later_reader = ScriptedReader::new([
        ScriptedAction::Bytes(b"ab".to_vec()),
        ScriptedAction::Error(ErrorKind::Interrupted),
    ]);
    let mut first = [0_u8; 2];
    let mut second = [0_u8; 2];
    let mut later_buffers = [IoSliceMut::new(&mut first), IoSliceMut::new(&mut second)];
    let count = read_vectored_fallback(&mut later_reader, &mut later_buffers)
        .expect("progress before an interrupt should be returned");
    assert_eq!(count, 2);
    assert_eq!(&first, b"ab");
    assert_eq!(&second, &[0_u8; 2]);
}
