// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::fs;
use std::path::Path;

use qubit_io::Output;
use qubit_local_files::LocalFileSystem;
use qubit_local_files::directory::LocalComponentPolicy;
use qubit_local_files::directory::LocalDirectoryHandle;
use qubit_local_files::error::LocalFileErrorKind;
use qubit_local_files::options::LocalCreateDirectoryOptions;
use qubit_local_files::options::LocalDirectoryOpenOptions;
use qubit_local_files::options::LocalWriteMode;
use qubit_local_files::options::LocalWriteOptions;
use tempfile::TempDir;

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

fn symlink_dir(target: &Path, link: &Path) -> bool {
    #[cfg(unix)]
    {
        std::os::unix::fs::symlink(target, link).expect("directory symlink fixture should be created");
        true
    }
    #[cfg(windows)]
    {
        support::windows_symlink::create_directory_link(target, link)
    }
}

fn handles(root: &Path) -> (LocalDirectoryHandle, LocalDirectoryHandle) {
    let options = LocalDirectoryOpenOptions::new();
    let host = LocalFileSystem::host()
        .expect("host filesystem should open")
        .open_directory_with_options(root, &options)
        .expect("host directory authority should open");
    let rooted = LocalFileSystem::rooted(root)
        .expect("rooted filesystem should open")
        .open_directory_with_options(Path::new("/"), &options)
        .expect("rooted directory authority should open");
    (host, rooted)
}

fn denied_link_handles(root: &Path) -> Vec<LocalDirectoryHandle> {
    let options = LocalDirectoryOpenOptions::new()
        .with_component_policy(LocalComponentPolicy::deny_names(vec!["link".into()]).expect("policy should be valid"));
    let mut handles = Vec::new();
    #[cfg(not(windows))]
    handles.push(
        LocalFileSystem::host()
            .expect("host filesystem should open")
            .open_directory_with_options(root, &options)
            .expect("host directory authority should open"),
    );
    let rooted = LocalFileSystem::rooted(root)
        .expect("rooted filesystem should open")
        .open_directory_with_options(Path::new("/"), &options)
        .expect("rooted directory authority should open");
    handles.push(rooted);
    handles
}

fn assert_create_new_rejects(handle: &LocalDirectoryHandle, root: &Path) -> LocalFileErrorKind {
    let error = handle
        .open_writer_with_options(Path::new("link"), &LocalWriteOptions::new(LocalWriteMode::CreateNew))
        .expect_err("CreateNew must reject an existing symlink entry");
    assert!(
        fs::symlink_metadata(root.join("link"))
            .expect("link should remain")
            .file_type()
            .is_symlink()
    );
    assert!(
        fs::symlink_metadata(root.join("missing")).is_err(),
        "dangling referent must remain absent"
    );
    error.kind()
}

#[test]
fn test_create_new_keeps_dangling_link_and_missing_referent() {
    let fixture = TempDir::new().expect("fixture directory should exist");
    let root = fixture.path();
    if !symlink_file(Path::new("missing"), &root.join("link")) {
        return;
    }
    let (host, rooted) = handles(root);
    let host_kind = assert_create_new_rejects(&host, root);
    let rooted_kind = assert_create_new_rejects(&rooted, root);

    let direct = LocalFileSystem::host().expect("host filesystem should open");
    let direct_error = direct
        .open_writer_with_options(&root.join("link"), &LocalWriteOptions::new(LocalWriteMode::CreateNew))
        .expect_err("filesystem CreateNew must reject the existing link");
    assert_eq!(host_kind, direct_error.kind());
    assert_eq!(rooted_kind, direct_error.kind());
}

#[test]
fn test_mkdir_keeps_final_directory_link_entry() {
    let fixture = TempDir::new().expect("fixture directory should exist");
    let root = fixture.path();
    fs::create_dir(root.join("target")).expect("target directory should exist");
    if !symlink_dir(Path::new("target"), &root.join("link")) {
        return;
    }
    let (host, rooted) = handles(root);
    let options = LocalCreateDirectoryOptions::new();
    let host_direct_error = LocalFileSystem::host()
        .expect("host filesystem should open")
        .create_directory_with_options(&root.join("link"), &options)
        .expect_err("filesystem mkdir must reject a final directory link");
    let rooted_direct_error = LocalFileSystem::rooted(root)
        .expect("rooted filesystem should open")
        .create_directory_with_options(Path::new("/link"), &options)
        .expect_err("rooted filesystem mkdir must reject a final directory link");
    for (handle, direct_kind) in [(&host, host_direct_error.kind()), (&rooted, rooted_direct_error.kind())] {
        let error = handle
            .create_directory_with_options(Path::new("link"), &options)
            .expect_err("mkdir must reject a final directory link entry");
        assert_eq!(direct_kind, error.kind());
        assert!(
            fs::symlink_metadata(root.join("link"))
                .expect("link remains")
                .file_type()
                .is_symlink()
        );
        assert!(root.join("target").is_dir());
    }
}

#[test]
fn test_mkdir_rejects_dangling_final_directory_link() {
    let fixture = TempDir::new().expect("fixture directory should exist");
    let root = fixture.path();
    if !symlink_dir(Path::new("missing"), &root.join("link")) {
        return;
    }
    let (host, rooted) = handles(root);
    for handle in [&host, &rooted] {
        assert!(
            handle
                .create_directory_with_options(Path::new("link"), &LocalCreateDirectoryOptions::new())
                .is_err(),
            "mkdir must reject a dangling final directory link",
        );
        assert!(
            fs::symlink_metadata(root.join("link"))
                .expect("dangling link should remain")
                .file_type()
                .is_symlink(),
        );
        assert!(
            fs::symlink_metadata(root.join("missing")).is_err(),
            "dangling link referent must remain absent",
        );
    }
}

#[test]
fn test_existing_file_link_rejects_create_new_without_changing_referent() {
    let fixture = TempDir::new().expect("fixture directory should exist");
    let root = fixture.path();
    fs::write(root.join("target"), b"original").expect("target should be written");
    if !symlink_file(Path::new("target"), &root.join("link")) {
        return;
    }
    let (host, rooted) = handles(root);
    assert_eq!(
        LocalFileErrorKind::AlreadyExists,
        assert_create_new_rejects_existing(&host, root)
    );
    assert_eq!(
        LocalFileErrorKind::AlreadyExists,
        assert_create_new_rejects_existing(&rooted, root)
    );
    assert_eq!(
        b"original",
        fs::read(root.join("target"))
            .expect("referent should remain")
            .as_slice()
    );
}

fn assert_create_new_rejects_existing(handle: &LocalDirectoryHandle, root: &Path) -> LocalFileErrorKind {
    let error = handle
        .open_writer_with_options(Path::new("link"), &LocalWriteOptions::new(LocalWriteMode::CreateNew))
        .expect_err("CreateNew must reject an existing link");
    assert!(
        fs::symlink_metadata(root.join("link"))
            .expect("link should remain")
            .file_type()
            .is_symlink()
    );
    error.kind()
}

#[test]
fn test_following_write_modes_keep_existing_contract() {
    let fixture = TempDir::new().expect("fixture directory should exist");
    let root = fixture.path();
    fs::write(root.join("target"), b"old").expect("target should be written");
    if !symlink_file(Path::new("target"), &root.join("link")) {
        return;
    }
    let (host, rooted) = handles(root);
    for handle in [&host, &rooted] {
        let mut writer = handle
            .open_writer_with_options(
                Path::new("link"),
                &LocalWriteOptions::new(LocalWriteMode::CreateOrReplace),
            )
            .expect("CreateOrReplace should continue following the final link");
        Output::write_fully(&mut writer, b"new").expect("replacement should write");
        let _outcome = writer.commit().expect("replacement should commit");
        assert_eq!(
            b"new",
            fs::read(root.join("target"))
                .expect("referent should change")
                .as_slice()
        );

        let mut writer = handle
            .open_writer_with_options(Path::new("link"), &LocalWriteOptions::new(LocalWriteMode::Append))
            .expect("Append should continue following the final link");
        Output::write_fully(&mut writer, b"+").expect("append should write");
        let _outcome = writer.commit().expect("append should commit");
        assert_eq!(
            b"new+",
            fs::read(root.join("target"))
                .expect("referent should append")
                .as_slice()
        );
    }
}

#[test]
fn test_intermediate_directory_link_obeys_policy() {
    let fixture = TempDir::new().expect("fixture directory should exist");
    let root = fixture.path();
    fs::create_dir(root.join("target")).expect("target directory should exist");
    fs::write(root.join("target/file"), b"old").expect("target file should exist");
    if !symlink_dir(Path::new("target"), &root.join("jump")) {
        return;
    }
    let options = LocalDirectoryOpenOptions::new();
    let host = LocalFileSystem::host()
        .expect("host filesystem should open")
        .open_directory_with_options(root, &options)
        .expect("host authority should open");
    let rooted = LocalFileSystem::rooted(root)
        .expect("rooted filesystem should open")
        .open_directory_with_options(Path::new("/"), &options)
        .expect("rooted authority should open");
    for handle in [&host, &rooted] {
        let mut writer = handle
            .open_writer_with_options(
                Path::new("jump/file"),
                &LocalWriteOptions::new(LocalWriteMode::CreateOrReplace),
            )
            .expect("an intermediate directory link should continue to resolve");
        Output::write_fully(&mut writer, b"new").expect("write should succeed");
        let _outcome = writer.commit().expect("writer should commit");
        assert_eq!(
            b"new",
            fs::read(root.join("target/file"))
                .expect("referent should change")
                .as_slice()
        );
    }

    let denied = LocalDirectoryOpenOptions::new()
        .with_component_policy(LocalComponentPolicy::deny_names(vec!["jump".into()]).expect("policy should be valid"));
    #[cfg(not(windows))]
    let denied_handles = [LocalFileSystem::host()
        .expect("host filesystem should open")
        .open_directory_with_options(root, &denied)
        .expect("host authority should open")];
    #[cfg(windows)]
    let denied_handles = [LocalFileSystem::rooted(root)
        .expect("rooted filesystem should open")
        .open_directory_with_options(Path::new("/"), &denied)
        .expect("rooted authority should open")];
    for denied_handle in &denied_handles {
        let error = denied_handle
            .open_writer_with_options(
                Path::new("jump/file"),
                &LocalWriteOptions::new(LocalWriteMode::CreateNew),
            )
            .expect_err("denied intermediate component must remain denied");
        assert_eq!(LocalFileErrorKind::InvalidPath, error.kind());
        assert_eq!(
            b"new",
            fs::read(root.join("target/file"))
                .expect("denied operation must not alter target")
                .as_slice()
        );
    }
}

#[test]
fn test_denied_final_component_cannot_be_bypassed_by_create_new() {
    let fixture = TempDir::new().expect("fixture directory should exist");
    let root = fixture.path();
    if !symlink_file(Path::new("missing"), &root.join("link")) {
        return;
    }
    for handle in denied_link_handles(root) {
        let error = handle
            .open_writer_with_options(Path::new("link"), &LocalWriteOptions::new(LocalWriteMode::CreateNew))
            .expect_err("denied name must remain rejected with CreateNew");
        assert_eq!(LocalFileErrorKind::InvalidPath, error.kind());
        assert!(
            fs::symlink_metadata(root.join("link"))
                .expect("link should remain")
                .file_type()
                .is_symlink()
        );
        assert!(fs::symlink_metadata(root.join("missing")).is_err());
    }
}
