// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

#![cfg(unix)]

use std::fs;
use std::path::Path;
use std::time::Duration;

use crate::LocalFileErrorKind;
use crate::LocalFileOperation;
use crate::LocalFileSystem;
use crate::options::LocalCopyOptions;
use crate::options::LocalDirectoryOpenOptions;
use crate::tests::rooted::support::resolution_observation_tests;

#[test]
fn directory_copy_deadline_covers_handle_path_prevalidation() {
    let fixture = tempfile::tempdir().expect("fixture should exist");
    fs::write(fixture.path().join("source"), b"source").expect("source should exist");
    let handle = LocalFileSystem::rooted(fixture.path())
        .expect("rooted filesystem should open")
        .open_directory_with_options(Path::new("/"), &LocalDirectoryOpenOptions::new())
        .expect("directory handle should open before arming the clock");

    resolution_observation_tests::reset();
    let _clock = resolution_observation_tests::DeadlineClockGuard::expire_after(0);
    let result = handle.copy_with_options(
        Path::new("source"),
        Path::new("copy"),
        &LocalCopyOptions::new().with_deadline(Duration::from_secs(60)),
    );

    assert!(
        result.is_err(),
        "the deadline should expire at the first resolver checkpoint"
    );
    let (metadata, directory_open, _) = resolution_observation_tests::snapshot();
    assert_eq!(metadata, 0, "expired prevalidation must do no metadata work");
    assert_eq!(
        directory_open, 0,
        "expired prevalidation must do no directory-open work"
    );
    assert!(!fixture.path().join("copy").exists());
}

#[test]
fn copy_started_at_helper_does_not_restart_an_expired_budget() {
    let fixture = tempfile::tempdir().expect("fixture should exist");
    fs::write(fixture.path().join("source"), b"source").expect("source should exist");
    let filesystem = LocalFileSystem::rooted(fixture.path()).expect("rooted filesystem should open");
    let started_at = std::time::Instant::now() - Duration::from_secs(60);

    let failure = filesystem
        .copy_with_options_started_at(
            Path::new("source"),
            Path::new("copy"),
            &LocalCopyOptions::new().with_deadline(Duration::from_secs(30)),
            started_at,
        )
        .expect_err("an expired start must remain expired in the backend");

    assert_eq!(failure.error().kind(), LocalFileErrorKind::Io);
    assert_eq!(failure.error().operation(), LocalFileOperation::Copy);
    assert_eq!(failure.error().io_error_kind(), std::io::ErrorKind::TimedOut);
    assert!(!fixture.path().join("copy").exists());
}
