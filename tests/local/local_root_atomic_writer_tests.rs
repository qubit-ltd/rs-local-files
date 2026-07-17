// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

#[cfg(unix)]
use std::io::Write;

#[cfg(unix)]
use qubit_local_files::{
    LocalAtomicWriteStage,
    LocalRelativePath,
    LocalRoot,
};

#[cfg(unix)]
use super::test_support::{
    count_atomic_temp_files,
    fs,
    temp_dir,
};

/// Verifies descriptor-relative atomic replacement and explicit abort cleanup.
#[cfg(unix)]
#[test]
fn test_begin_atomic_write_commits_and_aborts() {
    let root_path = temp_dir("rooted-atomic-lifecycle");
    fs::write(root_path.join("result.txt"), b"old")
        .expect("destination fixture should be written");
    let root = LocalRoot::open(&root_path).expect("root should open");
    let destination = LocalRelativePath::new("result.txt")
        .expect("destination should validate");

    let mut writer = root
        .begin_atomic_write(&destination)
        .expect("rooted atomic writer should begin");
    writer
        .write_all(b"new")
        .expect("rooted atomic writer should write");
    writer.commit().expect("rooted atomic writer should commit");
    assert_eq!(
        b"new",
        fs::read(root_path.join("result.txt"))
            .expect("committed destination should be readable")
            .as_slice(),
    );

    let mut writer = root
        .begin_atomic_write(&destination)
        .expect("second rooted atomic writer should begin");
    writer
        .write_all(b"discarded")
        .expect("aborted data should be staged");
    writer.abort().expect("rooted atomic writer should abort");
    assert_eq!(0, count_atomic_temp_files(&root_path));
    assert_eq!(
        b"new",
        fs::read(root_path.join("result.txt"))
            .expect("aborted destination should remain readable")
            .as_slice(),
    );
    fs::remove_dir_all(root_path).expect("atomic fixture should be removed");
}

/// Verifies best-effort cleanup when an uncommitted rooted writer is dropped.
#[cfg(unix)]
#[test]
fn test_drop_removes_rooted_atomic_staging_file() {
    let root_path = temp_dir("rooted-atomic-drop");
    let root = LocalRoot::open(&root_path).expect("root should open");
    let destination = LocalRelativePath::new("result.txt")
        .expect("destination should validate");

    let mut writer = root
        .begin_atomic_write(&destination)
        .expect("rooted atomic writer should begin");
    writer
        .write_all(b"discarded")
        .expect("discarded bytes should be staged");
    assert_eq!(1, count_atomic_temp_files(&root_path));
    drop(writer);

    assert_eq!(0, count_atomic_temp_files(&root_path));
    assert!(!root_path.join("result.txt").exists());
    fs::remove_dir_all(root_path).expect("atomic fixture should be removed");
}

/// Verifies that rooted atomic creation uses anchored parent traversal and
/// remains valid when the diagnostic root path is renamed.
#[cfg(unix)]
#[test]
fn test_commit_survives_root_rename_and_creates_parents() {
    let root_path = temp_dir("rooted-atomic-rename");
    let root = LocalRoot::open(&root_path).expect("root should open");
    let destination = LocalRelativePath::new("nested/result.txt")
        .expect("destination should validate");
    let mut writer = root
        .begin_atomic_write(&destination)
        .expect("rooted atomic writer should create its parent");
    writer
        .write_all(b"anchored")
        .expect("rooted atomic writer should write");
    let moved_path = root_path.with_extension("moved");
    fs::rename(&root_path, &moved_path).expect("root should be renamed");
    fs::create_dir(&root_path)
        .expect("replacement diagnostic root should exist");

    writer
        .commit()
        .expect("commit should use anchored parent descriptors");

    assert_eq!(
        b"anchored",
        fs::read(moved_path.join("nested/result.txt"))
            .expect("anchored destination should be readable")
            .as_slice(),
    );
    assert!(!root_path.join("nested/result.txt").exists());
    fs::remove_dir_all(root_path).expect("replacement root should be removed");
    fs::remove_dir_all(moved_path).expect("moved root should be removed");
}

/// Verifies permission preservation and final symbolic-link denial.
#[cfg(unix)]
#[test]
fn test_begin_atomic_write_preserves_permissions_and_rejects_symlink() {
    use std::os::unix::fs::{
        PermissionsExt,
        symlink,
    };

    let fixture = temp_dir("rooted-atomic-permissions");
    let root_path = fixture.join("root");
    fs::create_dir(&root_path).expect("root should be created");
    let destination_path = root_path.join("result.txt");
    fs::write(&destination_path, b"old")
        .expect("destination fixture should be written");
    fs::set_permissions(&destination_path, fs::Permissions::from_mode(0o640))
        .expect("destination permissions should be set");
    let root = LocalRoot::open(&root_path).expect("root should open");
    let destination = LocalRelativePath::new("result.txt")
        .expect("destination should validate");
    let mut writer = root
        .begin_atomic_write(&destination)
        .expect("rooted atomic writer should begin");
    writer
        .write_all(b"new")
        .expect("replacement should be staged");
    writer.commit().expect("replacement should commit");
    assert_eq!(
        0o640,
        fs::metadata(&destination_path)
            .expect("destination metadata should be readable")
            .permissions()
            .mode()
            & 0o777,
    );

    let outside_path = fixture.join("outside.txt");
    fs::write(&outside_path, b"outside")
        .expect("outside fixture should be written");
    let linked_path = root_path.join("linked.txt");
    symlink(&outside_path, &linked_path)
        .expect("final symlink should be created");
    let linked = LocalRelativePath::new("linked.txt")
        .expect("linked destination should validate lexically");
    let error = root
        .begin_atomic_write(&linked)
        .expect_err("final symlink should be rejected");

    assert_eq!(LocalAtomicWriteStage::InspectDestination, error.stage());
    assert_eq!(b"outside", fs::read(outside_path).unwrap().as_slice());
    fs::remove_dir_all(fixture).expect("permission fixture should be removed");
}

/// Verifies that a final entry replaced by a symbolic link after staging is
/// rejected without modifying the link target.
#[cfg(unix)]
#[test]
fn test_commit_rejects_final_symlink_replacement() {
    use std::os::unix::fs::symlink;

    let fixture = temp_dir("rooted-atomic-final-replacement");
    let root_path = fixture.join("root");
    fs::create_dir(&root_path).expect("root should be created");
    let destination_path = root_path.join("result.txt");
    fs::write(&destination_path, b"old")
        .expect("destination fixture should be written");
    let outside_path = fixture.join("outside.txt");
    fs::write(&outside_path, b"outside")
        .expect("outside fixture should be written");
    let root = LocalRoot::open(&root_path).expect("root should open");
    let destination = LocalRelativePath::new("result.txt")
        .expect("destination should validate");
    let mut writer = root
        .begin_atomic_write(&destination)
        .expect("rooted atomic writer should begin");
    writer
        .write_all(b"replacement")
        .expect("replacement should be staged");
    let displaced_path = root_path.join("displaced.txt");
    fs::rename(&destination_path, &displaced_path)
        .expect("destination should be displaced");
    symlink(&outside_path, &destination_path)
        .expect("destination symlink should be installed");

    let error = writer
        .commit()
        .expect_err("commit should reject the replacement symlink");

    assert_eq!(LocalAtomicWriteStage::ReplaceDestination, error.stage());
    assert_eq!(b"outside", fs::read(&outside_path).unwrap().as_slice());
    assert!(destination_path.is_symlink());
    assert_eq!(0, count_atomic_temp_files(&root_path));
    fs::remove_dir_all(fixture).expect("replacement fixture should be removed");
}

/// Verifies explicit flushing and creation of a previously missing atomic
/// destination.
#[cfg(unix)]
#[test]
fn test_commit_flushes_and_creates_missing_destination() {
    let root_path = temp_dir("rooted-atomic-new");
    let root = LocalRoot::open(&root_path).expect("root should open");
    let destination = LocalRelativePath::new("result.txt")
        .expect("destination should validate");
    let mut writer = root
        .begin_atomic_write(&destination)
        .expect("rooted atomic writer should begin");

    writer.write_all(b"new").expect("new data should be staged");
    writer.flush().expect("staging file should flush");
    writer.commit().expect("new destination should commit");

    assert_eq!(
        b"new",
        fs::read(root_path.join("result.txt")).unwrap().as_slice(),
    );
    fs::remove_dir_all(root_path)
        .expect("new atomic fixture should be removed");
}

/// Verifies structured preparation errors for an ordinary-file parent and a
/// directory destination.
#[cfg(unix)]
#[test]
fn test_begin_atomic_write_reports_parent_and_destination_type_errors() {
    let root_path = temp_dir("rooted-atomic-types");
    fs::write(root_path.join("parent-file"), b"file")
        .expect("parent file fixture should be written");
    fs::create_dir(root_path.join("destination-dir"))
        .expect("destination directory fixture should be created");
    let root = LocalRoot::open(&root_path).expect("root should open");

    let invalid_parent = LocalRelativePath::new("parent-file/result.txt")
        .expect("invalid parent should validate lexically");
    let error = root
        .begin_atomic_write(&invalid_parent)
        .expect_err("ordinary-file parent should fail");
    assert_eq!(LocalAtomicWriteStage::PrepareParent, error.stage());

    let directory = LocalRelativePath::new("destination-dir")
        .expect("directory destination should validate lexically");
    let error = root
        .begin_atomic_write(&directory)
        .expect_err("directory destination should fail");
    assert_eq!(LocalAtomicWriteStage::InspectDestination, error.stage());

    fs::remove_dir_all(root_path)
        .expect("atomic-type fixture should be removed");
}

/// Verifies that explicit abort reports a staging entry removed behind the
/// writer instead of silently claiming successful cleanup.
#[cfg(unix)]
#[test]
fn test_abort_reports_missing_staging_entry() {
    let root_path = temp_dir("rooted-atomic-abort-error");
    let root = LocalRoot::open(&root_path).expect("root should open");
    let destination = LocalRelativePath::new("result.txt")
        .expect("destination should validate");
    let writer = root
        .begin_atomic_write(&destination)
        .expect("rooted atomic writer should begin");
    let temporary_path = fs::read_dir(&root_path)
        .expect("root directory should be readable")
        .map(|entry| entry.expect("root entry should be readable").path())
        .find(|path| {
            path.file_name().and_then(|name| name.to_str()).is_some_and(
                |name| {
                    name.starts_with(".atomic-write-") && name.ends_with(".tmp")
                },
            )
        })
        .expect("staging entry should exist");
    fs::remove_file(temporary_path)
        .expect("staging entry should be removed behind the writer");

    let error = writer
        .abort()
        .expect_err("abort should report the missing staging entry");

    assert_eq!(LocalAtomicWriteStage::CleanupTemporaryFile, error.stage());
    assert_eq!(std::io::ErrorKind::NotFound, error.kind());
    fs::remove_dir_all(root_path)
        .expect("abort-error fixture should be removed");
}
