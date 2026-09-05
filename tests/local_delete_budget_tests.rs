// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Recursive deletion budget contracts across native namespaces.

use std::fs;
use std::path::Path;
use std::time::Duration;

use qubit_local_files::LocalFileSystem;
use qubit_local_files::error::LocalFileErrorKind;
use qubit_local_files::error::LocalResourceKind;
use qubit_local_files::options::LocalDeleteOptions;
use tempfile::tempdir;

/// Every configured recursive limit fails before an unbudgeted mutation.
#[test]
fn test_recursive_delete_rejects_exhausted_budgets_before_mutation() {
    for rooted in [false, true] {
        for options in [
            LocalDeleteOptions::new().with_recursive().with_max_entries(0),
            LocalDeleteOptions::new().with_recursive().with_max_depth(0),
            LocalDeleteOptions::new()
                .with_recursive()
                .with_max_pending_path_bytes(0),
            LocalDeleteOptions::new().with_recursive().with_deadline(Duration::ZERO),
        ] {
            let fixture = tempdir().expect("fixture should exist");
            let tree = fixture.path().join("tree");
            fs::create_dir(&tree).expect("tree should exist");
            fs::write(tree.join("child"), b"payload").expect("child should exist");
            let filesystem = if rooted {
                LocalFileSystem::rooted(fixture.path())
            } else {
                LocalFileSystem::host()
            }
            .expect("filesystem should open");
            let operand = if rooted { Path::new("tree") } else { tree.as_path() };
            let error = filesystem
                .delete_directory_with_options(operand, &options)
                .expect_err("exhausted budget must stop deletion");
            if options.deadline().is_some() {
                assert_eq!(std::io::ErrorKind::TimedOut, error.io_error_kind());
            } else {
                assert_eq!(LocalFileErrorKind::ResourceLimit, error.kind());
                assert!(error.resource_limit_error().is_some());
            }
            assert_eq!(
                b"payload",
                fs::read(tree.join("child")).expect("child must remain").as_slice()
            );
        }
    }
}

/// Enumeration itself consumes the entry budget and preserves partial effects.
#[test]
fn test_recursive_delete_budget_failure_retains_partial_publication() {
    for rooted in [false, true] {
        let fixture = tempdir().expect("fixture should exist");
        let tree = fixture.path().join("tree");
        for name in ["first", "second"] {
            fs::create_dir_all(tree.join(name)).expect("branch should exist");
            fs::write(tree.join(name).join("payload"), b"data").expect("payload should exist");
        }
        let filesystem = if rooted {
            LocalFileSystem::rooted(fixture.path())
        } else {
            LocalFileSystem::host()
        }
        .expect("filesystem should open");
        let operand = if rooted { Path::new("tree") } else { tree.as_path() };
        let options = LocalDeleteOptions::new().with_recursive().with_max_entries(4);
        let error = filesystem
            .delete_directory_with_options(operand, &options)
            .expect_err("second branch exceeds budget");
        assert_eq!(LocalFileErrorKind::PublicationIncomplete, error.kind());
        let resource = error
            .resource_limit_error()
            .expect("resource facts must survive partial deletion");
        assert_eq!(LocalResourceKind::Entry, resource.resource());
        assert_eq!(4, resource.limit());
        assert_eq!(0, resource.remaining());
        assert_eq!(1, fs::read_dir(&tree).expect("root remains").count());
        let _ = filesystem
            .delete_directory_with_options(operand, &LocalDeleteOptions::new().with_recursive())
            .expect("unbounded retry should remove remainder");
        assert!(!tree.exists());
    }
}

/// Exact budgets allow complete deletion, and explicit options replace
/// defaults.
#[test]
fn test_recursive_delete_exact_budget_and_explicit_override() {
    let fixture = tempdir().expect("fixture should exist");
    let mut filesystem = LocalFileSystem::rooted(fixture.path()).expect("Rooted should open");
    filesystem
        .set_default_delete_options(LocalDeleteOptions::new().with_recursive().with_max_entries(0))
        .expect("defaults should be accepted");
    fs::create_dir(fixture.path().join("tree")).expect("tree should exist");
    fs::write(fixture.path().join("tree/child"), b"data").expect("child should exist");
    let _ = filesystem
        .delete_directory(Path::new("tree"))
        .expect_err("default budget should reject work");
    let options = LocalDeleteOptions::new()
        .with_recursive()
        .with_max_depth(1)
        .with_max_entries(2)
        .with_max_pending_path_bytes(14)
        .with_deadline(Duration::from_secs(60));
    let _ = filesystem
        .delete_directory_with_options(Path::new("tree"), &options)
        .expect("explicit budget should replace defaults");
    assert!(!fixture.path().join("tree").exists());
}

/// Pending path capacity is checked while enumerating, before a wide root is
/// removed.
#[test]
fn test_recursive_delete_bounds_pending_paths_during_enumeration() {
    for rooted in [false, true] {
        let fixture = tempdir().expect("fixture should exist");
        let tree = fixture.path().join("tree");
        fs::create_dir(&tree).expect("tree should exist");
        for name in ["one", "two", "six"] {
            fs::write(tree.join(name), b"data").expect("child should exist");
        }
        let filesystem = if rooted {
            LocalFileSystem::rooted(fixture.path())
        } else {
            LocalFileSystem::host()
        }
        .expect("filesystem should open");
        let operand = if rooted { Path::new("tree") } else { tree.as_path() };
        let capacity = operand.as_os_str().len() + operand.join("one").as_os_str().len();
        let options = LocalDeleteOptions::new()
            .with_recursive()
            .with_max_pending_path_bytes(capacity);
        let error = filesystem
            .delete_directory_with_options(operand, &options)
            .expect_err("second queued child should exceed capacity");
        assert_eq!(LocalFileErrorKind::ResourceLimit, error.kind());
        let facts = error.resource_limit_error().expect("pending-path facts should survive");
        assert_eq!(LocalResourceKind::PendingPathBytes, facts.resource());
        assert_eq!(capacity, facts.limit());
        assert_eq!(0, facts.remaining());
        assert_eq!(3, fs::read_dir(&tree).expect("root should remain").count());
    }
}

/// Defaults are unbounded and every independent limit can be explicitly
/// removed.
#[test]
fn test_delete_options_limits_are_opt_in_and_removable() {
    let defaults = LocalDeleteOptions::default();
    assert_eq!(None, defaults.max_depth());
    assert_eq!(None, defaults.max_entries());
    assert_eq!(None, defaults.max_pending_path_bytes());
    assert_eq!(None, defaults.deadline());
    let limits = defaults
        .with_max_depth(1)
        .with_max_entries(2)
        .with_max_pending_path_bytes(3)
        .with_deadline(Duration::from_secs(4));
    assert_eq!(Some(1), limits.max_depth());
    assert_eq!(Some(2), limits.max_entries());
    assert_eq!(Some(3), limits.max_pending_path_bytes());
    assert_eq!(Some(Duration::from_secs(4)), limits.deadline());
    assert_eq!(
        defaults,
        limits
            .without_max_depth()
            .without_max_entries()
            .without_max_pending_path_bytes()
            .without_deadline()
    );
}

/// Deadline expiry at traversal checkpoints preserves known deletion effects.
#[cfg(feature = "test-support")]
#[test]
fn test_recursive_delete_deadlines_preserve_partial_effects() {
    use qubit_local_files::test_support::install_test_fault;

    for rooted in [false, true] {
        // Rooted explicitly checks the final end-of-directory read as well.
        let removal_check = if rooted { 7 } else { 6 };
        for checkpoint in [2, 3, removal_check, removal_check + 1] {
            let fixture = tempdir().expect("fixture should exist");
            let tree = fixture.path().join("tree");
            fs::create_dir(&tree).expect("tree should exist");
            let child = tree.join("child");
            fs::write(&child, b"payload").expect("child should exist");
            let filesystem = if rooted {
                LocalFileSystem::rooted(fixture.path())
            } else {
                LocalFileSystem::host()
            }
            .expect("filesystem should open");
            let operand = if rooted { Path::new("tree") } else { tree.as_path() };
            let options = LocalDeleteOptions::new()
                .with_recursive()
                .with_deadline(Duration::from_secs(60));
            let fault = format!("local-delete-deadline-{checkpoint}");
            let _fault = install_test_fault(&fault).expect("deadline fault should install");
            let error = filesystem
                .delete_directory_with_options(operand, &options)
                .expect_err("deadline must stop recursive deletion");
            assert_eq!(std::io::ErrorKind::TimedOut, error.io_error_kind());
            assert!(tree.exists(), "unfinished root must remain");
            if checkpoint > removal_check {
                assert_eq!(LocalFileErrorKind::PublicationIncomplete, error.kind());
                assert!(!child.exists(), "completed deletion must not be rolled back");
            } else {
                assert_ne!(LocalFileErrorKind::PublicationIncomplete, error.kind());
                assert!(child.exists(), "deadline must precede mutation");
            }
        }
    }
}
