// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::ffi::OsString;
use std::fs;
use std::path::Path;

use qubit_local_files::LocalFileSystem;
use qubit_local_files::directory::LocalComponentPolicy;
use qubit_local_files::error::LocalFileErrorKind;
use qubit_local_files::options::LocalDirectoryOpenOptions;
use tempfile::tempdir;

fn component_policy(names: &[&str]) -> LocalComponentPolicy {
    LocalComponentPolicy::deny_names(names.iter().map(OsString::from).collect())
        .expect("fixture names should form a valid component policy")
}

fn rooted_handle(path: &Path, policy: LocalComponentPolicy) -> qubit_local_files::directory::LocalDirectoryHandle {
    LocalFileSystem::rooted(path)
        .expect("Rooted filesystem should open")
        .open_directory_with_options(
            Path::new("/"),
            &LocalDirectoryOpenOptions::new().with_component_policy(policy),
        )
        .expect("Rooted authority handle should open")
}

#[test]
fn test_child_directory_retains_parent_denied_names() {
    let fixture = tempdir().expect("fixture directory should exist");
    fs::create_dir(fixture.path().join("allowed")).expect("allowed directory should exist");
    fs::create_dir(fixture.path().join("blocked")).expect("blocked directory should exist");

    let parent = rooted_handle(fixture.path(), component_policy(&["blocked"]));
    assert_eq!(
        LocalFileErrorKind::InvalidPath,
        parent
            .open_directory(Path::new("blocked"), &LocalDirectoryOpenOptions::new())
            .expect_err("parent denied name should be rejected")
            .kind()
    );

    let child = parent
        .open_directory(Path::new("allowed"), &LocalDirectoryOpenOptions::new())
        .expect("allowed child should open");
    assert_eq!(
        LocalFileErrorKind::InvalidPath,
        child
            .open_directory(Path::new("blocked"), &LocalDirectoryOpenOptions::new())
            .expect_err("child must retain the parent denied name")
            .kind()
    );
    assert_eq!(
        LocalFileErrorKind::InvalidPath,
        parent
            .open_directory(Path::new("blocked"), &LocalDirectoryOpenOptions::new())
            .expect_err("opening a child must not mutate the parent")
            .kind()
    );
}

#[test]
fn test_opening_dot_cannot_clear_parent_denied_names() {
    let fixture = tempdir().expect("fixture directory should exist");
    fs::create_dir(fixture.path().join("blocked")).expect("blocked directory should exist");

    let parent = rooted_handle(fixture.path(), component_policy(&["blocked"]));
    let derived = parent
        .open_directory(Path::new("."), &LocalDirectoryOpenOptions::new())
        .expect("opening dot should preserve the same directory authority");

    assert_eq!(
        LocalFileErrorKind::InvalidPath,
        derived
            .open_directory(Path::new("blocked"), &LocalDirectoryOpenOptions::new())
            .expect_err("dot-derived handle must retain parent denied names")
            .kind()
    );
}

#[test]
fn test_child_directory_combines_parent_and_child_denied_names() {
    let fixture = tempdir().expect("fixture directory should exist");
    let allowed = fixture.path().join("allowed");
    fs::create_dir(&allowed).expect("allowed directory should exist");
    fs::create_dir(allowed.join("parent-denied")).expect("parent denied directory should exist");
    fs::create_dir(allowed.join("child-denied")).expect("child denied directory should exist");

    let parent = rooted_handle(fixture.path(), component_policy(&["parent-denied"]));
    let child = parent
        .open_directory(
            Path::new("allowed"),
            &LocalDirectoryOpenOptions::new().with_component_policy(component_policy(&["child-denied"])),
        )
        .expect("allowed child should open");

    for name in ["parent-denied", "child-denied"] {
        assert_eq!(
            LocalFileErrorKind::InvalidPath,
            child
                .open_directory(Path::new(name), &LocalDirectoryOpenOptions::new())
                .expect_err("parent and child denied names should both remain denied")
                .kind()
        );
    }
}

#[test]
fn test_allowed_child_directory_opens_and_retains_authority() {
    let fixture = tempdir().expect("fixture directory should exist");
    fs::create_dir(fixture.path().join("allowed")).expect("allowed directory should exist");

    let parent = rooted_handle(fixture.path(), component_policy(&["blocked"]));
    let child = parent
        .open_directory(Path::new("allowed"), &LocalDirectoryOpenOptions::new())
        .expect("allowed child should open");
    assert!(
        child
            .entry_metadata(Path::new("."))
            .expect("child metadata should resolve")
            .is_some()
    );
}

#[cfg(unix)]
#[test]
fn test_symlink_expansion_checks_denied_names_before_parent_collapse() {
    use std::os::unix::fs::symlink;

    let fixture = tempdir().expect("fixture directory should exist");
    fs::create_dir(fixture.path().join("allowed")).expect("allowed directory should exist");
    fs::create_dir(fixture.path().join("hidden")).expect("hidden directory should exist");
    symlink("hidden/../allowed", fixture.path().join("expand")).expect("expansion symlink should be created");

    let parent = rooted_handle(fixture.path(), component_policy(&["hidden"]));
    assert_eq!(
        LocalFileErrorKind::InvalidPath,
        parent
            .open_directory(
                Path::new("expand"),
                &LocalDirectoryOpenOptions::new()
                    .with_symlink_policy(qubit_local_files::policy::LocalSymlinkPolicy::FollowWithinScope),
            )
            .expect_err("expanded denied component must be rejected before '..' collapses it")
            .kind()
    );
}
