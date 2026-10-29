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

#[cfg(windows)]
mod support;

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
    let fixture = tempfile::tempdir().expect("fixture should be created");
    std::fs::write(fixture.path().join("x"), b"x").expect("target file should be created");
    create_current_steps_reparse_link(&fixture.path().join("l"));
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

#[cfg(windows)]
fn create_current_steps_reparse_link(link: &Path) {
    use std::os::windows::fs::OpenOptionsExt;
    use std::os::windows::io::AsRawHandle;
    use std::ptr::null_mut;

    use windows_sys::Win32::Storage::FileSystem::FILE_FLAG_BACKUP_SEMANTICS;
    use windows_sys::Win32::Storage::FileSystem::FILE_FLAG_OPEN_REPARSE_POINT;
    use windows_sys::Win32::System::IO::DeviceIoControl;
    use windows_sys::Win32::System::Ioctl::FSCTL_GET_REPARSE_POINT;
    use windows_sys::Win32::System::Ioctl::FSCTL_SET_REPARSE_POINT;

    const RAW_TARGET: [u16; 5] = [0x002e, 0x005c, 0x002e, 0x005c, 0x002e];

    assert!(
        support::windows_symlink::create_directory_link(Path::new("."), link),
        "Windows directory link fixture should be available"
    );
    let substitute_bytes = (RAW_TARGET.len() * 2) as u16;
    let print_bytes = substitute_bytes;
    let data_length = 12 + usize::from(substitute_bytes) + usize::from(print_bytes);
    let mut buffer = vec![0_u8; 20 + usize::from(substitute_bytes) + usize::from(print_bytes)];
    buffer[0..4].copy_from_slice(&0xA000_000Cu32.to_le_bytes());
    buffer[4..6].copy_from_slice(&(data_length as u16).to_le_bytes());
    buffer[8..10].copy_from_slice(&0_u16.to_le_bytes());
    buffer[10..12].copy_from_slice(&substitute_bytes.to_le_bytes());
    buffer[12..14].copy_from_slice(&substitute_bytes.to_le_bytes());
    buffer[14..16].copy_from_slice(&print_bytes.to_le_bytes());
    buffer[16..20].copy_from_slice(&1_u32.to_le_bytes());
    for (index, unit) in RAW_TARGET.iter().chain(RAW_TARGET.iter()).enumerate() {
        let start = 20 + index * 2;
        buffer[start..start + 2].copy_from_slice(&unit.to_le_bytes());
    }

    let file = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .custom_flags(FILE_FLAG_OPEN_REPARSE_POINT | FILE_FLAG_BACKUP_SEMANTICS)
        .open(link)
        .expect("reparse point should open without following");
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
        "relative reparse target should be installed: {}",
        std::io::Error::last_os_error()
    );

    let mut readback = vec![0_u8; 1024];
    // SAFETY: file remains open and readback is a writable buffer of the stated
    // size.
    let result = unsafe {
        DeviceIoControl(
            file.as_raw_handle(),
            FSCTL_GET_REPARSE_POINT,
            null_mut(),
            0,
            readback.as_mut_ptr().cast(),
            readback.len() as u32,
            &raw mut returned,
            null_mut(),
        )
    };
    assert_ne!(
        0,
        result,
        "relative reparse target should be readable: {}",
        std::io::Error::last_os_error()
    );
    assert!(returned >= 40, "reparse readback should include both target names");
    assert_eq!(u32::from_le_bytes(readback[0..4].try_into().unwrap()), 0xA000_000C);
    let substitute_name_offset = u16::from_le_bytes([readback[8], readback[9]]);
    let substitute_name_length = u16::from_le_bytes([readback[10], readback[11]]);
    let print_name_offset = u16::from_le_bytes([readback[12], readback[13]]);
    let print_name_length = u16::from_le_bytes([readback[14], readback[15]]);
    assert_eq!(substitute_name_offset, 0);
    assert_eq!(
        substitute_name_length, 10,
        "five raw UTF-16 code units occupy ten bytes"
    );
    assert_eq!(print_name_offset, 10);
    assert_eq!(print_name_length, 10, "five raw UTF-16 code units occupy ten bytes");
    let substitute_offset = 20 + usize::from(substitute_name_offset);
    let substitute_length = usize::from(substitute_name_length);
    let actual: Vec<u16> = readback[substitute_offset..substitute_offset + substitute_length]
        .chunks_exact(2)
        .map(|bytes| u16::from_le_bytes([bytes[0], bytes[1]]))
        .collect();
    assert_eq!(actual, RAW_TARGET, "fixture must preserve the raw UTF-16 substitute");
    let print_offset = 20 + usize::from(print_name_offset);
    let print_length = usize::from(print_name_length);
    let actual_print: Vec<u16> = readback[print_offset..print_offset + print_length]
        .chunks_exact(2)
        .map(|bytes| u16::from_le_bytes([bytes[0], bytes[1]]))
        .collect();
    assert_eq!(
        actual_print, RAW_TARGET,
        "fixture print name should match the raw UTF-16 target"
    );
    assert_eq!(
        u32::from_le_bytes(readback[16..20].try_into().unwrap()),
        1,
        "fixture must be relative"
    );
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
