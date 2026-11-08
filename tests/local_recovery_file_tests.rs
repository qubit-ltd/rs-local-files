// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

#![cfg(feature = "digest")]

mod support;

use std::fs::read;
use std::fs::write;

use qubit_io::Output;
use qubit_io::Streams;
use qubit_local_files::LocalFileSystem;
use qubit_local_files::options::LocalWriteMode;
use qubit_local_files::options::LocalWriteOptions;
use qubit_local_files::options::LocalWritePrecondition;
use qubit_local_files::policy::LocalSymlinkPolicy;
use qubit_local_files::recovery::LocalRecoveryFileRole;
use sha2::Sha256;
use support::conditional_temp_directory::tempdir;

#[test]
fn test_recovery_file_drop_keeps_file_and_reader_works() {
    let directory = tempdir().expect("create temporary directory");
    let target = directory.path().join("target");
    write(&target, b"existing").expect("write existing target");
    let mut filesystem = LocalFileSystem::host().expect("host filesystem");
    filesystem
        .set_symlink_policy(LocalSymlinkPolicy::Reject)
        .expect("reject symbolic links");
    let mut writer = filesystem
        .open_conditional_writer::<Sha256>(
            &target,
            LocalWritePrecondition::Absent,
            &LocalWriteOptions::new(LocalWriteMode::CreateOrReplace),
        )
        .expect("open conditional writer");
    writer.write_fully(b"candidate B").expect("write candidate");
    let failure = writer.commit().expect_err("existing target prevents commit");
    let retained_path = {
        let recovery = failure.candidate().expect("candidate recovery file");
        assert_eq!(recovery.role(), LocalRecoveryFileRole::CandidateCopy);
        let path = recovery.namespace_path().to_path_buf();
        let mut reader = recovery.open_reader().expect("open retained recovery file");
        let mut contents = Vec::new();
        Streams::copy_input_to_output(&mut reader, &mut contents).expect("read recovery content");
        assert_eq!(contents, b"candidate B");
        path
    };
    drop(failure);

    assert_eq!(
        read(retained_path).expect("recovery file remains after drop"),
        b"candidate B"
    );
}
