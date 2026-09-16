// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Windows regressions for handle-relative Rooted symbolic-link copying.

#![cfg(windows)]

use std::fs;
use std::io::ErrorKind;
use std::path::Path;

use qubit_local_files::LocalFileSystem;
use tempfile::tempdir;

/// Creates a file link or reports that the host has not granted link creation.
fn create_file_link(target: &Path, link: &Path) -> bool {
    match std::os::windows::fs::symlink_file(target, link) {
        Ok(()) => true,
        Err(error) if error.kind() == ErrorKind::PermissionDenied => false,
        Err(error) => panic!("file-link fixture should be created: {error}"),
    }
}

/// Creates a directory link or reports that link creation is unavailable.
fn create_directory_link(target: &Path, link: &Path) -> bool {
    match std::os::windows::fs::symlink_dir(target, link) {
        Ok(()) => true,
        Err(error) if error.kind() == ErrorKind::PermissionDenied => false,
        Err(error) => panic!("directory-link fixture should be created: {error}"),
    }
}

/// Verifies copying a dangling link still succeeds after the diagnostic root
/// path is renamed, proving the operation remains handle-relative.
#[test]
fn test_rooted_copy_preserves_dangling_link_after_root_rename() {
    let parent = tempdir().expect("temporary parent should be created");
    let original = parent.path().join("original");
    let renamed = parent.path().join("renamed");
    fs::create_dir(&original).expect("root fixture should be created");
    let target = Path::new(r"..\outside\missing");
    if !create_file_link(target, &original.join("source")) {
        return;
    }
    let filesystem = LocalFileSystem::rooted(&original).expect("root authority should open");
    fs::rename(&original, &renamed).expect("diagnostic root should be renamed");

    let _ = filesystem
        .copy(Path::new("source"), Path::new("destination"))
        .expect("dangling link should copy through retained handles");

    assert_eq!(target, fs::read_link(renamed.join("destination")).unwrap());
}

/// Verifies directory-link classification reads the final link attributes and
/// does not require its external target to exist.
#[test]
fn test_rooted_copy_preserves_dangling_directory_link_kind() {
    let root = tempdir().expect("temporary root should be created");
    let target = Path::new(r"C:\outside\missing-directory");
    if !create_directory_link(target, &root.path().join("source")) {
        return;
    }
    let filesystem = LocalFileSystem::rooted(root.path()).expect("root authority should open");

    let _ = filesystem
        .copy(Path::new("source"), Path::new("destination"))
        .expect("dangling directory link should copy without target lookup");

    assert_eq!(target, fs::read_link(root.path().join("destination")).unwrap());
}

/// Verifies copying a link uses the retained root after the diagnostic path is
/// replaced by an unrelated directory tree.
#[test]
fn test_rooted_link_copy_ignores_replacement_diagnostic_root() {
    let parent = tempdir().expect("temporary parent should be created");
    let original = parent.path().join("original");
    let renamed = parent.path().join("renamed");
    fs::create_dir(&original).expect("root fixture should be created");
    let retained_target = Path::new(r"retained\missing");
    if !create_file_link(retained_target, &original.join("source")) {
        return;
    }
    let filesystem = LocalFileSystem::rooted(&original).expect("root authority should open");
    fs::rename(&original, &renamed).expect("opened root should be renamed");
    fs::create_dir(&original).expect("replacement root should be created");
    assert!(
        create_file_link(Path::new(r"replacement\missing"), &original.join("source")),
        "replacement link should be created after link permission was established",
    );

    let _ = filesystem
        .copy(Path::new("source"), Path::new("destination"))
        .expect("link copy should use retained root handles");

    assert_eq!(retained_target, fs::read_link(renamed.join("destination")).unwrap(),);
    assert!(!original.join("destination").exists());
}

/// Verifies a link to an existing external file is copied without opening the
/// target through the Rooted authority.
#[test]
fn test_rooted_copy_preserves_link_to_existing_external_file() {
    let root = tempdir().expect("temporary root should be created");
    let outside = tempdir().expect("external directory should be created");
    let target = outside.path().join("payload");
    fs::write(&target, b"external").expect("external target should be written");
    if !create_file_link(&target, &root.path().join("source")) {
        return;
    }
    let filesystem = LocalFileSystem::rooted(root.path()).expect("root authority should open");

    let _ = filesystem
        .copy(Path::new("source"), Path::new("destination"))
        .expect("external-target link should copy as a link entry");

    assert_eq!(target, fs::read_link(root.path().join("destination")).unwrap());
}
