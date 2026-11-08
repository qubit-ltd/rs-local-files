// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

#![cfg(unix)]

use std::fs;
use std::path::Path;

use qubit_io::Output;
use qubit_local_files::LocalFileSystem;
use qubit_local_files::options::LocalWriteMode;
use qubit_local_files::options::LocalWriteOptions;
use tempfile::tempdir;

fn displace_and_replace_staging(directory: &Path) -> (std::path::PathBuf, std::path::PathBuf) {
    let sentinel = directory.join("identity-sentinel");
    fs::write(&sentinel, b"untouched sentinel").expect("create independent sentinel");
    let mut candidates = fs::read_dir(directory)
        .expect("directory listing")
        .map(|entry| entry.expect("entry").path())
        .filter(|path| path != &sentinel)
        .collect::<Vec<_>>();
    assert_eq!(
        1,
        candidates.len(),
        "one staging entry should remain after excluding sentinel"
    );
    let staged = candidates.pop().expect("staging entry");
    let displaced = directory.join("displaced-staging");
    fs::rename(&staged, &displaced).expect("move original staging entry");
    fs::write(&staged, b"replacement").expect("create replacement entry");
    (staged, displaced)
}

fn assert_sentinel_unchanged(directory: &Path) {
    assert_eq!(
        b"untouched sentinel",
        fs::read(directory.join("identity-sentinel"))
            .expect("sentinel remains")
            .as_slice()
    );
}

fn write_staged_payload(writer: &mut impl Output<Item = u8>) {
    writer
        .write_fully(b"original staged payload")
        .expect("write staged payload");
}

#[test]
fn test_host_staged_cleanup_preserves_replacement_identity() {
    let fixture = tempdir().expect("fixture");
    let target = fixture.path().join("destination");
    let mut writer = LocalFileSystem::host()
        .expect("Host filesystem")
        .open_writer(&target)
        .expect("staged writer");
    write_staged_payload(&mut writer);
    let (staged, displaced) = displace_and_replace_staging(fixture.path());

    let error = writer.abort().expect_err("cleanup must reject replacement identity");
    assert_eq!(
        b"replacement",
        fs::read(&staged).expect("replacement remains").as_slice()
    );
    assert_eq!(
        b"original staged payload",
        fs::read(&displaced).expect("original staging remains").as_slice()
    );
    assert_sentinel_unchanged(fixture.path());
    drop(error);
}

#[test]
fn test_unix_rooted_staged_cleanup_preserves_replacement_identity() {
    let fixture = tempdir().expect("fixture");
    let filesystem = LocalFileSystem::rooted(fixture.path()).expect("Rooted filesystem");
    let mut writer = filesystem.open_writer(Path::new("destination")).expect("staged writer");
    write_staged_payload(&mut writer);
    let (staged, displaced) = displace_and_replace_staging(fixture.path());

    let error = writer.abort().expect_err("cleanup must reject replacement identity");
    assert_eq!(
        b"replacement",
        fs::read(&staged).expect("replacement remains").as_slice()
    );
    assert_eq!(
        b"original staged payload",
        fs::read(&displaced).expect("original staging remains").as_slice()
    );
    assert_sentinel_unchanged(fixture.path());
    drop(error);
}

#[test]
fn test_host_staged_drop_preserves_replacement_identity() {
    let fixture = tempdir().expect("fixture");
    let target = fixture.path().join("destination");
    let mut writer = LocalFileSystem::host()
        .expect("Host filesystem")
        .open_writer(&target)
        .expect("staged writer");
    write_staged_payload(&mut writer);
    let (staged, displaced) = displace_and_replace_staging(fixture.path());
    drop(writer);

    assert_eq!(
        b"replacement",
        fs::read(&staged).expect("replacement remains").as_slice()
    );
    assert_eq!(
        b"original staged payload",
        fs::read(&displaced).expect("original staging remains").as_slice()
    );
    assert_sentinel_unchanged(fixture.path());
}

#[test]
fn test_create_new_commit_failure_preserves_replacement_staging_identity() {
    let fixture = tempdir().expect("fixture");
    let target = fixture.path().join("destination");
    let mut writer = LocalFileSystem::host()
        .expect("Host filesystem")
        .open_writer_with_options(&target, &LocalWriteOptions::new(LocalWriteMode::CreateNew))
        .expect("staged writer");
    write_staged_payload(&mut writer);
    let (staged, displaced) = displace_and_replace_staging(fixture.path());
    fs::write(&target, b"concurrent target").expect("create competing destination");

    let error = writer
        .commit()
        .expect_err("create-new must reject the competing destination");
    drop(error);

    assert_eq!(
        b"replacement",
        fs::read(&staged).expect("replacement remains").as_slice()
    );
    assert_eq!(
        b"original staged payload",
        fs::read(&displaced).expect("original staging remains").as_slice()
    );
    assert_eq!(
        b"concurrent target",
        fs::read(&target).expect("competing destination remains").as_slice()
    );
    assert_sentinel_unchanged(fixture.path());
}

#[test]
fn test_unix_rooted_staged_drop_preserves_replacement_identity() {
    let fixture = tempdir().expect("fixture");
    let filesystem = LocalFileSystem::rooted(fixture.path()).expect("Rooted filesystem");
    let mut writer = filesystem.open_writer(Path::new("destination")).expect("staged writer");
    write_staged_payload(&mut writer);
    let (staged, displaced) = displace_and_replace_staging(fixture.path());
    drop(writer);

    assert_eq!(
        b"replacement",
        fs::read(&staged).expect("replacement remains").as_slice()
    );
    assert_eq!(
        b"original staged payload",
        fs::read(&displaced).expect("original staging remains").as_slice()
    );
    assert_sentinel_unchanged(fixture.path());
}

#[test]
fn test_unix_rooted_create_new_commit_failure_preserves_replacement_staging_identity() {
    let fixture = tempdir().expect("fixture");
    let filesystem = LocalFileSystem::rooted(fixture.path()).expect("Rooted filesystem");
    let mut writer = filesystem
        .open_writer_with_options(
            Path::new("destination"),
            &LocalWriteOptions::new(LocalWriteMode::CreateNew),
        )
        .expect("staged writer");
    write_staged_payload(&mut writer);
    let (staged, displaced) = displace_and_replace_staging(fixture.path());
    fs::write(fixture.path().join("destination"), b"concurrent target").expect("create competing destination");

    let error = writer
        .commit()
        .expect_err("create-new must reject the competing destination");
    drop(error);

    assert_eq!(
        b"replacement",
        fs::read(&staged).expect("replacement remains").as_slice()
    );
    assert_eq!(
        b"original staged payload",
        fs::read(&displaced).expect("original staging remains").as_slice()
    );
    assert_eq!(
        b"concurrent target",
        fs::read(fixture.path().join("destination"))
            .expect("competing destination remains")
            .as_slice()
    );
    assert_sentinel_unchanged(fixture.path());
}
