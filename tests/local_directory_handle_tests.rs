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

#[cfg(windows)]
#[allow(clippy::permissions_set_readonly_false)]
fn clear_readonly_attribute(path: &Path) {
    let mut permissions = fs::metadata(path)
        .expect("read-only payload metadata should be available")
        .permissions();
    permissions.set_readonly(false);
    fs::set_permissions(path, permissions).expect("clear read-only attribute before cleanup");
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

#[cfg(windows)]
#[test]
fn test_readonly_stage_preserves_file_attributes() {
    use std::fs::OpenOptions;
    use std::os::windows::io::AsRawHandle;

    use qubit_local_files::options::LocalTempDirectoryOptions;
    use qubit_local_files::policy::LocalTreeFinalAccess;
    use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_ARCHIVE;
    use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_HIDDEN;
    use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_READONLY;
    use windows_sys::Win32::Storage::FileSystem::FILE_BASIC_INFO;
    use windows_sys::Win32::Storage::FileSystem::FileBasicInfo;

    let fixture = tempdir().expect("fixture directory should exist");
    let parent = LocalFileSystem::host()
        .expect("host filesystem should open")
        .open_directory_with_options(fixture.path(), &LocalDirectoryOpenOptions::new())
        .expect("parent handle should open");
    let mut stage = parent
        .create_staged_directory(&LocalTempDirectoryOptions::new())
        .expect("stage should be created");
    stage
        .write_file_new(Path::new("payload"), &mut &b"attributes"[..], 64)
        .expect("payload should be written");
    let payload = fixture.path().join(stage.relative_name()).join("payload");
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .open(&payload)
        .expect("open payload");
    let mut information = unsafe { std::mem::zeroed::<FILE_BASIC_INFO>() };
    assert_ne!(
        unsafe {
            windows_sys::Win32::Storage::FileSystem::GetFileInformationByHandleEx(
                file.as_raw_handle().cast(),
                FileBasicInfo,
                (&mut information as *mut FILE_BASIC_INFO).cast(),
                std::mem::size_of::<FILE_BASIC_INFO>() as u32,
            )
        },
        0,
        "query payload attributes",
    );
    let retained = FILE_ATTRIBUTE_HIDDEN | FILE_ATTRIBUTE_ARCHIVE;
    information.FileAttributes |= retained;
    assert_ne!(
        unsafe {
            windows_sys::Win32::Storage::FileSystem::SetFileInformationByHandle(
                file.as_raw_handle().cast(),
                FileBasicInfo,
                (&information as *const FILE_BASIC_INFO).cast(),
                std::mem::size_of::<FILE_BASIC_INFO>() as u32,
            )
        },
        0,
        "set payload HIDDEN and ARCHIVE attributes",
    );

    stage
        .apply_access_policy(LocalTreeFinalAccess::ReadOnlyCurrentUser)
        .expect("apply read-only access policy");

    let mut after = unsafe { std::mem::zeroed::<FILE_BASIC_INFO>() };
    assert_ne!(
        unsafe {
            windows_sys::Win32::Storage::FileSystem::GetFileInformationByHandleEx(
                file.as_raw_handle().cast(),
                FileBasicInfo,
                (&mut after as *mut FILE_BASIC_INFO).cast(),
                std::mem::size_of::<FILE_BASIC_INFO>() as u32,
            )
        },
        0,
        "query transitioned payload attributes",
    );
    assert_eq!(after.FileAttributes & retained, retained);
    assert_ne!(after.FileAttributes & FILE_ATTRIBUTE_READONLY, 0);

    drop(file);
    clear_readonly_attribute(&payload);
    stage.cleanup().expect("clean up staged fixture");
}

#[test]
fn test_open_directory_accepts_relative_parent_shape() {
    let fixture = tempdir().expect("fixture directory should exist");
    fs::create_dir(fixture.path().join("child")).expect("child directory should exist");
    fs::create_dir(fixture.path().join("nested")).expect("nested directory should exist");

    let parent = rooted_handle(fixture.path(), LocalComponentPolicy::allow_all());
    let child = parent
        .open_directory(Path::new("nested/../child"), &LocalDirectoryOpenOptions::new())
        .expect("a relative parent component should resolve within the authority");
    assert!(
        child
            .entry_metadata(Path::new("."))
            .expect("child metadata should resolve")
            .is_some()
    );
}

#[cfg(unix)]
#[test]
fn test_open_directory_rejects_absolute_path_shapes_before_resolution() {
    let fixture = tempdir().expect("fixture directory should exist");
    let child = fixture.path().join("child");
    fs::create_dir(&child).expect("real child directory should exist");
    fs::write(child.join("sentinel"), b"unchanged").expect("sentinel should be written");

    let parent = rooted_handle(fixture.path(), LocalComponentPolicy::allow_all());
    for input in [Path::new("/"), Path::new("/child")] {
        let error = parent
            .open_directory(input, &LocalDirectoryOpenOptions::new())
            .expect_err("rooted and absolute paths must be rejected");
        assert_eq!(LocalFileErrorKind::InvalidPath, error.kind());
        assert_eq!(
            qubit_local_files::error::LocalFileOperation::BindPath,
            error.operation()
        );
        assert_eq!(Some(input), error.path());
    }
    assert_eq!(
        b"unchanged",
        fs::read(child.join("sentinel"))
            .expect("sentinel should remain")
            .as_slice()
    );
}

#[cfg(windows)]
#[test]
fn test_open_directory_rejects_windows_absolute_and_prefix_shapes_before_resolution() {
    let fixture = tempdir().expect("fixture directory should exist");
    let child = fixture.path().join("child");
    fs::create_dir(&child).expect("real child directory should exist");
    fs::write(child.join("sentinel"), b"unchanged").expect("sentinel should be written");

    let parent = rooted_handle(fixture.path(), LocalComponentPolicy::allow_all());
    for input in [
        Path::new(r"\child"),
        Path::new(r"C:\child"),
        Path::new(r"C:child"),
        Path::new(r"\\server\share\child"),
    ] {
        let error = parent
            .open_directory(input, &LocalDirectoryOpenOptions::new())
            .expect_err("rooted, absolute, and prefixed paths must be rejected");
        assert_eq!(LocalFileErrorKind::InvalidPath, error.kind());
        assert_eq!(
            qubit_local_files::error::LocalFileOperation::BindPath,
            error.operation()
        );
        assert_eq!(Some(input), error.path());
    }
    assert_eq!(
        b"unchanged",
        fs::read(child.join("sentinel"))
            .expect("sentinel should remain")
            .as_slice()
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
