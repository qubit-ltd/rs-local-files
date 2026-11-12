// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::fs;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use super::LocalDirectoryWalker;
use crate::LocalDirectoryReopenPolicy;
use crate::LocalListOptions;
use crate::LocalSymlinkPolicy;
use crate::LocalWalkErrorPolicy;

fn fixture() -> tempfile::TempDir {
    let directory = tempfile::tempdir().expect("walker fixture should be created");
    fs::create_dir(directory.path().join("first")).expect("first directory should be created");
    fs::create_dir(directory.path().join("second")).expect("second directory should be created");
    fs::write(directory.path().join("file"), b"file").expect("file should be written");
    directory
}

fn assert_released(walker: &LocalDirectoryWalker) {
    assert!(walker.terminated);
    assert!(walker.stack.is_empty());
    assert_eq!(0, walker.stack.capacity());
    assert!(walker.followed_directories.is_empty());
    if let Some(state) = walker.rooted.as_ref() {
        assert!(state.stack.is_empty());
        assert_eq!(0, state.stack.capacity());
        assert!(state.followed_directories.is_empty());
    }
    assert!(walker.open_directories.is_none());
}

fn host(directory: &tempfile::TempDir, options: LocalListOptions) -> LocalDirectoryWalker {
    LocalDirectoryWalker::open_with_diagnostic(
        directory.path().to_path_buf(),
        directory.path().to_path_buf(),
        options,
        LocalSymlinkPolicy::Reject,
        Instant::now(),
    )
    .expect("Host walker should open")
}

fn rooted(directory: &tempfile::TempDir, options: LocalListOptions) -> LocalDirectoryWalker {
    let root = crate::rooted::Root::open(directory.path()).expect("Rooted authority should open");
    LocalDirectoryWalker::open_rooted(
        Arc::new(root),
        None,
        PathBuf::from("/"),
        options,
        LocalSymlinkPolicy::Reject,
        Instant::now(),
    )
    .expect("Rooted walker should open")
}

#[test]
fn test_terminal_entry_budget_releases_host_and_rooted_frames() {
    let directory = fixture();
    for mut walker in [
        host(
            &directory,
            LocalListOptions::new()
                .with_max_entries(1)
                .with_max_open_directories(3)
                .with_error_policy(LocalWalkErrorPolicy::Continue),
        ),
        rooted(
            &directory,
            LocalListOptions::new()
                .with_max_entries(1)
                .with_max_open_directories(3)
                .with_error_policy(LocalWalkErrorPolicy::Continue),
        ),
    ] {
        assert!(walker.next().expect("first entry should be returned").is_ok());
        assert!(
            walker.stack.last().is_some_and(|frame| frame.entries.is_some())
                || walker
                    .rooted
                    .as_ref()
                    .is_some_and(|state| { state.stack.last().is_some_and(|frame| frame.reader.is_some()) }),
            "the active native reader must be open before the terminal step"
        );
        assert!(walker.next().expect("entry budget error should be returned").is_err());
        assert_released(&walker);
        assert!(walker.next().is_none());
    }
}

#[test]
#[cfg(feature = "test-support")]
fn test_fail_fast_releases_both_backend_states() {
    let directory = fixture();
    for rooted_backend in [false, true] {
        let options = LocalListOptions::new()
            .with_max_open_directories(3)
            .with_error_policy(LocalWalkErrorPolicy::FailFast);
        let mut walker = if rooted_backend {
            rooted(&directory, options)
        } else {
            host(&directory, options)
        };
        let _fault =
            crate::test_support::install_test_fault("walker-entry").expect("reader error fault should install");
        assert!(walker.next().expect("reader error should be returned").is_err());
        assert_released(&walker);
        assert!(walker.next().is_none());
    }
}

#[test]
fn test_expired_deadline_releases_both_backend_states() {
    let directory = fixture();
    for rooted_backend in [false, true] {
        let mut walker = if rooted_backend {
            rooted(&directory, LocalListOptions::new().with_max_open_directories(3))
        } else {
            host(&directory, LocalListOptions::new().with_max_open_directories(3))
        };
        walker.deadline = Some(Instant::now() - std::time::Duration::from_nanos(1));
        let error = walker.next().expect("deadline error should be returned").unwrap_err();
        assert_eq!(std::io::ErrorKind::TimedOut, error.io_error_kind());
        assert_released(&walker);
        assert!(walker.next().is_none());
    }
}

#[test]
fn test_zero_depth_and_normal_exhaustion_release_frames() {
    let directory = fixture();
    let mut zero_depth = host(&directory, LocalListOptions::new().with_max_depth(0));
    assert!(zero_depth.next().is_none());
    assert_released(&zero_depth);

    let mut exhausted = rooted(
        &directory,
        LocalListOptions::new().with_reopen_policy(LocalDirectoryReopenPolicy::Fail),
    );
    while exhausted.next().is_some() {}
    assert_released(&exhausted);
}
