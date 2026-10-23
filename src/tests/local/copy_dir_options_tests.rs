// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Crate-private contract tests for `LocalCopyDirOptions`.

use std::io::Cursor;
use std::time::Duration;
use std::time::Instant;

use crate::local::CopyBudget;
use crate::local::LocalCopyDirOptions;
use crate::options::LocalCopyConflictPolicy;
use crate::options::LocalCopyTypeConflictPolicy;
use crate::policy::LocalDurabilityRequirement;
use crate::policy::LocalSymlinkPolicy;
use crate::temp::internal::generated_target;

#[test]
fn test_local_copy_dir_options_builders_update_every_policy() {
    let options = LocalCopyDirOptions::new()
        .with_conflict(LocalCopyConflictPolicy::Overwrite)
        .with_type_conflict(LocalCopyTypeConflictPolicy::Replace)
        .with_symlink_policy(LocalSymlinkPolicy::FollowWithinScope)
        .preserve_permissions()
        .with_open_retry_timeout(Duration::from_secs(1))
        .with_durability(LocalDurabilityRequirement::Required);
    assert_eq!(options.conflict_policy(), LocalCopyConflictPolicy::Overwrite);
    assert_eq!(options.type_conflict_policy(), LocalCopyTypeConflictPolicy::Replace);
    assert_eq!(options.symlink_policy(), LocalSymlinkPolicy::FollowWithinScope);
    assert!(options.preserves_permissions());
    assert_eq!(options.open_retry_timeout(), Some(Duration::from_secs(1)));
    assert_eq!(options.durability(), LocalDurabilityRequirement::Required);
    assert_eq!(LocalCopyDirOptions::default(), LocalCopyDirOptions::new());
}

#[test]
fn test_copy_budget_directory_permit_releases_capacity_on_drop() {
    let options = LocalCopyDirOptions::new().with_max_open_directories(1);
    let budget = CopyBudget::new(options);

    let permit = budget
        .acquire_directory()
        .expect("first directory permit should fit")
        .expect("configured limit should return a permit");
    assert!(budget.acquire_directory().is_err());

    drop(permit);
    assert!(
        budget
            .acquire_directory()
            .expect("dropped permit should restore capacity")
            .is_some()
    );
}

#[test]
fn test_copy_budget_stops_at_the_next_chunk_deadline_boundary() {
    let started = Instant::now();
    let options = LocalCopyDirOptions::new()
        .with_deadline(Duration::from_millis(1))
        .with_started_at(started);
    let mut budget = CopyBudget::new(options);
    let source = vec![b'x'; 64 * 1024 + 1];
    let mut reader = Cursor::new(source.clone());
    let mut writer = Vec::new();
    let mut calls = 0_usize;

    let mut now = || {
        calls += 1;
        if calls <= 4 {
            started
        } else {
            started + Duration::from_millis(1)
        }
    };
    let error = budget
        .copy_with_now(&mut reader, &mut writer, &mut now)
        .expect_err("the second chunk must observe the expired deadline");

    assert_eq!(std::io::ErrorKind::TimedOut, error.kind());
    assert!(writer.len() < source.len());
    assert_eq!(64 * 1024, writer.len());
}

#[test]
fn test_copy_budget_accepts_pure_item_wrappers() {
    let mut budget = CopyBudget::new(LocalCopyDirOptions::new());
    let mut input = qubit_io::CountingInput::new(Cursor::new(b"payload"));
    let mut output = qubit_io::CountingOutput::new(Vec::<u8>::new());

    assert_eq!(budget.copy(&mut input, &mut output).unwrap(), 7);
    assert_eq!(input.items_read(), 7);
    assert_eq!(output.items_written(), 7);
    assert_eq!(output.into_inner(), b"payload");
}

#[test]
fn test_copy_budget_accepts_dynamic_protocol_references() {
    let mut budget = CopyBudget::new(LocalCopyDirOptions::new());
    let mut source = Cursor::new(b"dynamic".to_vec());
    let mut destination = Vec::new();
    let reader: &mut dyn qubit_io::ByteInput = &mut source;
    let writer: &mut dyn qubit_io::ByteOutput = &mut destination;

    assert_eq!(budget.copy(reader, writer).unwrap(), 7);
    assert_eq!(destination, b"dynamic");
}

#[test]
fn test_copy_budget_accepts_standard_cursor_and_vec() {
    let mut budget = CopyBudget::new(LocalCopyDirOptions::new());
    let mut reader = Cursor::new(b"standard".to_vec());
    let mut writer = Vec::new();

    assert_eq!(budget.copy(&mut reader, &mut writer).unwrap(), 8);
    assert_eq!(writer, b"standard");
}

#[test]
fn test_generated_keep_target_promotes_only_well_formed_sandbox_paths() {
    assert_eq!(
        std::path::Path::new("/parent/resource"),
        generated_target(std::path::Path::new("/parent/sandbox/resource"))
            .expect("sandboxed path should derive a sibling target")
    );
    assert!(generated_target(std::path::Path::new("resource")).is_err());
    assert!(generated_target(std::path::Path::new("/resource")).is_err());
    assert!(generated_target(std::path::Path::new("/parent/sandbox/..")).is_err());
    assert_eq!(
        std::path::Path::new("/resource"),
        generated_target(std::path::Path::new("/sandbox/resource"))
            .expect("root sandbox should derive a root sibling target")
    );
}
