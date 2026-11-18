// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

#![cfg(feature = "digest")]

mod support;

use qubit_local_files::LocalFileSystem;
use qubit_local_files::error::LocalFileErrorKind;
use qubit_local_files::options::LocalWriteMode;
use qubit_local_files::options::LocalWriteOptions;
use qubit_local_files::options::LocalWritePrecondition;
use qubit_local_files::policy::LocalSymlinkPolicy;
use sha2::Sha256;

use crate::support::conditional_temp_directory::tempdir;

#[test]
fn test_conditional_writer_rejects_directory_qualified_regular_file_without_staging() {
    let directory = tempdir().expect("fixture directory should be created");
    let target = directory.path().join("regular-file");
    std::fs::write(&target, b"original").expect("target fixture should be written");
    let directory_qualified = std::path::PathBuf::from(format!("{}/", target.display()));
    let mut filesystem = LocalFileSystem::host().expect("Host filesystem should open");
    filesystem
        .set_symlink_policy(LocalSymlinkPolicy::Reject)
        .expect("conditional writes should reject symlinks");

    let error = filesystem
        .open_conditional_writer::<Sha256>(
            &directory_qualified,
            LocalWritePrecondition::Absent,
            &LocalWriteOptions::new(LocalWriteMode::CreateNew),
        )
        .expect_err("a regular file cannot satisfy a directory-qualified operand");

    assert_eq!(error.kind(), LocalFileErrorKind::InvalidPath);
    assert_eq!(Some(directory_qualified.as_path()), error.path());
    assert_eq!(
        b"original",
        std::fs::read(&target)
            .expect("target should remain readable")
            .as_slice()
    );
    assert_eq!(
        1,
        std::fs::read_dir(directory.path())
            .expect("fixture should be readable")
            .count()
    );
}
