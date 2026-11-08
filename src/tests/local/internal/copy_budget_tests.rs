// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::io;
use std::io::Cursor;
use std::time::Duration;
use std::time::Instant;

use qubit_io::Output;

use crate::LocalCopyDirOptions;
use crate::local::CopyBudget;

struct InterruptOnceOutput {
    bytes: Vec<u8>,
    interrupted: bool,
}

impl Output for InterruptOnceOutput {
    type Item = u8;

    unsafe fn write_unchecked(&mut self, input: &[u8], index: usize, count: usize) -> io::Result<usize> {
        if !self.interrupted {
            self.interrupted = true;
            return Err(io::ErrorKind::Interrupted.into());
        }
        self.bytes.extend_from_slice(&input[index..index + count]);
        Ok(count)
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

struct ZeroOutput;

impl Output for ZeroOutput {
    type Item = u8;

    unsafe fn write_unchecked(&mut self, _input: &[u8], _index: usize, _count: usize) -> io::Result<usize> {
        Ok(0)
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[test]
fn test_copy_budget_checks_deadline_after_eof() {
    let started = Instant::now();
    let duration = Duration::from_secs(60);
    let mut budget = CopyBudget::new(
        LocalCopyDirOptions::new()
            .with_started_at(started)
            .with_deadline(duration),
    );
    let mut reader = Cursor::new(&b""[..]);
    let mut writer = Vec::<u8>::new();
    let mut calls = 0;
    let failure = budget
        .copy_with_now(&mut reader, &mut writer, || {
            calls += 1;
            if calls == 1 { started } else { started + duration }
        })
        .expect_err("EOF must not bypass the post-read deadline check");
    assert_eq!(io::ErrorKind::TimedOut, failure.kind());
    assert!(writer.is_empty());
    assert!(calls >= 2, "the clock must be read after EOF");
}

#[test]
fn test_copy_budget_retries_interrupted_write_and_rejects_zero_progress() {
    let mut budget = CopyBudget::new(LocalCopyDirOptions::new());
    let mut reader = Cursor::new(b"retry");
    let mut writer = InterruptOnceOutput {
        bytes: Vec::new(),
        interrupted: false,
    };

    assert_eq!(5, budget.copy(&mut reader, &mut writer).unwrap());
    assert_eq!(b"retry", writer.bytes.as_slice());

    let mut budget = CopyBudget::new(LocalCopyDirOptions::new());
    let mut reader = Cursor::new(b"x");
    let error = budget.copy(&mut reader, &mut ZeroOutput).unwrap_err();
    assert_eq!(io::ErrorKind::WriteZero, error.kind());
}

#[test]
fn test_copy_budget_rejects_unrepresentable_absolute_deadline() {
    let budget = CopyBudget::new(
        LocalCopyDirOptions::new()
            .with_started_at(Instant::now())
            .with_deadline(Duration::MAX),
    );

    let error = budget
        .absolute_deadline()
        .expect_err("an unrepresentable deadline must be rejected");

    assert_eq!(io::ErrorKind::InvalidInput, error.kind());
    assert!(error.to_string().contains("monotonic clock range"));
}

#[test]
fn test_copy_budget_depth_limit_accepts_boundary_and_reports_excess() {
    let budget = CopyBudget::new(LocalCopyDirOptions::new().with_max_depth(1));

    budget.check_depth(1).expect("the configured depth is allowed");
    let error = budget.check_depth(2).expect_err("deeper entries must be rejected");

    assert_eq!(io::ErrorKind::QuotaExceeded, error.kind());
    let facts = error
        .get_ref()
        .and_then(|source| source.downcast_ref::<crate::LocalResourceLimitError>())
        .expect("depth failure must retain resource facts");
    assert_eq!(crate::LocalResourceKind::Depth, facts.resource());
    assert_eq!(1, facts.limit());
    assert_eq!(2, facts.requested());
}

#[test]
fn test_copy_budget_directory_permit_exhaustion_recovers_after_drop() {
    let budget = CopyBudget::new(LocalCopyDirOptions::new().with_max_open_directories(1));
    let permit = budget
        .acquire_directory()
        .expect("first directory permit should be acquired")
        .expect("configured pool should return a permit");
    let error = budget
        .acquire_directory()
        .expect_err("the single directory slot is occupied");
    assert_eq!(io::ErrorKind::QuotaExceeded, error.kind());
    drop(permit);
    assert!(
        budget
            .acquire_directory()
            .expect("released slot should recover")
            .is_some()
    );
}

#[test]
fn test_target_deletion_keeps_copy_deadline_without_source_resource_limits() {
    let started = Instant::now();
    let duration = Duration::from_secs(10);
    let budget = CopyBudget::new(
        LocalCopyDirOptions::new()
            .with_started_at(started)
            .with_deadline(duration)
            .with_max_depth(1)
            .with_max_entries(2)
            .with_max_bytes(3)
            .with_max_open_directories(4),
    );

    let (options, deletion_started) = budget.deletion_options();

    assert_eq!(deletion_started, started);
    assert_eq!(options.deadline(), Some(duration));
    assert!(options.recursive());
    assert_eq!(options.max_depth(), None);
    assert_eq!(options.max_entries(), None);
    assert_eq!(options.max_pending_path_bytes(), None);
}

#[test]
fn test_implicit_copy_start_is_preserved_for_target_deletion() {
    let before = Instant::now();
    let duration = Duration::from_secs(10);
    let budget = CopyBudget::new(LocalCopyDirOptions::new().with_deadline(duration));
    let after = Instant::now();

    let (options, started) = budget.deletion_options();

    assert!(started >= before && started <= after);
    assert_eq!(options.deadline(), Some(duration));
    let (_, projected_again) = budget.deletion_options();
    assert_eq!(projected_again, started);
}
