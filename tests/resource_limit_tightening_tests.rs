// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Resource ceilings compose without replacing operation behavior.

use std::time::Duration;

use proptest::option::of;
use proptest::prelude::any;
use proptest::prelude::prop_assert;
use proptest::prelude::prop_assert_eq;
use proptest::prelude::proptest;
use qubit_local_files::options::LocalCopyOptions;
use qubit_local_files::options::LocalCopySourceMode;
use qubit_local_files::options::LocalDeleteOptions;
use qubit_local_files::options::LocalDirectoryReopenPolicy;
use qubit_local_files::options::LocalListOptions;
use qubit_local_files::options::LocalWalkErrorPolicy;
use qubit_local_files::policy::LocalSymlinkPolicy;

/// Copy limits must not inherit source mode or parent-creation behavior.
#[test]
fn test_copy_tightening_preserves_request_behavior() {
    let request = LocalCopyOptions::new().with_entry_source().with_max_entries(20);
    let ceiling = LocalCopyOptions::new()
        .with_tree_source()
        .with_create_parent()
        .with_max_entries(5)
        .with_max_bytes(0)
        .with_max_depth(2)
        .with_max_open_directories(1)
        .with_deadline(Duration::ZERO);
    let result = request.tighten_resource_limits(&ceiling);
    assert_eq!(result.source_mode(), LocalCopySourceMode::Entry);
    assert!(!result.creates_parent());
    assert_eq!(result.max_entries(), Some(5));
    assert_eq!(result.max_bytes(), Some(0));
    assert_eq!(result.max_depth(), Some(2));
    assert_eq!(result.max_open_directories(), Some(1));
    assert_eq!(result.deadline(), Some(Duration::ZERO));
    assert_eq!(result, result.tighten_resource_limits(&ceiling));
}

/// A listing ceiling must not enable recursive traversal.
#[test]
fn test_list_tightening_preserves_request_behavior() {
    let request = LocalListOptions::new().with_max_entries(3);
    let ceiling = LocalListOptions::new()
        .with_recursive()
        .with_reopen_policy(LocalDirectoryReopenPolicy::Fail)
        .with_error_policy(LocalWalkErrorPolicy::Continue)
        .with_symlink_policy(LocalSymlinkPolicy::Reject)
        .with_max_entries(10)
        .with_max_depth(2)
        .with_max_seen_name_bytes(20)
        .with_max_open_directories(1)
        .with_deadline(Duration::ZERO);
    let result = request.tighten_resource_limits(&ceiling);
    assert!(!result.recursive());
    assert_eq!(result.reopen_policy(), request.reopen_policy());
    assert_eq!(result.error_policy(), request.error_policy());
    assert_eq!(result.symlink_policy(), request.symlink_policy());
    assert_eq!(result.max_entries(), Some(3));
    assert_eq!(result.max_depth(), Some(2));
    assert_eq!(result.max_seen_name_bytes(), Some(20));
    assert_eq!(result.max_open_directories(), Some(1));
    assert_eq!(result.deadline(), Some(Duration::ZERO));
    assert_eq!(result, result.tighten_resource_limits(&ceiling));
}

/// Deletion ceilings cannot enable recursion or tolerate missing entries.
#[test]
fn test_delete_tightening_preserves_request_behavior() {
    let request = LocalDeleteOptions::new().with_max_entries(3);
    let ceiling = LocalDeleteOptions::new()
        .with_recursive()
        .with_missing_ok()
        .with_max_entries(10)
        .with_max_depth(2)
        .with_max_pending_path_bytes(20)
        .with_deadline(Duration::ZERO);
    let result = request.tighten_resource_limits(&ceiling);
    assert!(!result.recursive());
    assert!(!result.missing_ok());
    assert_eq!(result.max_entries(), Some(3));
    assert_eq!(result.max_depth(), Some(2));
    assert_eq!(result.max_pending_path_bytes(), Some(20));
    assert_eq!(result.deadline(), Some(Duration::ZERO));
    assert_eq!(result, result.tighten_resource_limits(&ceiling));
}

proptest! {
    /// Entry ceilings have the algebra of intersection, with None as infinity.
    #[test]
    fn test_copy_entry_limit_intersection(
        a in of(any::<usize>()),
        b in of(any::<usize>()),
        c in of(any::<usize>()),
    ) {
        let build = |limit: Option<usize>| limit.map_or_else(LocalCopyOptions::new,
            |limit| LocalCopyOptions::new().with_max_entries(limit));
        let a = build(a);
        let b = build(b);
        let c = build(c);
        let ab = a.tighten_resource_limits(&b);
        prop_assert_eq!(ab, b.tighten_resource_limits(&a));
        prop_assert_eq!(ab, ab.tighten_resource_limits(&b));
        prop_assert_eq!(ab.tighten_resource_limits(&c), a.tighten_resource_limits(&b.tighten_resource_limits(&c)));
        prop_assert_eq!(a.tighten_resource_limits(&LocalCopyOptions::new()), a);
        if let Some(limit) = a.max_entries() { prop_assert!(ab.max_entries().is_some_and(|value| value <= limit)); }
        if let Some(limit) = b.max_entries() { prop_assert!(ab.max_entries().is_some_and(|value| value <= limit)); }
    }
}

proptest! {
    /// The max_depth ceiling composes monotonically and without hidden defaults.
    #[test]
    fn test_localcopyoptions_max_depth_intersection(
        a in of(any::<usize>()),
        b in of(any::<usize>()),
        c in of(any::<usize>()),
    ) {
        let build = |value: Option<usize>| value.map_or_else(LocalCopyOptions::new,
            |value| LocalCopyOptions::new().with_max_depth(value));
        let a = build(a);
        let b = build(b);
        let c = build(c);
        let ab = a.tighten_resource_limits(&b);
        prop_assert_eq!(ab, b.tighten_resource_limits(&a));
        prop_assert_eq!(ab, ab.tighten_resource_limits(&b));
        prop_assert_eq!(ab.tighten_resource_limits(&c), a.tighten_resource_limits(&b.tighten_resource_limits(&c)));
        prop_assert_eq!(a.tighten_resource_limits(&LocalCopyOptions::new()), a);
        if let Some(limit) = a.max_depth() { prop_assert!(ab.max_depth().is_some_and(|value| value <= limit)); }
        if let Some(limit) = b.max_depth() { prop_assert!(ab.max_depth().is_some_and(|value| value <= limit)); }
    }
}

proptest! {
    /// The max_bytes ceiling composes monotonically and without hidden defaults.
    #[test]
    fn test_localcopyoptions_max_bytes_intersection(
        a in of(any::<u64>()),
        b in of(any::<u64>()),
        c in of(any::<u64>()),
    ) {
        let build = |value: Option<u64>| value.map_or_else(LocalCopyOptions::new,
            |value| LocalCopyOptions::new().with_max_bytes(value));
        let a = build(a);
        let b = build(b);
        let c = build(c);
        let ab = a.tighten_resource_limits(&b);
        prop_assert_eq!(ab, b.tighten_resource_limits(&a));
        prop_assert_eq!(ab, ab.tighten_resource_limits(&b));
        prop_assert_eq!(ab.tighten_resource_limits(&c), a.tighten_resource_limits(&b.tighten_resource_limits(&c)));
        prop_assert_eq!(a.tighten_resource_limits(&LocalCopyOptions::new()), a);
        if let Some(limit) = a.max_bytes() { prop_assert!(ab.max_bytes().is_some_and(|value| value <= limit)); }
        if let Some(limit) = b.max_bytes() { prop_assert!(ab.max_bytes().is_some_and(|value| value <= limit)); }
    }
}

proptest! {
    /// The max_open_directories ceiling composes monotonically and without hidden defaults.
    #[test]
    fn test_localcopyoptions_max_open_directories_intersection(
        a in of(any::<usize>()),
        b in of(any::<usize>()),
        c in of(any::<usize>()),
    ) {
        let build = |value: Option<usize>| value.map_or_else(LocalCopyOptions::new,
            |value| LocalCopyOptions::new().with_max_open_directories(value));
        let a = build(a);
        let b = build(b);
        let c = build(c);
        let ab = a.tighten_resource_limits(&b);
        prop_assert_eq!(ab, b.tighten_resource_limits(&a));
        prop_assert_eq!(ab, ab.tighten_resource_limits(&b));
        prop_assert_eq!(ab.tighten_resource_limits(&c), a.tighten_resource_limits(&b.tighten_resource_limits(&c)));
        prop_assert_eq!(a.tighten_resource_limits(&LocalCopyOptions::new()), a);
        if let Some(limit) = a.max_open_directories() { prop_assert!(ab.max_open_directories().is_some_and(|value| value <= limit)); }
        if let Some(limit) = b.max_open_directories() { prop_assert!(ab.max_open_directories().is_some_and(|value| value <= limit)); }
    }
}

proptest! {
    /// The deadline ceiling composes monotonically and without hidden defaults.
    #[test]
    fn test_localcopyoptions_deadline_intersection(
        a in of(any::<u64>()),
        b in of(any::<u64>()),
        c in of(any::<u64>()),
    ) {
        let build = |value: Option<u64>| value.map_or_else(LocalCopyOptions::new,
            |value| LocalCopyOptions::new().with_deadline(Duration::from_nanos(value)));
        let a = build(a);
        let b = build(b);
        let c = build(c);
        let ab = a.tighten_resource_limits(&b);
        prop_assert_eq!(ab, b.tighten_resource_limits(&a));
        prop_assert_eq!(ab, ab.tighten_resource_limits(&b));
        prop_assert_eq!(ab.tighten_resource_limits(&c), a.tighten_resource_limits(&b.tighten_resource_limits(&c)));
        prop_assert_eq!(a.tighten_resource_limits(&LocalCopyOptions::new()), a);
        if let Some(limit) = a.deadline() { prop_assert!(ab.deadline().is_some_and(|value| value <= limit)); }
        if let Some(limit) = b.deadline() { prop_assert!(ab.deadline().is_some_and(|value| value <= limit)); }
    }
}

proptest! {
    /// The max_depth ceiling composes monotonically and without hidden defaults.
    #[test]
    fn test_locallistoptions_max_depth_intersection(
        a in of(any::<usize>()),
        b in of(any::<usize>()),
        c in of(any::<usize>()),
    ) {
        let build = |value: Option<usize>| value.map_or_else(LocalListOptions::new,
            |value| LocalListOptions::new().with_max_depth(value));
        let a = build(a);
        let b = build(b);
        let c = build(c);
        let ab = a.tighten_resource_limits(&b);
        prop_assert_eq!(ab, b.tighten_resource_limits(&a));
        prop_assert_eq!(ab, ab.tighten_resource_limits(&b));
        prop_assert_eq!(ab.tighten_resource_limits(&c), a.tighten_resource_limits(&b.tighten_resource_limits(&c)));
        prop_assert_eq!(a.tighten_resource_limits(&LocalListOptions::new()), a);
        if let Some(limit) = a.max_depth() { prop_assert!(ab.max_depth().is_some_and(|value| value <= limit)); }
        if let Some(limit) = b.max_depth() { prop_assert!(ab.max_depth().is_some_and(|value| value <= limit)); }
    }
}

proptest! {
    /// The max_entries ceiling composes monotonically and without hidden defaults.
    #[test]
    fn test_locallistoptions_max_entries_intersection(
        a in of(any::<usize>()),
        b in of(any::<usize>()),
        c in of(any::<usize>()),
    ) {
        let build = |value: Option<usize>| value.map_or_else(LocalListOptions::new,
            |value| LocalListOptions::new().with_max_entries(value));
        let a = build(a);
        let b = build(b);
        let c = build(c);
        let ab = a.tighten_resource_limits(&b);
        prop_assert_eq!(ab, b.tighten_resource_limits(&a));
        prop_assert_eq!(ab, ab.tighten_resource_limits(&b));
        prop_assert_eq!(ab.tighten_resource_limits(&c), a.tighten_resource_limits(&b.tighten_resource_limits(&c)));
        prop_assert_eq!(a.tighten_resource_limits(&LocalListOptions::new()), a);
        if let Some(limit) = a.max_entries() { prop_assert!(ab.max_entries().is_some_and(|value| value <= limit)); }
        if let Some(limit) = b.max_entries() { prop_assert!(ab.max_entries().is_some_and(|value| value <= limit)); }
    }
}

proptest! {
    /// The max_open_directories ceiling composes monotonically and without hidden defaults.
    #[test]
    fn test_locallistoptions_max_open_directories_intersection(
        a in of(any::<usize>()),
        b in of(any::<usize>()),
        c in of(any::<usize>()),
    ) {
        let build = |value: Option<usize>| value.map_or_else(LocalListOptions::new,
            |value| LocalListOptions::new().with_max_open_directories(value));
        let a = build(a);
        let b = build(b);
        let c = build(c);
        let ab = a.tighten_resource_limits(&b);
        prop_assert_eq!(ab, b.tighten_resource_limits(&a));
        prop_assert_eq!(ab, ab.tighten_resource_limits(&b));
        prop_assert_eq!(ab.tighten_resource_limits(&c), a.tighten_resource_limits(&b.tighten_resource_limits(&c)));
        prop_assert_eq!(a.tighten_resource_limits(&LocalListOptions::new()), a);
        if let Some(limit) = a.max_open_directories() { prop_assert!(ab.max_open_directories().is_some_and(|value| value <= limit)); }
        if let Some(limit) = b.max_open_directories() { prop_assert!(ab.max_open_directories().is_some_and(|value| value <= limit)); }
    }
}

proptest! {
    /// The max_seen_name_bytes ceiling composes monotonically and without hidden defaults.
    #[test]
    fn test_locallistoptions_max_seen_name_bytes_intersection(
        a in of(any::<usize>()),
        b in of(any::<usize>()),
        c in of(any::<usize>()),
    ) {
        let build = |value: Option<usize>| value.map_or_else(LocalListOptions::new,
            |value| LocalListOptions::new().with_max_seen_name_bytes(value));
        let a = build(a);
        let b = build(b);
        let c = build(c);
        let ab = a.tighten_resource_limits(&b);
        prop_assert_eq!(ab, b.tighten_resource_limits(&a));
        prop_assert_eq!(ab, ab.tighten_resource_limits(&b));
        prop_assert_eq!(ab.tighten_resource_limits(&c), a.tighten_resource_limits(&b.tighten_resource_limits(&c)));
        prop_assert_eq!(a.tighten_resource_limits(&LocalListOptions::new()), a);
        if let Some(limit) = a.max_seen_name_bytes() { prop_assert!(ab.max_seen_name_bytes().is_some_and(|value| value <= limit)); }
        if let Some(limit) = b.max_seen_name_bytes() { prop_assert!(ab.max_seen_name_bytes().is_some_and(|value| value <= limit)); }
    }
}

proptest! {
    /// The deadline ceiling composes monotonically and without hidden defaults.
    #[test]
    fn test_locallistoptions_deadline_intersection(
        a in of(any::<u64>()),
        b in of(any::<u64>()),
        c in of(any::<u64>()),
    ) {
        let build = |value: Option<u64>| value.map_or_else(LocalListOptions::new,
            |value| LocalListOptions::new().with_deadline(Duration::from_nanos(value)));
        let a = build(a);
        let b = build(b);
        let c = build(c);
        let ab = a.tighten_resource_limits(&b);
        prop_assert_eq!(ab, b.tighten_resource_limits(&a));
        prop_assert_eq!(ab, ab.tighten_resource_limits(&b));
        prop_assert_eq!(ab.tighten_resource_limits(&c), a.tighten_resource_limits(&b.tighten_resource_limits(&c)));
        prop_assert_eq!(a.tighten_resource_limits(&LocalListOptions::new()), a);
        if let Some(limit) = a.deadline() { prop_assert!(ab.deadline().is_some_and(|value| value <= limit)); }
        if let Some(limit) = b.deadline() { prop_assert!(ab.deadline().is_some_and(|value| value <= limit)); }
    }
}

proptest! {
    /// The max_depth ceiling composes monotonically and without hidden defaults.
    #[test]
    fn test_localdeleteoptions_max_depth_intersection(
        a in of(any::<usize>()),
        b in of(any::<usize>()),
        c in of(any::<usize>()),
    ) {
        let build = |value: Option<usize>| value.map_or_else(LocalDeleteOptions::new,
            |value| LocalDeleteOptions::new().with_max_depth(value));
        let a = build(a);
        let b = build(b);
        let c = build(c);
        let ab = a.tighten_resource_limits(&b);
        prop_assert_eq!(ab, b.tighten_resource_limits(&a));
        prop_assert_eq!(ab, ab.tighten_resource_limits(&b));
        prop_assert_eq!(ab.tighten_resource_limits(&c), a.tighten_resource_limits(&b.tighten_resource_limits(&c)));
        prop_assert_eq!(a.tighten_resource_limits(&LocalDeleteOptions::new()), a);
        if let Some(limit) = a.max_depth() { prop_assert!(ab.max_depth().is_some_and(|value| value <= limit)); }
        if let Some(limit) = b.max_depth() { prop_assert!(ab.max_depth().is_some_and(|value| value <= limit)); }
    }
}

proptest! {
    /// The max_entries ceiling composes monotonically and without hidden defaults.
    #[test]
    fn test_localdeleteoptions_max_entries_intersection(
        a in of(any::<usize>()),
        b in of(any::<usize>()),
        c in of(any::<usize>()),
    ) {
        let build = |value: Option<usize>| value.map_or_else(LocalDeleteOptions::new,
            |value| LocalDeleteOptions::new().with_max_entries(value));
        let a = build(a);
        let b = build(b);
        let c = build(c);
        let ab = a.tighten_resource_limits(&b);
        prop_assert_eq!(ab, b.tighten_resource_limits(&a));
        prop_assert_eq!(ab, ab.tighten_resource_limits(&b));
        prop_assert_eq!(ab.tighten_resource_limits(&c), a.tighten_resource_limits(&b.tighten_resource_limits(&c)));
        prop_assert_eq!(a.tighten_resource_limits(&LocalDeleteOptions::new()), a);
        if let Some(limit) = a.max_entries() { prop_assert!(ab.max_entries().is_some_and(|value| value <= limit)); }
        if let Some(limit) = b.max_entries() { prop_assert!(ab.max_entries().is_some_and(|value| value <= limit)); }
    }
}

proptest! {
    /// The max_pending_path_bytes ceiling composes monotonically and without hidden defaults.
    #[test]
    fn test_localdeleteoptions_max_pending_path_bytes_intersection(
        a in of(any::<usize>()),
        b in of(any::<usize>()),
        c in of(any::<usize>()),
    ) {
        let build = |value: Option<usize>| value.map_or_else(LocalDeleteOptions::new,
            |value| LocalDeleteOptions::new().with_max_pending_path_bytes(value));
        let a = build(a);
        let b = build(b);
        let c = build(c);
        let ab = a.tighten_resource_limits(&b);
        prop_assert_eq!(ab, b.tighten_resource_limits(&a));
        prop_assert_eq!(ab, ab.tighten_resource_limits(&b));
        prop_assert_eq!(ab.tighten_resource_limits(&c), a.tighten_resource_limits(&b.tighten_resource_limits(&c)));
        prop_assert_eq!(a.tighten_resource_limits(&LocalDeleteOptions::new()), a);
        if let Some(limit) = a.max_pending_path_bytes() { prop_assert!(ab.max_pending_path_bytes().is_some_and(|value| value <= limit)); }
        if let Some(limit) = b.max_pending_path_bytes() { prop_assert!(ab.max_pending_path_bytes().is_some_and(|value| value <= limit)); }
    }
}

proptest! {
    /// The deadline ceiling composes monotonically and without hidden defaults.
    #[test]
    fn test_localdeleteoptions_deadline_intersection(
        a in of(any::<u64>()),
        b in of(any::<u64>()),
        c in of(any::<u64>()),
    ) {
        let build = |value: Option<u64>| value.map_or_else(LocalDeleteOptions::new,
            |value| LocalDeleteOptions::new().with_deadline(Duration::from_nanos(value)));
        let a = build(a);
        let b = build(b);
        let c = build(c);
        let ab = a.tighten_resource_limits(&b);
        prop_assert_eq!(ab, b.tighten_resource_limits(&a));
        prop_assert_eq!(ab, ab.tighten_resource_limits(&b));
        prop_assert_eq!(ab.tighten_resource_limits(&c), a.tighten_resource_limits(&b.tighten_resource_limits(&c)));
        prop_assert_eq!(a.tighten_resource_limits(&LocalDeleteOptions::new()), a);
        if let Some(limit) = a.deadline() { prop_assert!(ab.deadline().is_some_and(|value| value <= limit)); }
        if let Some(limit) = b.deadline() { prop_assert!(ab.deadline().is_some_and(|value| value <= limit)); }
    }
}

/// Tightening retains invalid zero handle limits for operation-time validation.
#[test]
fn test_tightened_zero_handles_are_rejected() {
    use qubit_local_files::LocalFileSystem;
    use qubit_local_files::error::LocalFileErrorKind;
    let dir = tempfile::tempdir().expect("isolated directory");
    let host = LocalFileSystem::host().expect("host filesystem");
    let options =
        LocalListOptions::new().tighten_resource_limits(&LocalListOptions::new().with_max_open_directories(0));
    let error = host
        .list_with_options(dir.path(), &options)
        .expect_err("zero handles are invalid");
    assert_eq!(error.kind(), LocalFileErrorKind::InvalidOptions);
}

/// Zero byte and entry ceilings remain enforced when a composed request runs.
#[test]
fn test_tightened_zero_limits_stop_copy_and_deletion() {
    use qubit_local_files::LocalFileSystem;
    use qubit_local_files::error::LocalFileErrorKind;
    let dir = tempfile::tempdir().expect("isolated directory");
    let source = dir.path().join("source");
    let target = dir.path().join("target");
    std::fs::write(&source, b"payload").expect("source fixture");
    let host = LocalFileSystem::host().expect("host filesystem");
    let copy = LocalCopyOptions::new().tighten_resource_limits(&LocalCopyOptions::new().with_max_bytes(0));
    let error = host
        .copy_with_options(&source, &target, &copy)
        .expect_err("zero bytes cannot copy payload");
    assert_eq!(error.error().kind(), LocalFileErrorKind::ResourceLimit);
    assert!(!target.exists());
    let delete = LocalDeleteOptions::new()
        .with_recursive()
        .tighten_resource_limits(&LocalDeleteOptions::new().with_max_entries(0));
    let error = host
        .delete_directory_with_options(dir.path(), &delete)
        .expect_err("zero entries cannot delete root entry");
    assert_eq!(error.kind(), LocalFileErrorKind::ResourceLimit);
    assert_eq!(std::fs::read(source).expect("source remains"), b"payload");
}
