// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

#[cfg(unix)]
use std::fs;
#[cfg(unix)]
use std::io::Read;
#[cfg(unix)]
use std::path::Path;

#[cfg(unix)]
use qubit_local_files::LocalFileErrorKind;
#[cfg(unix)]
use qubit_local_files::LocalFileKind;
#[cfg(unix)]
use qubit_local_files::LocalFileSystem;
#[cfg(unix)]
use qubit_local_files::LocalReadOptions;
#[cfg(unix)]
use qubit_local_files::LocalSymlinkPolicy;
#[cfg(unix)]
use tempfile::tempdir;

/// Verifies host resolution follows an intermediate symbolic link.
#[cfg(unix)]
#[test]
fn test_host_path_resolution_follows_intermediate_symlink() {
    use std::os::unix::fs::symlink;

    let directory = tempdir().expect("temporary directory should be created");
    let target = directory.path().join("target");
    fs::create_dir(&target).expect("target directory should be created");
    fs::write(target.join("payload"), b"payload").expect("target payload should be written");
    symlink(&target, directory.path().join("link")).expect("intermediate link should be created");

    let mut reader = LocalFileSystem::host()
        .open_reader(
            &directory.path().join("link/payload"),
            &LocalReadOptions::new(),
        )
        .expect("host resolution should follow the intermediate link");
    let mut content = String::new();
    reader
        .read_to_string(&mut content)
        .expect("resolved payload should be readable");
    assert_eq!("payload", content);
}

/// Verifies a rejecting host policy reports a required symbolic-link traversal.
#[cfg(unix)]
#[test]
fn test_host_path_resolution_rejects_required_symbolic_link() {
    use std::os::unix::fs::symlink;

    let directory = tempdir().expect("temporary directory should be created");
    let target = directory.path().join("target");
    fs::write(&target, b"payload").expect("target file should be written");
    let link = directory.path().join("link");
    symlink(&target, &link).expect("final link should be created");

    let error = LocalFileSystem::host()
        .with_symlink_policy(LocalSymlinkPolicy::Reject)
        .expect("Host should accept Reject")
        .open_reader(&link, &LocalReadOptions::new())
        .expect_err("rejecting policy must reject a followed final link");
    assert_eq!(LocalFileErrorKind::Unsupported, error.kind());
}

/// Verifies a followed dangling link retains the native resolution failure.
#[cfg(unix)]
#[test]
fn test_host_path_resolution_reports_dangling_followed_link() {
    use std::os::unix::fs::symlink;

    let directory = tempdir().expect("temporary directory should be created");
    let link = directory.path().join("dangling");
    symlink("missing-target", &link).expect("dangling link should be created");

    let error = LocalFileSystem::host()
        .open_reader(&link, &LocalReadOptions::new())
        .expect_err("following a dangling link must fail");
    assert_eq!(LocalFileErrorKind::NotFound, error.kind());
}

/// Verifies final links remain inspectable as entries when metadata does not
/// follow the final component.
#[cfg(unix)]
#[test]
fn test_host_path_resolution_preserves_final_link_for_metadata() {
    use std::os::unix::fs::symlink;

    let directory = tempdir().expect("temporary directory should be created");
    let target = directory.path().join("target");
    fs::write(&target, b"payload").expect("target file should be written");
    let link = directory.path().join("link");
    symlink(&target, &link).expect("final link should be created");

    let metadata = LocalFileSystem::host()
        .metadata(Path::new(&link))
        .expect("metadata should inspect the final link entry");
    assert_eq!(LocalFileKind::Symlink, metadata.kind());
}
