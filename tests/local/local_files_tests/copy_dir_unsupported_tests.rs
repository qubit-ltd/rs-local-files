// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::io::ErrorKind;

use super::super::api_tests::{
    LocalCopyConflictPolicy,
    LocalCopyDirOptions,
    LocalCopyDirStage,
};

use super::super::test_support::{
    fs,
    temp_dir,
};

#[test]
fn test_copy_dir_fail_and_skip_require_native_no_replace_installation() {
    for conflict in
        [LocalCopyConflictPolicy::Fail, LocalCopyConflictPolicy::Skip]
    {
        let dir = temp_dir("copy-dir-unsupported-no-replace");
        let source = dir.join("source");
        let destination = dir.join("destination");
        fs::create_dir(&source).expect("source directory should be created");
        fs::write(source.join("data.txt"), b"payload")
            .expect("source file should be written");

        let error = qubit_local_files::copy::directory(
            &source,
            &destination,
            LocalCopyDirOptions::new().with_conflict(conflict),
        )
        .expect_err("no-replace file commit should be unsupported");

        assert_eq!(LocalCopyDirStage::CommitFile, error.stage());
        assert_eq!(ErrorKind::Unsupported, error.kind());
        assert!(!destination.join("data.txt").exists());
        assert!(destination.is_dir());
        fs::remove_dir_all(dir).expect("copy fixture should be removed");
    }
}

#[test]
fn test_copy_dir_overwrite_uses_ordinary_replacement() {
    let dir = temp_dir("copy-dir-unsupported-overwrite");
    let source = dir.join("source");
    let destination = dir.join("destination");
    fs::create_dir(&source).expect("source directory should be created");
    fs::write(source.join("data.txt"), b"payload")
        .expect("source file should be written");

    qubit_local_files::copy::directory(
        &source,
        &destination,
        LocalCopyDirOptions::new()
            .with_conflict(LocalCopyConflictPolicy::Overwrite),
    )
    .expect("overwrite copy should use ordinary replacement");

    assert_eq!(
        b"payload",
        fs::read(destination.join("data.txt"))
            .expect("copied file should be readable")
            .as_slice()
    );
    fs::remove_dir_all(dir).expect("copy fixture should be removed");
}
