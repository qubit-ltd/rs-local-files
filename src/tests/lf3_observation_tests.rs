// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
use std::io;

use qubit_io::Input;

use crate::error::LocalFileErrorKind;

#[test]
fn test_lf3_observation_collect_limit_and_sentinel() {
    let bytes = [1_u8, 2, 3];
    let mut exact = bytes.as_slice();
    assert_eq!(
        crate::observation::reader::collect_limited(&mut exact, Some(3)).unwrap(),
        bytes,
    );
    let mut short = bytes.as_slice();
    assert_eq!(
        crate::observation::reader::collect_limited(&mut short, Some(2))
            .unwrap_err()
            .kind(),
        LocalFileErrorKind::ResourceLimit,
    );
    let mut empty = [].as_slice();
    assert!(
        crate::observation::reader::collect_limited(&mut empty, Some(0))
            .unwrap()
            .is_empty()
    );
}

#[test]
fn test_lf3_observation_collect_retries_interrupted_and_preserves_read_errors() {
    let mut interrupted = ScriptedInput { phase: 0 };
    assert_eq!(
        crate::observation::reader::collect_limited(&mut interrupted, Some(4)).unwrap(),
        b"data",
    );

    let mut failing = FailingInput { phase: 0 };
    assert_eq!(
        crate::observation::reader::collect_limited(&mut failing, None)
            .unwrap_err()
            .kind(),
        LocalFileErrorKind::Io,
    );
}

#[test]
fn test_lf3_observation_collect_accepts_u64_maximum() {
    let mut input = b"finite".as_slice();
    assert_eq!(
        crate::observation::reader::collect_limited(&mut input, Some(u64::MAX)).unwrap(),
        b"finite",
    );
}

#[test]
fn test_lf3_observation_collect_uses_one_byte_at_the_limit() {
    let mut input = ProbeCountingInput {
        offset: 0,
        requests: Vec::new(),
    };
    assert_eq!(
        crate::observation::reader::collect_limited(&mut input, Some(3)).unwrap(),
        b"abc",
    );
    assert_eq!(input.requests, [3, 1]);
}

#[test]
fn test_lf3_observation_collect_allocation_failure_is_resource_limit() {
    let mut input = b"data".as_slice();
    let error = crate::observation::reader::collect_limited_with_test_reserver(&mut input, None, |_, _| {
        Err(io::Error::other("injected reservation failure"))
    })
    .unwrap_err();
    assert_eq!(error.kind(), LocalFileErrorKind::ResourceLimit);

    let mut independent_input = b"ok".as_slice();
    let bytes = crate::observation::reader::collect_limited_with_test_reserver(
        &mut independent_input,
        None,
        |output, additional| output.try_reserve(additional).map_err(io::Error::other),
    )
    .expect("independent reserver remains unaffected");
    assert_eq!(bytes, b"ok");
}

/// Emits an interruption, a fixed payload, then EOF for collector retry tests.
struct ScriptedInput {
    phase: u8,
}
impl Input for ScriptedInput {
    type Item = u8;
    unsafe fn read_unchecked(&mut self, output: &mut [u8], index: usize, count: usize) -> io::Result<usize> {
        match self.phase {
            0 => {
                self.phase = 1;
                Err(io::ErrorKind::Interrupted.into())
            }
            1 => {
                self.phase = 2;
                let bytes = b"data";
                let amount = count.min(bytes.len());
                output[index..index + amount].copy_from_slice(&bytes[..amount]);
                Ok(amount)
            }
            _ => Ok(0),
        }
    }
}

/// Emits a partial payload followed by a read error.
struct FailingInput {
    phase: u8,
}

/// Records each requested read size while returning a fixed payload.
struct ProbeCountingInput {
    offset: usize,
    requests: Vec<usize>,
}
impl Input for ProbeCountingInput {
    type Item = u8;
    unsafe fn read_unchecked(&mut self, output: &mut [u8], index: usize, count: usize) -> io::Result<usize> {
        self.requests.push(count);
        let bytes = b"abc";
        let amount = count.min(bytes.len().saturating_sub(self.offset));
        output[index..index + amount].copy_from_slice(&bytes[self.offset..self.offset + amount]);
        self.offset += amount;
        Ok(amount)
    }
}
impl Input for FailingInput {
    type Item = u8;
    unsafe fn read_unchecked(&mut self, output: &mut [u8], index: usize, count: usize) -> io::Result<usize> {
        if self.phase == 0 {
            self.phase = 1;
            let amount = count.min(2);
            output[index..index + amount].copy_from_slice(b"ab");
            Ok(amount)
        } else {
            Err(io::Error::other("scripted failure"))
        }
    }
}
