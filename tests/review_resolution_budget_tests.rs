// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::path::Path;

use qubit_local_files::LocalFileSystem;
use qubit_local_files::error::LocalFileErrorKind;
use qubit_local_files::error::LocalResourceKind;
use qubit_local_files::options::LocalObservedReadOptions;
use qubit_local_files::options::LocalResolutionLimits;

fn observed_read_options(limit: usize) -> LocalObservedReadOptions {
    LocalObservedReadOptions::new()
        .with_resolution_limits(LocalResolutionLimits::new().with_max_resolved_path_bytes(limit))
}

fn assert_pending_byte_limit(error: &qubit_local_files::LocalFileError, limit: usize, observed: usize) {
    assert_eq!(error.kind(), LocalFileErrorKind::ResourceLimit);
    let limit_error = error
        .resource_limit_error()
        .expect("resource limit facts should be retained");
    assert_eq!(limit_error.resource(), LocalResourceKind::PendingResolutionBytes);
    assert_eq!(limit_error.limit(), limit);
    assert_eq!(limit_error.observed(), Some(observed));
}

#[cfg(unix)]
#[test]
fn pending_current_steps_use_the_native_component_byte_budget() {
    use std::os::unix::fs::symlink;

    let fixture = tempfile::tempdir().expect("fixture should be created");
    std::fs::write(fixture.path().join("x"), b"x").expect("target file should be created");
    symlink("././.", fixture.path().join("l")).expect("link should be created");
    let filesystem = LocalFileSystem::rooted(fixture.path()).expect("root should open without a budget");

    let error = filesystem
        .open_observed_reader(Path::new("l/x"), &observed_read_options(7))
        .expect_err("three current steps and the x suffix require eight bytes");
    assert_pending_byte_limit(&error, 7, 8);

    let mut reader = filesystem
        .open_observed_reader(Path::new("l/x"), &observed_read_options(8))
        .expect("the exact native byte budget should succeed");
    let contents = reader.read_to_end_limited(8).expect("target should be readable");
    assert_eq!(contents, b"x");
}

#[cfg(windows)]
#[test]
fn pending_current_steps_use_the_native_component_byte_budget() {
    use std::os::windows::fs::symlink_dir;

    let fixture = tempfile::tempdir().expect("fixture should be created");
    std::fs::write(fixture.path().join("x"), b"x").expect("target file should be created");
    symlink_dir(".", fixture.path().join("l")).expect("directory link should be created");
    let filesystem = LocalFileSystem::rooted(fixture.path()).expect("root should open without a budget");

    let error = filesystem
        .open_observed_reader(Path::new("l/x"), &observed_read_options(14))
        .expect_err("three current steps and the x suffix require sixteen bytes");
    assert_pending_byte_limit(&error, 14, 16);

    let mut reader = filesystem
        .open_observed_reader(Path::new("l/x"), &observed_read_options(16))
        .expect("the exact native byte budget should succeed");
    let contents = reader.read_to_end_limited(8).expect("target should be readable");
    assert_eq!(contents, b"x");
}

#[cfg(unix)]
#[test]
fn pending_parent_steps_use_three_native_bytes_each() {
    use std::os::unix::fs::symlink;

    let fixture = tempfile::tempdir().expect("fixture should be created");
    std::fs::create_dir_all(fixture.path().join("dir")).expect("link parent should be created");
    std::fs::create_dir_all(fixture.path().join("longname")).expect("target parent should be created");
    std::fs::write(fixture.path().join("longname/x"), b"x").expect("target file should be created");
    symlink("../longname", fixture.path().join("dir/l")).expect("link should be created");
    let filesystem = LocalFileSystem::rooted(fixture.path()).expect("root should open without a budget");

    let error = filesystem
        .open_observed_reader(Path::new("dir/l/x"), &observed_read_options(13))
        .expect_err("the expanded parent, long name, and x suffix require fourteen bytes");
    assert_pending_byte_limit(&error, 13, 14);
}

#[cfg(windows)]
#[test]
fn pending_parent_steps_use_six_native_bytes_each() {
    use std::os::windows::fs::symlink_dir;

    let fixture = tempfile::tempdir().expect("fixture should be created");
    std::fs::create_dir_all(fixture.path().join("dir")).expect("link parent should be created");
    std::fs::create_dir_all(fixture.path().join("longname")).expect("target parent should be created");
    std::fs::write(fixture.path().join("longname/x"), b"x").expect("target file should be created");
    symlink_dir("..\\longname", fixture.path().join("dir/l")).expect("directory link should be created");
    let filesystem = LocalFileSystem::rooted(fixture.path()).expect("root should open without a budget");

    let error = filesystem
        .open_observed_reader(Path::new("dir/l/x"), &observed_read_options(27))
        .expect_err("the expanded parent, long name, and x suffix require twenty-eight bytes");
    assert_pending_byte_limit(&error, 27, 28);
}
