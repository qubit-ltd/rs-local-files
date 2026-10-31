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
use std::os::windows::ffi::OsStrExt;
use std::os::windows::fs::OpenOptionsExt;
use std::os::windows::io::AsRawHandle;
use std::path::Path;
use std::ptr::null_mut;

use qubit_io::Input;
use qubit_local_files::LocalFileSystem;
use tempfile::tempdir;
use windows_sys::Win32::Storage::FileSystem::FILE_FLAG_BACKUP_SEMANTICS;
use windows_sys::Win32::Storage::FileSystem::FILE_FLAG_OPEN_REPARSE_POINT;
use windows_sys::Win32::System::IO::DeviceIoControl;
use windows_sys::Win32::System::Ioctl::FSCTL_SET_REPARSE_POINT;

mod support;
use support::windows_symlink::create_directory_link;
use support::windows_symlink::create_file_link;

/// Preflights both native link kinds before the Windows contract tests run.
#[test]
fn test_windows_symlink_fixture_preflight() {
    let root = tempdir().expect("preflight root should be created");
    let file_target = root.path().join("file-target");
    let directory_target = root.path().join("directory-target");
    fs::write(&file_target, b"target").expect("file target should be written");
    fs::create_dir(&directory_target).expect("directory target should be created");
    let _fixtures_available = create_file_link(&file_target, &root.path().join("file-link"))
        && create_directory_link(&directory_target, &root.path().join("directory-link"));
}

/// Replaces a file link's record with distinct actual and display names.
fn set_distinct_reparse_names(link: &Path, actual: &Path, display: &Path) {
    let substitute: Vec<u16> = actual.as_os_str().encode_wide().collect();
    let print: Vec<u16> = display.as_os_str().encode_wide().collect();
    let substitute_bytes = (substitute.len() * 2) as u16;
    let print_bytes = (print.len() * 2) as u16;
    let data_length = 12 + usize::from(substitute_bytes) + usize::from(print_bytes);
    let mut buffer = vec![0_u8; 20 + usize::from(substitute_bytes) + usize::from(print_bytes)];
    buffer[0..4].copy_from_slice(&0xA000_000Cu32.to_le_bytes());
    buffer[4..6].copy_from_slice(&(data_length as u16).to_le_bytes());
    buffer[8..10].copy_from_slice(&0_u16.to_le_bytes());
    buffer[10..12].copy_from_slice(&substitute_bytes.to_le_bytes());
    buffer[12..14].copy_from_slice(&substitute_bytes.to_le_bytes());
    buffer[14..16].copy_from_slice(&print_bytes.to_le_bytes());
    buffer[16..20].copy_from_slice(&1_u32.to_le_bytes());
    for (index, unit) in substitute.iter().chain(print.iter()).enumerate() {
        let start = 20 + index * 2;
        buffer[start..start + 2].copy_from_slice(&unit.to_le_bytes());
    }
    let file = fs::OpenOptions::new()
        .read(true)
        .write(true)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS)
        .open(link)
        .expect("symbolic-link reparse point should open without following");
    let mut returned = 0_u32;
    // SAFETY: file owns the link handle and buffer remains valid for the call.
    let result = unsafe {
        DeviceIoControl(
            file.as_raw_handle(),
            FSCTL_SET_REPARSE_POINT,
            buffer.as_ptr().cast(),
            buffer.len() as u32,
            null_mut(),
            0,
            &raw mut returned,
            null_mut(),
        )
    };
    assert_ne!(
        0,
        result,
        "reparse record should be replaced: {}",
        std::io::Error::last_os_error()
    );
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
    assert!(create_file_link(
        Path::new(r"replacement\missing"),
        &original.join("source")
    ));

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

/// Verifies Rooted reading and copying follow the substitute name when the
/// reparse record's display name points at a different file.
#[test]
fn test_rooted_copy_uses_actual_reparse_target_instead_of_display_name() {
    let root = tempdir().expect("temporary root should be created");
    let actual = root.path().join("actual-payload");
    let display = root.path().join("display-payload");
    let actual_link_target = Path::new("actual-payload");
    fs::write(&actual, b"actual target").expect("actual target should be written");
    fs::write(&display, b"display target").expect("display target should be written");
    if !create_file_link(&actual, &root.path().join("source")) {
        return;
    }
    set_distinct_reparse_names(
        &root.path().join("source"),
        actual_link_target,
        Path::new("display-payload"),
    );
    let mut filesystem = LocalFileSystem::rooted(root.path()).expect("root authority should open");
    filesystem
        .set_symlink_policy(qubit_local_files::policy::LocalSymlinkPolicy::FollowWithinScope)
        .expect("Rooted policy should permit the in-scope target for this regression");

    let mut reader = filesystem
        .open_reader(Path::new("source"))
        .expect("Rooted reader should follow the actual target");
    let mut source_bytes = [0_u8; 13];
    Input::read_exactly(&mut reader, &mut source_bytes).expect("Rooted reader should read target bytes");
    assert_eq!(b"actual target", source_bytes.as_slice());

    let _ = filesystem
        .copy(Path::new("source"), Path::new("destination"))
        .expect("link with distinct names should copy through its actual target");

    assert_eq!(
        actual_link_target,
        fs::read_link(root.path().join("destination")).unwrap()
    );
    assert_eq!(
        b"actual target",
        fs::read(root.path().join("destination")).unwrap().as_slice()
    );
}
