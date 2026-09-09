// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::fs;
use std::path::Path;

use qubit_local_files::LocalFileSystem;
use qubit_local_files::outcome::LocalFileMetadata;
use tempfile::tempdir;

fn assert_permissions_equal(actual: &LocalFileMetadata, expected: &fs::Metadata) {
    assert_eq!(
        actual.permissions().is_read_only(),
        expected.permissions().readonly(),
        "portable read-only observation must match the native entry",
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            actual.permissions().unix_mode(),
            Some(expected.permissions().mode() & 0o7777),
            "Unix mode bits must survive metadata conversion",
        );
    }
    #[cfg(windows)]
    assert_eq!(actual.permissions().unix_mode(), None);
}

#[test]
fn test_read_only_file_permissions_match_across_public_entry_points() {
    let directory = tempdir().expect("create fixture");
    let physical = directory.path().join("payload");
    fs::write(&physical, b"payload").expect("write fixture");
    let original = fs::metadata(&physical).expect("original metadata").permissions();
    let mut read_only = original.clone();
    read_only.set_readonly(true);
    fs::set_permissions(&physical, read_only).expect("set read-only permissions");

    let native = fs::symlink_metadata(&physical).expect("native metadata");
    let host = LocalFileSystem::host().expect("Host filesystem");
    let rooted = LocalFileSystem::rooted(directory.path()).expect("Rooted filesystem");
    let host_metadata = host.metadata(&physical).expect("Host metadata");
    let rooted_metadata = rooted.metadata(Path::new("payload")).expect("Rooted metadata");
    let reader_metadata = rooted
        .open_reader(Path::new("payload"))
        .expect("Rooted reader")
        .metadata()
        .clone();
    let host_entries = host
        .list(directory.path())
        .expect("Host list")
        .collect::<Result<Vec<_>, _>>()
        .expect("Host entries");
    let rooted_entries = rooted
        .list(Path::new("/"))
        .expect("Rooted list")
        .collect::<Result<Vec<_>, _>>()
        .expect("Rooted entries");
    fs::set_permissions(&physical, original).expect("restore permissions before assertions");

    assert!(native.permissions().readonly());
    assert_eq!(host_entries.len(), 1);
    assert_eq!(rooted_entries.len(), 1);
    for observed in [
        &host_metadata,
        &rooted_metadata,
        &reader_metadata,
        host_entries[0].metadata(),
        rooted_entries[0].metadata(),
    ] {
        assert_permissions_equal(observed, &native);
    }
}

#[cfg(unix)]
#[test]
fn test_directory_root_and_listing_preserve_unix_mode_bits() {
    use std::os::unix::fs::PermissionsExt;

    let directory = tempdir().expect("create fixture");
    let child = directory.path().join("child");
    fs::create_dir(&child).expect("create child");
    fs::set_permissions(&child, fs::Permissions::from_mode(0o750)).expect("set child mode");
    let rooted = LocalFileSystem::rooted(directory.path()).expect("Rooted filesystem");
    let root_native = fs::symlink_metadata(directory.path()).expect("native root");
    let child_native = fs::symlink_metadata(&child).expect("native child");
    assert_permissions_equal(&rooted.metadata(Path::new("/")).expect("root metadata"), &root_native);
    assert_permissions_equal(
        &rooted.metadata(Path::new("child")).expect("child metadata"),
        &child_native,
    );
    let entries = rooted
        .list(Path::new("/"))
        .expect("root list")
        .collect::<Result<Vec<_>, _>>()
        .expect("entries");
    assert_eq!(entries.len(), 1);
    assert_permissions_equal(entries[0].metadata(), &child_native);
}

#[cfg(unix)]
#[test]
fn test_symlink_metadata_and_reader_observe_their_respective_objects() {
    use std::os::unix::fs::PermissionsExt;
    use std::os::unix::fs::symlink;

    let directory = tempdir().expect("create fixture");
    let physical = directory.path().join("payload");
    fs::write(&physical, b"payload").expect("write fixture");
    fs::set_permissions(&physical, fs::Permissions::from_mode(0o640)).expect("set payload mode");
    let link = directory.path().join("link");
    symlink("payload", &link).expect("create relative link");
    let rooted = LocalFileSystem::rooted(directory.path()).expect("Rooted filesystem");
    assert_permissions_equal(
        &rooted.metadata(Path::new("link")).expect("link metadata"),
        &fs::symlink_metadata(&link).expect("native link metadata"),
    );
    assert_permissions_equal(
        rooted.open_reader(Path::new("link")).expect("linked reader").metadata(),
        &fs::metadata(&physical).expect("native target metadata"),
    );
}
