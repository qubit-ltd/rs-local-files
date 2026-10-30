// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Windows regressions for ordinary and extended absolute root prefixes.

#![cfg(windows)]

use std::ffi::OsString;
use std::fs;
use std::os::windows::ffi::OsStrExt;
use std::os::windows::ffi::OsStringExt;
use std::path::Path;
use std::path::PathBuf;

use qubit_io::Input;
use qubit_local_files::LocalFileSystem;
use tempfile::tempdir;

mod support;
use support::windows_symlink::create_directory_link;

fn with_verbatim_prefix(path: &Path) -> PathBuf {
    let wide: Vec<u16> = path.as_os_str().encode_wide().collect();
    assert!(wide.len() >= 3, "absolute disk path should include a prefix");
    let mut result: Vec<u16> = r"\\?\".encode_utf16().collect();
    result.extend_from_slice(&wide);
    PathBuf::from(OsString::from_wide(&result))
}

fn with_disk_prefix(path: &Path) -> PathBuf {
    let wide: Vec<u16> = path.as_os_str().encode_wide().collect();
    let verbatim_prefix: Vec<u16> = r"\\?\".encode_utf16().collect();
    let disk_path = wide
        .as_slice()
        .strip_prefix(verbatim_prefix.as_slice())
        .unwrap_or(&wide);
    PathBuf::from(OsString::from_wide(disk_path))
}

fn ascii_uppercase_unit(unit: u16) -> u16 {
    if (u16::from(b'a')..=u16::from(b'z')).contains(&unit) {
        unit - u16::from(b'a' - b'A')
    } else {
        unit
    }
}

fn read_root_payload(root: &Path) -> Result<Vec<u8>, qubit_local_files::LocalFileError> {
    let filesystem = LocalFileSystem::rooted(root)?;
    let mut reader = filesystem.open_reader(Path::new("payload"))?;
    let mut bytes = [0_u8; 14];
    Input::read_exactly(&mut reader, &mut bytes).expect("payload should be readable");
    Ok(bytes.to_vec())
}

/// Checks all ordinary/extended prefix pairings against the actual link target.
#[test]
fn test_root_open_accepts_equivalent_disk_prefix_spellings() {
    let parent = tempdir().expect("temporary parent should be created");
    let target = parent.path().join("target");
    fs::create_dir(&target).expect("target directory should be created");
    fs::write(target.join("payload"), b"correct target").expect("payload should be written");

    assert_eq!(target, with_disk_prefix(&with_verbatim_prefix(&target)));
    let target_forms = [with_disk_prefix(&target), with_verbatim_prefix(&target)];
    for (input_index, input_form) in [false, true].into_iter().enumerate() {
        for (target_index, target_form) in target_forms.iter().enumerate() {
            let link = parent.path().join(format!("link-{input_index}-{target_index}"));
            if !create_directory_link(target_form, &link) {
                eprintln!(
                    "skipping equivalent prefix pairing {input_index}/{target_index}: directory symlink creation is unavailable"
                );
                continue;
            }
            let input = if input_form { with_verbatim_prefix(&link) } else { link };
            let actual = read_root_payload(&input).unwrap_or_else(|error| {
                panic!("equivalent prefix pairing {input_index}/{target_index} should open: {error}")
            });
            assert_eq!(b"correct target", actual.as_slice());
        }
    }
}

/// Checks that a target with a different drive anchor is rejected.
#[test]
fn test_root_open_rejects_different_drive_prefix() {
    let parent = tempdir().expect("temporary parent should be created");
    let link = parent.path().join("different-drive-link");
    let root_drive = ascii_uppercase_unit(
        with_disk_prefix(parent.path())
            .as_os_str()
            .encode_wide()
            .next()
            .expect("root should have a drive prefix"),
    );
    let other_drive = (b'A'..=b'Z')
        .map(|letter| PathBuf::from(format!("{}:\\", char::from(letter))))
        .filter(|drive| {
            drive
                .as_os_str()
                .encode_wide()
                .next()
                .is_some_and(|letter| ascii_uppercase_unit(letter) != root_drive)
                && drive.is_dir()
        })
        .find_map(|drive| tempfile::tempdir_in(drive).ok());
    let Some(other_drive) = other_drive else {
        eprintln!("skipping different-drive anchor case: no other writable drive is available");
        return;
    };
    fs::write(other_drive.path().join("payload"), b"correct target").expect("other-drive payload should be written");
    if !create_directory_link(other_drive.path(), &link) {
        eprintln!("skipping different-drive anchor case: directory symlink creation is unavailable");
        return;
    }
    let error = read_root_payload(&link).expect_err("a different drive anchor must be rejected");
    assert_eq!(qubit_local_files::error::LocalFileErrorKind::InvalidPath, error.kind());
    assert_eq!(
        Some("absolute symbolic-link target uses a different native root"),
        error.reason()
    );
}

// No controlled UNC share is available in this test fixture; UNC and
// VerbatimUNC equivalence remains unverified until a Windows share is provided.
