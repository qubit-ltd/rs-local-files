// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Explicit persistence bases are independent of temporary creation spelling.

use std::fs;
use std::io::Write;
use std::path::Path;

use qubit_local_files::LocalFileSystem;
use qubit_local_files::options::LocalPersistOptions;
use qubit_local_files::options::LocalTempDirectoryOptions;
use qubit_local_files::options::LocalTempFileOptions;

/// A base link resolving to virtual root remains inside the creating authority.
#[cfg(unix)]
#[test]
fn test_persist_at_accepts_rooted_base_link_to_virtual_root() {
    let dir = tempfile::tempdir().expect("isolated fixture");
    std::os::unix::fs::symlink("/", dir.path().join("root-link")).expect("virtual root link");
    let filesystem = LocalFileSystem::rooted(dir.path()).expect("rooted filesystem");
    let mut file = filesystem.create_temp_file().expect("temporary file");
    file.write_all(b"root-bound").expect("temporary contents");
    let _ = file
        .persist_at(
            Path::new("/root-link"),
            Path::new("published-file"),
            LocalPersistOptions::new(),
        )
        .expect("publish through virtual root link");
    assert_eq!(
        fs::read(dir.path().join("published-file")).expect("published contents"),
        b"root-bound"
    );
    let directory = filesystem.create_temp_directory().expect("temporary directory");
    let _ = directory
        .persist_at(
            Path::new("/root-link"),
            Path::new("published-directory"),
            LocalPersistOptions::new(),
        )
        .expect("publish directory through virtual root link");
    assert!(dir.path().join("published-directory").is_dir());
}

/// Absolute creation parents support explicit relative persistence in both
/// scopes.
#[test]
fn test_persist_at_uses_explicit_base_for_both_resources() {
    for rooted in [false, true] {
        let dir = tempfile::tempdir().expect("isolated fixture");
        fs::create_dir(dir.path().join("dest")).expect("destination base");
        let filesystem = if rooted {
            LocalFileSystem::rooted(dir.path())
        } else {
            LocalFileSystem::host()
        }
        .expect("filesystem");
        let creation = if rooted { Path::new("/") } else { dir.path() };
        let physical_base = dir.path().join("dest");
        let base = if rooted {
            Path::new("/dest")
        } else {
            physical_base.as_path()
        };
        let mut temporary = filesystem
            .create_temp_file_with_options(&LocalTempFileOptions::new().with_parent(creation))
            .expect("temporary file");
        temporary.write_all(b"payload").expect("temporary content");
        let _ = temporary
            .persist_at(base, Path::new("file"), LocalPersistOptions::new())
            .expect("explicit file persistence");
        assert_eq!(
            fs::read(physical_base.join("file")).expect("published file"),
            b"payload"
        );
        let temporary = filesystem
            .create_temp_directory_with_options(&LocalTempDirectoryOptions::new().with_parent(creation))
            .expect("temporary directory");
        let _ = temporary
            .persist_at(base, Path::new("directory"), LocalPersistOptions::new())
            .expect("explicit directory persistence");
        assert!(physical_base.join("directory").is_dir());
    }
}

/// Invalid target parameters return a file guard that remains open and
/// writable.
#[test]
fn test_invalid_target_keeps_original_file_open() {
    let dir = tempfile::tempdir().expect("isolated fixture");
    let filesystem = LocalFileSystem::host().expect("filesystem");
    let temporary = filesystem
        .create_temp_file_with_options(&LocalTempFileOptions::new().with_parent(dir.path()))
        .expect("temporary file");
    let mut error = temporary
        .persist_at(
            Path::new("relative-base"),
            Path::new("result"),
            LocalPersistOptions::new(),
        )
        .expect_err("relative base must fail");
    error
        .resource_mut()
        .write_all(b"still-open")
        .expect("invalid parameters must not close source");
    error.resource_mut().cleanup().expect("retained cleanup");
}

/// The absolute-only entry point rejects relative targets regardless of scope.
#[test]
fn test_relative_persist_is_always_rejected_before_close() {
    for rooted in [false, true] {
        let dir = tempfile::tempdir().expect("isolated fixture");
        let filesystem = if rooted {
            LocalFileSystem::rooted(dir.path())
        } else {
            LocalFileSystem::host()
        }
        .expect("filesystem");
        let creation = if rooted { Path::new("/") } else { dir.path() };
        let temporary = filesystem
            .create_temp_file_with_options(&LocalTempFileOptions::new().with_parent(creation))
            .expect("temporary file");
        let mut error = temporary
            .persist(Path::new("relative"))
            .expect_err("relative persist must fail consistently");
        error
            .resource_mut()
            .write_all(b"still-open")
            .expect("relative target must not close source");
        error.resource_mut().cleanup().expect("retained cleanup");
    }
}

/// Every invalid base or target fails before source closing in either
/// namespace.
#[test]
fn test_persist_at_rejects_invalid_parameters_without_consuming_resources() {
    use qubit_local_files::outcome::LocalPersistStage;
    for rooted in [false, true] {
        let dir = tempfile::tempdir().expect("isolated fixture");
        fs::write(dir.path().join("file-base"), b"not a directory").expect("file base");
        fs::create_dir(dir.path().join("dest")).expect("destination base");
        let filesystem = if rooted {
            LocalFileSystem::rooted(dir.path())
        } else {
            LocalFileSystem::host()
        }
        .expect("filesystem");
        let root = if rooted { Path::new("/") } else { dir.path() };
        let cases = [
            (
                Path::new("relative-base").to_path_buf(),
                Path::new("target").to_path_buf(),
            ),
            (root.join("file-base"), Path::new("target").to_path_buf()),
            (root.join("missing-base"), Path::new("target").to_path_buf()),
            (root.join("./dest"), Path::new("target").to_path_buf()),
            (root.join("dest/.."), Path::new("target").to_path_buf()),
            (root.to_path_buf(), root.join("absolute-target")),
            (root.to_path_buf(), Path::new("").to_path_buf()),
            (root.to_path_buf(), Path::new("invalid\0target").to_path_buf()),
        ];
        for (base, target) in cases {
            let temporary = filesystem
                .create_temp_file_with_options(&LocalTempFileOptions::new().with_parent(root))
                .expect("temporary file");
            let mut error = temporary
                .persist_at(&base, &target, LocalPersistOptions::new())
                .expect_err("invalid file persistence operands");
            assert_eq!(error.stage(), LocalPersistStage::ResolveTarget);
            assert_eq!(error.requested_target(), target);
            assert_eq!(error.error().current_directory(), Some(base.as_path()));
            error
                .resource_mut()
                .write_all(b"still-open")
                .expect("invalid parameter leaves handle open");
            error.resource_mut().cleanup().expect("file cleanup");
            let temporary = filesystem
                .create_temp_directory_with_options(&LocalTempDirectoryOptions::new().with_parent(root))
                .expect("temporary directory");
            let mut error = temporary
                .persist_at(&base, &target, LocalPersistOptions::new())
                .expect_err("invalid directory persistence operands");
            assert_eq!(error.stage(), LocalPersistStage::ResolveTarget);
            error.resource_mut().cleanup().expect("directory cleanup");
        }
    }
}

/// Rooted persistence keeps the original opened root after its physical rename.
#[cfg(unix)]
#[test]
fn test_persist_at_retains_root_authority_after_rename() {
    let dir = tempfile::tempdir().expect("isolated fixture");
    let original = dir.path().join("original");
    let moved = dir.path().join("moved");
    fs::create_dir_all(original.join("dest")).expect("destination base");
    let filesystem = LocalFileSystem::rooted(&original).expect("root authority");
    let mut file = filesystem.create_temp_file().expect("temporary file");
    file.write_all(b"payload").expect("source content");
    let directory = filesystem.create_temp_directory().expect("temporary directory");
    fs::rename(&original, &moved).expect("rename opened root");
    fs::create_dir_all(original.join("dest")).expect("unrelated replacement root");
    let _ = file
        .persist_at(Path::new("/dest"), Path::new("file"), LocalPersistOptions::new())
        .expect("retained file authority");
    let _ = directory
        .persist_at(Path::new("/dest"), Path::new("directory"), LocalPersistOptions::new())
        .expect("retained directory authority");
    assert_eq!(
        fs::read(moved.join("dest/file")).expect("correct root content"),
        b"payload"
    );
    assert!(moved.join("dest/directory").is_dir());
    assert_eq!(
        fs::read_dir(original.join("dest"))
            .expect("replacement root entries")
            .count(),
        0
    );
}

/// Host persistence resolves a linked explicit base before a relative parent.
#[cfg(unix)]
#[test]
fn test_persist_at_host_base_uses_native_parent_order() {
    use std::os::unix::fs::symlink;
    let dir = tempfile::tempdir().expect("isolated fixture");
    fs::create_dir(dir.path().join("a")).expect("a directory");
    fs::create_dir_all(dir.path().join("b/inner")).expect("b directory");
    symlink("../b/inner", dir.path().join("a/link")).expect("linked base");
    let filesystem = LocalFileSystem::host().expect("host filesystem");
    let mut temporary = filesystem
        .create_temp_file_with_options(&LocalTempFileOptions::new().with_parent(dir.path()))
        .expect("temporary file");
    temporary.write_all(b"payload").expect("source content");
    let _ = temporary
        .persist_at(
            &dir.path().join("a/link"),
            Path::new("../published"),
            LocalPersistOptions::new(),
        )
        .expect("native base parent traversal");
    assert_eq!(
        fs::read(dir.path().join("b/published")).expect("native target"),
        b"payload"
    );
    assert!(!dir.path().join("a/published").exists());
}

/// Explicit Host bases remain fixed after process PWD changes in an isolated
/// child.
#[test]
fn test_persist_at_ignores_process_pwd_changes() {
    const CHILD: &str = "RS_LOCAL_FILES_PERSIST_BASE_CHILD";
    if std::env::var_os(CHILD).is_none() {
        let status = std::process::Command::new(std::env::current_exe().expect("test executable"))
            .args(["--exact", "test_persist_at_ignores_process_pwd_changes", "--nocapture"])
            .env(CHILD, "1")
            .status()
            .expect("PWD-isolated child");
        assert!(status.success());
        return;
    }
    let dir = tempfile::tempdir().expect("isolated fixture");
    let original_pwd = std::env::current_dir().expect("original PWD");
    let first = dir.path().join("first");
    let second = dir.path().join("second");
    fs::create_dir(&first).expect("first directory");
    fs::create_dir(&second).expect("second directory");
    std::env::set_current_dir(&first).expect("select creation PWD");
    let filesystem = LocalFileSystem::host().expect("host filesystem");
    let mut file = filesystem.create_temp_file().expect("PWD-relative temporary file");
    let directory = filesystem
        .create_temp_directory()
        .expect("PWD-relative temporary directory");
    file.write_all(b"payload").expect("temporary content");
    std::env::set_current_dir(&second).expect("change process PWD");
    let file_result = file.persist_at(&first, Path::new("file"), LocalPersistOptions::new());
    let directory_result = directory.persist_at(&first, Path::new("directory"), LocalPersistOptions::new());
    std::env::set_current_dir(original_pwd).expect("restore child PWD before cleanup");
    let _ = file_result.expect("fixed file base");
    let _ = directory_result.expect("fixed directory base");
    assert_eq!(
        fs::read(first.join("file")).expect("first directory content"),
        b"payload"
    );
    assert!(first.join("directory").is_dir());
    assert_eq!(fs::read_dir(second).expect("second directory entries").count(), 0);
}

/// Explicit relative Rooted targets cannot walk above their retained authority.
#[test]
fn test_persist_at_rejects_rooted_escape() {
    use qubit_local_files::outcome::LocalPersistStage;
    let dir = tempfile::tempdir().expect("isolated fixture");
    fs::create_dir(dir.path().join("dest")).expect("destination base");
    let filesystem = LocalFileSystem::rooted(dir.path()).expect("root authority");
    let temporary = filesystem.create_temp_file().expect("temporary file");
    let mut error = temporary
        .persist_at(
            Path::new("/dest"),
            Path::new("../../outside"),
            LocalPersistOptions::new(),
        )
        .expect_err("root escape must fail");
    assert_eq!(error.stage(), LocalPersistStage::ResolveTarget);
    assert!(error.resolved_target().is_none());
    error
        .resource_mut()
        .write_all(b"still-open")
        .expect("escape rejected before close");
    error.resource_mut().cleanup().expect("file cleanup");
    let temporary = filesystem.create_temp_directory().expect("temporary directory");
    let mut error = temporary
        .persist_at(
            Path::new("/dest"),
            Path::new("../../outside"),
            LocalPersistOptions::new(),
        )
        .expect_err("directory root escape must fail");
    assert_eq!(error.stage(), LocalPersistStage::ResolveTarget);
    error.resource_mut().cleanup().expect("directory cleanup");
}
