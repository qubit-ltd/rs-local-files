// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::fs;
use std::path::Path;

use qubit_local_files::LocalFileSystem;
use qubit_local_files::error::LocalFileErrorKind;
use qubit_local_files::error::LocalFileOperation;
use qubit_local_files::outcome::LocalFileKind;
use tempfile::tempdir;

fn fixture() -> (tempfile::TempDir, LocalFileSystem) {
    let root = tempdir().expect("fixture root");
    fs::write(root.path().join("file"), b"data").expect("file fixture");
    fs::create_dir(root.path().join("dir")).expect("directory fixture");
    fs::create_dir(root.path().join("work")).expect("work directory fixture");
    let filesystem = LocalFileSystem::rooted(root.path()).expect("rooted filesystem");
    (root, filesystem)
}

#[test]
fn test_review_canonicalize_requires_existing_target_and_preserves_directory_intent() {
    let (_root, rooted) = fixture();
    for missing in ["/missing", "/missing/child"] {
        let error = rooted
            .canonicalize(Path::new(missing))
            .expect_err("missing target must not canonicalize");
        assert_eq!(error.kind(), LocalFileErrorKind::NotFound);
    }
    assert_eq!(rooted.canonicalize(Path::new("/")).expect("root"), Path::new("/"));
    assert_eq!(
        rooted.canonicalize(Path::new("/file")).expect("file"),
        Path::new("/file")
    );
    for qualified in ["/file/", "/file/."] {
        assert_eq!(
            rooted
                .canonicalize(Path::new(qualified))
                .expect_err("file cannot satisfy directory intent")
                .kind(),
            LocalFileErrorKind::NotDirectory,
        );
    }
    assert_eq!(
        rooted.canonicalize(Path::new("/dir/")).expect("directory"),
        Path::new("/dir")
    );

    let host_root = tempdir().expect("host root");
    fs::write(host_root.path().join("file"), b"data").expect("host file fixture");
    let host = LocalFileSystem::host().expect("host filesystem");
    let host_file = host_root.path().join("file");
    for missing in [host_root.path().join("missing"), host_root.path().join("missing/child")] {
        let error = host.canonicalize(&missing).expect_err("missing host target");
        assert_eq!(error.kind(), LocalFileErrorKind::NotFound);
    }
    for qualified in [
        format!("{}/", host_file.display()),
        format!("{}/.", host_file.display()),
    ] {
        assert_eq!(
            host.canonicalize(Path::new(&qualified))
                .expect_err("host file directory intent")
                .kind(),
            LocalFileErrorKind::NotDirectory,
        );
    }
    assert!(
        host.canonicalize(&host_root.path().join("file"))
            .expect("host existing file")
            .is_absolute()
    );
    for qualified in [
        format!("{}/", host_file.display()),
        format!("{}/.", host_file.display()),
    ] {
        assert_eq!(
            host.target_metadata(Path::new(&qualified))
                .expect_err("host target metadata file directory intent")
                .kind(),
            LocalFileErrorKind::NotDirectory,
        );
    }
    fs::create_dir(host_root.path().join("dir")).expect("host directory fixture");
    assert_eq!(
        host.target_metadata(Path::new(&format!("{}/", host_root.path().join("dir").display())))
            .expect("host directory metadata")
            .kind(),
        LocalFileKind::Directory,
    );
}

#[test]
fn test_review_target_metadata_preserves_root_alias_and_directory_intent() {
    let (root, filesystem) = fixture();
    #[cfg(unix)]
    {
        use std::os::unix::fs::symlink;
        symlink("/", root.path().join("root-alias")).expect("root alias");
        symlink("missing-target", root.path().join("dangling")).expect("dangling link");
        assert_eq!(
            filesystem
                .target_metadata(Path::new("/root-alias"))
                .expect("target metadata at virtual root")
                .kind(),
            LocalFileKind::Directory,
        );
        assert_eq!(
            filesystem
                .canonicalize(Path::new("/dangling"))
                .expect_err("dangling link canonicalization")
                .kind(),
            LocalFileErrorKind::NotFound,
        );
    }
    for qualified in ["/file/", "/file/."] {
        assert_eq!(
            filesystem
                .target_metadata(Path::new(qualified))
                .expect_err("target metadata must honor directory intent")
                .kind(),
            LocalFileErrorKind::NotDirectory,
        );
    }
    assert_eq!(
        filesystem
            .target_metadata(Path::new("/dir/"))
            .expect("directory target")
            .kind(),
        LocalFileKind::Directory,
    );
}

#[test]
fn test_review_metadata_errors_include_namespace_path_and_pwd() {
    let (_root, mut filesystem) = fixture();
    filesystem
        .set_current_directory(Path::new("/work"))
        .expect("set virtual PWD");
    let error = filesystem
        .target_metadata(Path::new("missing"))
        .expect_err("missing target metadata");
    assert_eq!(error.kind(), LocalFileErrorKind::NotFound);
    assert_eq!(error.operation(), LocalFileOperation::Metadata);
    assert_eq!(error.path(), Some(Path::new("/work/missing")));
    assert_eq!(error.current_directory(), Some(Path::new("/work")));
}

#[test]
fn test_review_host_metadata_errors_capture_pwd_only_for_relative_input() {
    let working = std::env::current_dir().expect("process PWD");
    let host = LocalFileSystem::host().expect("host filesystem");
    let relative_error = host
        .target_metadata(Path::new("review-missing-metadata-entry"))
        .expect_err("relative missing metadata");
    assert_eq!(
        relative_error.path(),
        Some(working.join("review-missing-metadata-entry").as_path())
    );
    assert_eq!(relative_error.current_directory(), Some(working.as_path()));
    let absolute = working.join("review-absent-metadata-entry");
    let absolute_error = host.target_metadata(&absolute).expect_err("absolute missing metadata");
    assert_eq!(absolute_error.path(), Some(absolute.as_path()));
    assert_eq!(absolute_error.current_directory(), None);
}
