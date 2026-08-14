// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::fs;
use std::path::Path;
use std::path::PathBuf;
use std::time::Duration;

use qubit_local_files::LocalFileErrorKind;
use qubit_local_files::LocalFileKind;
use qubit_local_files::LocalFileSystem;
use qubit_local_files::LocalListOptions;
#[cfg(unix)]
use qubit_local_files::LocalSymlinkPolicy;
use qubit_local_files::LocalWalkErrorPolicy;
#[cfg(feature = "internal-test-support")]
use qubit_local_files::install_test_fault;
use tempfile::tempdir;

/// Runs a test-support-only fault case in an isolated child test process.
#[cfg(feature = "internal-test-support")]
fn run_walker_fault_process<F>(test_name: &str, fault: &str, action: F)
where
    F: FnOnce(),
{
    const TEST_FAULT_ENV: &str = "QUBIT_LOCAL_FILES_TEST_FAULT";
    const TEST_FAULT_CHILD_ENV: &str = "QUBIT_LOCAL_FILES_TEST_FAULT_CHILD";
    if std::env::var_os(TEST_FAULT_ENV)
        .is_some_and(|selected| selected == std::ffi::OsStr::new(fault))
    {
        let _fault = install_test_fault(fault)
            .expect("test fault controller should install");
        action();
        return;
    }
    if std::env::var_os(TEST_FAULT_CHILD_ENV).is_some() {
        return;
    }
    let executable =
        std::env::current_exe().expect("current test executable should exist");
    let status = std::process::Command::new(executable)
        .arg("--exact")
        .arg(test_name)
        .arg("--nocapture")
        .env(TEST_FAULT_ENV, fault)
        .env(TEST_FAULT_CHILD_ENV, "1")
        .status()
        .expect("test fault child should launch");
    assert!(status.success(), "test fault child should pass");
}

/// Resolves the existing prefix of a path while preserving a missing leaf.
///
/// macOS commonly exposes `/var` through the `/private/var` symlink, so the
/// bound path used by the filesystem can differ lexically from the fixture
/// path even when both paths name the same entry.
#[cfg(target_os = "macos")]
fn bound_path(path: &Path) -> PathBuf {
    let mut missing = Vec::new();
    let mut current = path;
    loop {
        if let Ok(mut resolved) = fs::canonicalize(current) {
            for component in missing.iter().rev() {
                resolved.push(component);
            }
            return resolved;
        }
        let Some(name) = current.file_name() else {
            return path.to_path_buf();
        };
        missing.push(name.to_owned());
        let Some(parent) = current.parent() else {
            return path.to_path_buf();
        };
        current = parent;
    }
}

#[cfg(not(target_os = "macos"))]
fn bound_path(path: &Path) -> PathBuf {
    path.to_path_buf()
}

/// Asserts that a diagnostic path matches the filesystem's bound spelling.
fn assert_bound_path(expected: &Path, actual: Option<&Path>) {
    let expected = bound_path(expected);
    assert_eq!(Some(expected.as_path()), actual);
}

/// Verifies non-recursive traversal returns only immediate entries and retains
/// the bound root path for diagnostics.
#[test]
fn test_local_directory_walker_non_recursive_listing_retains_bound_root() {
    let directory = tempdir().expect("temporary directory should be created");
    fs::create_dir(directory.path().join("nested"))
        .expect("nested directory should be created");
    fs::write(directory.path().join("nested/child"), b"child")
        .expect("nested child should be written");
    fs::write(directory.path().join("top"), b"top")
        .expect("top-level fixture should be written");

    let walker = LocalFileSystem::host()
        .list(directory.path(), &LocalListOptions::new())
        .expect("directory should open for listing");
    assert_eq!(bound_path(directory.path()), walker.root());
    let mut entries = walker
        .collect::<Result<Vec<_>, _>>()
        .expect("non-recursive traversal should succeed");
    entries
        .sort_by(|left, right| left.relative_path().cmp(right.relative_path()));

    assert_eq!(2, entries.len());
    assert_eq!(PathBuf::from("nested"), entries[0].relative_path());
    assert_eq!(LocalFileKind::Directory, entries[0].metadata().kind());
    assert_eq!(PathBuf::from("top"), entries[1].relative_path());
    assert_eq!(
        bound_path(&directory.path().join("top")),
        entries[1].diagnostic_path(),
    );
}

/// Verifies a regular file cannot be opened as a directory traversal root.
#[test]
fn test_local_directory_walker_rejects_regular_file_root() {
    let directory = tempdir().expect("temporary directory should be created");
    let file = directory.path().join("file");
    fs::write(&file, b"payload").expect("file fixture should be written");

    let error = LocalFileSystem::host()
        .list(&file, &LocalListOptions::new())
        .expect_err("regular files must not open as directory walkers");

    assert_eq!(LocalFileErrorKind::TypeConflict, error.kind());
    assert_bound_path(&file, error.path());
}

/// Verifies opening a missing traversal root preserves the path-specific
/// not-found classification.
#[test]
fn test_local_directory_walker_rejects_missing_root() {
    let directory = tempdir().expect("temporary directory should be created");
    let missing = directory.path().join("missing");

    let error = LocalFileSystem::host()
        .list(&missing, &LocalListOptions::new())
        .expect_err("missing directories must not open as walkers");

    assert_eq!(LocalFileErrorKind::NotFound, error.kind());
    assert_bound_path(&missing, error.path());
}

/// Verifies a zero traversal depth yields no entries, including entries that
/// would otherwise be visible at the root level.
#[test]
fn test_local_directory_walker_zero_max_depth_yields_no_entries() {
    let directory = tempdir().expect("temporary directory should be created");
    fs::write(directory.path().join("entry"), b"payload")
        .expect("entry fixture should be written");

    let entries = LocalFileSystem::host()
        .list(directory.path(), &LocalListOptions::new().with_max_depth(0))
        .expect("walker should open")
        .collect::<Result<Vec<_>, _>>()
        .expect("zero-depth traversal should succeed");

    assert!(entries.is_empty());
}

/// Verifies zero depth is applied before entry and name budgets.
#[test]
fn test_local_directory_walker_zero_depth_does_not_consume_yield_budgets() {
    let directory = tempdir().expect("temporary directory should be created");
    fs::write(directory.path().join("entry"), b"payload")
        .expect("entry fixture should be written");

    let entries = LocalFileSystem::host()
        .list(
            directory.path(),
            &LocalListOptions::new()
                .with_max_depth(0)
                .with_max_entries(0)
                .with_max_seen_name_bytes(0),
        )
        .expect("walker should open")
        .collect::<Result<Vec<_>, _>>()
        .expect("zero-depth traversal should not consume yield budgets");

    assert!(entries.is_empty());
}

/// Verifies an exhausted global resource budget terminates Continue traversal.
#[test]
fn test_local_directory_walker_resource_limit_terminates_continue_policy() {
    let directory = tempdir().expect("temporary directory should be created");
    fs::write(directory.path().join("first"), b"first")
        .expect("first entry should be written");
    fs::write(directory.path().join("second"), b"second")
        .expect("second entry should be written");
    let mut walker = LocalFileSystem::host()
        .list(
            directory.path(),
            &LocalListOptions::new()
                .with_max_entries(0)
                .with_error_policy(LocalWalkErrorPolicy::Continue),
        )
        .expect("walker should open");

    assert!(
        walker
            .next()
            .expect("resource limit should be yielded")
            .is_err(),
    );
    assert!(
        walker.next().is_none(),
        "resource exhaustion must terminate"
    );
}

/// Verifies an unrepresentable monotonic deadline is rejected without panic.
#[test]
fn test_local_directory_walker_rejects_unrepresentable_deadline() {
    let directory = tempdir().expect("temporary directory should be created");

    let error = LocalFileSystem::host()
        .list(
            directory.path(),
            &LocalListOptions::new().with_deadline(Duration::MAX),
        )
        .expect_err("unrepresentable deadline should be invalid");

    assert_eq!(LocalFileErrorKind::InvalidOptions, error.kind());
}

/// Verifies zero directory-handle budgets are rejected instead of being
/// silently rewritten to one.
#[test]
fn test_local_directory_walker_rejects_zero_open_directory_budget() {
    let directory = tempdir().expect("temporary directory should be created");

    let error = LocalFileSystem::host()
        .list(
            directory.path(),
            &LocalListOptions::new().with_max_open_directories(0),
        )
        .expect_err("zero directory-handle budgets must be invalid");

    assert_eq!(LocalFileErrorKind::InvalidOptions, error.kind());
    assert_bound_path(directory.path(), error.path());
    assert_eq!(
        Some("maximum open directory count must be greater than zero"),
        error.reason(),
    );
}

/// Verifies follow-mode resolves a symlinked directory and detects a traversal
/// cycle rather than looping indefinitely.
#[cfg(unix)]
#[test]
fn test_local_directory_walker_follow_mode_traverses_links_and_rejects_cycles()
{
    use std::os::unix::fs::symlink;

    let directory = tempdir().expect("temporary directory should be created");
    let outside = tempdir().expect("outside directory should be created");
    let target = outside.path().join("target");
    fs::create_dir(&target)
        .expect("outside target directory should be created");
    fs::write(target.join("child"), b"payload")
        .expect("target child should be written");
    symlink(&target, directory.path().join("link"))
        .expect("directory link should be created");

    let entries = LocalFileSystem::host()
        .list(
            directory.path(),
            &LocalListOptions::new()
                .with_recursive()
                .with_symlink_policy(LocalSymlinkPolicy::FollowWithinScope),
        )
        .expect("follow-mode walker should open")
        .collect::<Result<Vec<_>, _>>()
        .expect("one symlinked directory should be traversable");
    assert!(
        entries
            .iter()
            .any(|entry| entry.relative_path() == "link/child")
    );

    symlink(directory.path(), target.join("cycle"))
        .expect("cycle link should be created");
    let error = LocalFileSystem::host()
        .list(
            directory.path(),
            &LocalListOptions::new()
                .with_recursive()
                .with_symlink_policy(LocalSymlinkPolicy::FollowWithinScope),
        )
        .expect("cycle walker should open")
        .find_map(Result::err)
        .expect("cycle detection should return a structured error");

    assert_eq!(LocalFileErrorKind::InvalidPath, error.kind());
}

/// Verifies follow-mode reports the dangling-link metadata failure at the
/// entry itself instead of treating the unresolved link as an ordinary file.
#[cfg(unix)]
#[test]
fn test_local_directory_walker_follow_mode_reports_dangling_link() {
    use std::os::unix::fs::symlink;

    let directory = tempdir().expect("temporary directory should be created");
    let link = directory.path().join("dangling");
    symlink(directory.path().join("missing"), &link)
        .expect("dangling link fixture should be created");

    let error = LocalFileSystem::host()
        .list(
            directory.path(),
            &LocalListOptions::new()
                .with_symlink_policy(LocalSymlinkPolicy::FollowWithinScope),
        )
        .expect("follow-mode walker should open")
        .next()
        .expect("dangling entry should be observed")
        .expect_err("follow-mode walker must resolve a symlink target");

    assert_eq!(LocalFileErrorKind::NotFound, error.kind());
    assert_bound_path(&link, error.path());
}

/// Verifies recursive traversal reports native child-directory opening errors
/// after yielding the readable parent entry.
#[cfg(unix)]
#[test]
fn test_local_directory_walker_reports_unreadable_child_directory() {
    use std::os::unix::fs::PermissionsExt;

    // SAFETY: `geteuid` reads the current process identity without pointers or
    // mutable state.
    if unsafe { libc::geteuid() } == 0 {
        return;
    }
    let directory = tempdir().expect("temporary directory should be created");
    let child = directory.path().join("restricted");
    fs::create_dir(&child).expect("restricted child should be created");
    fs::write(child.join("entry"), b"payload")
        .expect("restricted child fixture should be written");
    fs::set_permissions(&child, fs::Permissions::from_mode(0o000))
        .expect("restricted child should become unreadable");

    let mut walker = LocalFileSystem::host()
        .list(directory.path(), &LocalListOptions::new().with_recursive())
        .expect("recursive walker should open before entering its child");
    let result = walker
        .next()
        .expect("restricted directory entry should be read from its parent");
    fs::set_permissions(&child, fs::Permissions::from_mode(0o700))
        .expect("restricted child permissions should be restored");

    let error =
        result.expect_err("unreadable child descent must return an error");
    assert_eq!(LocalFileErrorKind::PermissionDenied, error.kind());
    assert_bound_path(&child, error.path());
}

/// Verifies opening an unreadable traversal root reports the native directory
/// enumeration failure rather than constructing an empty walker.
#[cfg(unix)]
#[test]
fn test_local_directory_walker_rejects_unreadable_root_directory() {
    use std::os::unix::fs::PermissionsExt;

    // SAFETY: `geteuid` reads the current process identity without pointers or
    // mutable state.
    if unsafe { libc::geteuid() } == 0 {
        return;
    }
    let directory = tempdir().expect("temporary directory should be created");
    let root = directory.path().join("restricted-root");
    fs::create_dir(&root).expect("restricted root should be created");
    fs::set_permissions(&root, fs::Permissions::from_mode(0o000))
        .expect("restricted root should become unreadable");

    let error = LocalFileSystem::host()
        .list(&root, &LocalListOptions::new())
        .expect_err("unreadable traversal root must not open a walker");
    fs::set_permissions(&root, fs::Permissions::from_mode(0o700))
        .expect("restricted root permissions should be restored");

    assert_eq!(LocalFileErrorKind::PermissionDenied, error.kind());
    assert_bound_path(&root, error.path());
}

/// Verifies the default fail-fast policy fuses a walker after its first
/// iteration error.
#[cfg(unix)]
#[test]
fn test_local_directory_walker_fail_fast_stops_after_error() {
    use std::os::unix::fs::symlink;

    let directory = tempdir().expect("temporary directory should be created");
    symlink(
        directory.path().join("missing"),
        directory.path().join("dangling"),
    )
    .expect("dangling link fixture should be created");

    let mut walker = LocalFileSystem::host()
        .list(
            directory.path(),
            &LocalListOptions::new()
                .with_symlink_policy(LocalSymlinkPolicy::FollowWithinScope),
        )
        .expect("walker should open");
    assert!(
        walker
            .next()
            .expect("dangling link should produce an error")
            .is_err()
    );
    assert!(walker.next().is_none());
}

/// Verifies the continue policy publishes later entries after an iteration
/// error instead of fusing the walker.
#[cfg(unix)]
#[test]
fn test_local_directory_walker_continue_policy_keeps_iterating() {
    use std::os::unix::fs::symlink;

    let directory = tempdir().expect("temporary directory should be created");
    fs::write(directory.path().join("readable"), b"payload")
        .expect("readable entry should be created");
    symlink(
        directory.path().join("missing"),
        directory.path().join("dangling"),
    )
    .expect("dangling link fixture should be created");

    let walker = LocalFileSystem::host()
        .list(
            directory.path(),
            &LocalListOptions::new()
                .with_symlink_policy(LocalSymlinkPolicy::FollowWithinScope)
                .with_error_policy(LocalWalkErrorPolicy::Continue),
        )
        .expect("walker should open");
    let mut saw_error = false;
    let mut saw_readable = false;
    for result in walker {
        match result {
            Ok(entry) if entry.relative_path() == "readable" => {
                saw_readable = true;
            }
            Err(error) => {
                assert_eq!(LocalFileErrorKind::NotFound, error.kind());
                saw_error = true;
            }
            Ok(_) => {}
        }
    }
    assert!(saw_error);
    assert!(saw_readable);
}

/// Verifies recursive traversal uses stable native directory identities.
#[cfg(feature = "internal-test-support")]
#[test]
fn test_local_directory_walker_detects_native_directory_identity_cycle() {
    const TEST_NAME: &str =
        "test_local_directory_walker_detects_native_directory_identity_cycle";
    run_walker_fault_process(
        TEST_NAME,
        "walker-directory-identity-cycle",
        || {
            let directory =
                tempdir().expect("temporary directory should be created");
            fs::create_dir(directory.path().join("nested"))
                .expect("nested directory should be created");
            let error = LocalFileSystem::host()
                .list(
                    directory.path(),
                    &LocalListOptions::new().with_recursive(),
                )
                .expect("walker should open")
                .find_map(Result::err)
                .expect("forced directory identity collision should fail");

            assert_eq!(LocalFileErrorKind::InvalidPath, error.kind());
        },
    );
}
