// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

//! Native no-follow observations for symbolic-link metadata during copy.

#![cfg(any(target_os = "macos", windows))]

use std::fs;
use std::path::Path;

use qubit_local_files::LocalFileSystem;
use qubit_local_files::options::LocalCopyOptions;
use qubit_local_files::options::LocalMetadataPreservePolicy;
use tempfile::tempdir;

#[cfg(windows)]
mod support;

fn symlink_file(target: &Path, link: &Path) -> bool {
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(target, link).expect("file symlink fixture should be created");
        true
    }
    #[cfg(windows)]
    {
        support::windows_symlink::create_file_link(target, link)
    }
}

fn options(tree: bool) -> LocalCopyOptions {
    let options = LocalCopyOptions::new().with_metadata_preservation(LocalMetadataPreservePolicy::Permissions);
    if tree { options.with_tree_source() } else { options }
}

fn assert_link_observation(source: &Path, target: &Path, referent: &Path) {
    let source_metadata =
        fs::symlink_metadata(source).expect("source link metadata should be observed without following");
    let target_metadata =
        fs::symlink_metadata(target).expect("target link metadata should be observed without following");
    let referent_metadata = fs::metadata(referent).expect("referent metadata should be observed with following");
    assert!(source_metadata.file_type().is_symlink());
    assert!(target_metadata.file_type().is_symlink());
    assert!(!referent_metadata.file_type().is_symlink());
    assert!(referent_metadata.permissions().readonly());
    assert_ne!(
        source_metadata.permissions().readonly(),
        referent_metadata.permissions().readonly(),
        "source link metadata must differ from the read-only referent",
    );
    assert_eq!(fs::read_link(source).unwrap(), fs::read_link(target).unwrap());
    assert_eq!(b"referent", fs::read(referent).unwrap().as_slice());
    eprintln!(
        "no-follow readonly observations: source={}, target={}, referent={}",
        source_metadata.permissions().readonly(),
        target_metadata.permissions().readonly(),
        referent_metadata.permissions().readonly(),
    );
}

fn set_readonly(path: &Path, readonly: bool) {
    // `metadata` and `set_permissions` follow symlinks. This intentionally
    // changes the referent only; the test does not claim to configure link
    // permissions independently, so it cannot establish Q03 preservation.
    let mut permissions = fs::metadata(path)
        .expect("referent metadata should be available")
        .permissions();
    permissions.set_readonly(readonly);
    fs::set_permissions(path, permissions).expect("referent permissions should be updated");
}

/// Verifies Host copy inspects a single link entry without changing its
/// referent.
#[test]
fn native_host_copy_does_not_use_single_symlink_referent_permissions() {
    let directory = tempdir().expect("temporary directory should be created");
    let referent = directory.path().join("referent");
    let source = directory.path().join("source-link");
    let target = directory.path().join("target-link");
    fs::write(&referent, b"referent").expect("referent should be written");
    set_readonly(&referent, true);
    assert!(
        symlink_file(Path::new("referent"), &source),
        "file symlink fixture should be available"
    );

    let outcome = LocalFileSystem::host()
        .expect("Host filesystem should open")
        .copy_with_options(&source, &target, &options(false))
        .expect("Host should copy the link entry");

    eprintln!(
        "requested Permissions; reported metadata policy: {:?}",
        outcome.metadata_preservation()
    );
    assert_eq!(1, outcome.stats().files());
    assert_link_observation(&source, &target, &referent);
    set_readonly(&referent, false);
}

/// Verifies Rooted copy inspects a single link entry without changing its
/// referent.
#[test]
fn native_rooted_copy_does_not_use_single_symlink_referent_permissions() {
    let directory = tempdir().expect("temporary directory should be created");
    let referent = directory.path().join("referent");
    let source = directory.path().join("source-link");
    let target = directory.path().join("target-link");
    fs::write(&referent, b"referent").expect("referent should be written");
    set_readonly(&referent, true);
    assert!(
        symlink_file(Path::new("referent"), &source),
        "file symlink fixture should be available"
    );

    let outcome = LocalFileSystem::rooted(directory.path())
        .expect("Rooted filesystem should open")
        .copy_with_options(Path::new("/source-link"), Path::new("/target-link"), &options(false))
        .expect("Rooted should copy the link entry");

    eprintln!(
        "requested Permissions; reported metadata policy: {:?}",
        outcome.metadata_preservation()
    );
    assert_eq!(1, outcome.stats().files());
    assert_link_observation(&source, &target, &referent);
    set_readonly(&referent, false);
}

/// Verifies Host copy inspects a symlink entry in a mixed tree without changing
/// its referent.
#[test]
fn native_host_copy_does_not_use_mixed_tree_symlink_referent_permissions() {
    let directory = tempdir().expect("temporary directory should be created");
    let source_root = directory.path().join("source");
    let target_root = directory.path().join("target");
    fs::create_dir(&source_root).expect("source directory should be created");
    let source_referent = source_root.join("referent");
    fs::write(&source_referent, b"referent").expect("referent should be written");
    set_readonly(&source_referent, true);
    assert!(
        symlink_file(Path::new("referent"), &source_root.join("link")),
        "file symlink fixture should be available"
    );

    let outcome = LocalFileSystem::host()
        .expect("Host filesystem should open")
        .copy_with_options(&source_root, &target_root, &options(true))
        .expect("Host should copy the mixed tree");

    eprintln!(
        "requested Permissions; reported metadata policy: {:?}",
        outcome.metadata_preservation()
    );
    assert_eq!(2, outcome.stats().files());
    assert_link_observation(
        &source_root.join("link"),
        &target_root.join("link"),
        &target_root.join("referent"),
    );
    set_readonly(&source_referent, false);
    set_readonly(&target_root.join("referent"), false);
}

/// Verifies Rooted copy inspects a symlink entry in a mixed tree without
/// changing its referent.
#[test]
fn native_rooted_copy_does_not_use_mixed_tree_symlink_referent_permissions() {
    let directory = tempdir().expect("temporary directory should be created");
    let source_root = directory.path().join("source");
    let target_root = directory.path().join("target");
    fs::create_dir(&source_root).expect("source directory should be created");
    let source_referent = source_root.join("referent");
    fs::write(&source_referent, b"referent").expect("referent should be written");
    set_readonly(&source_referent, true);
    assert!(
        symlink_file(Path::new("referent"), &source_root.join("link")),
        "file symlink fixture should be available"
    );

    let outcome = LocalFileSystem::rooted(directory.path())
        .expect("Rooted filesystem should open")
        .copy_with_options(Path::new("/source"), Path::new("/target"), &options(true))
        .expect("Rooted should copy the mixed tree");

    eprintln!(
        "requested Permissions; reported metadata policy: {:?}",
        outcome.metadata_preservation()
    );
    assert_eq!(2, outcome.stats().files());
    assert_link_observation(
        &source_root.join("link"),
        &target_root.join("link"),
        &target_root.join("referent"),
    );
    set_readonly(&source_referent, false);
    set_readonly(&target_root.join("referent"), false);
}
