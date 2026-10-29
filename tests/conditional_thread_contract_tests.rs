// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

#![cfg(feature = "digest")]

use std::error::Error;
use std::thread;

use qubit_io::Input;
use qubit_io::Output;
use qubit_local_files::LocalFileSystem;
use qubit_local_files::options::LocalWriteMode;
use qubit_local_files::options::LocalWriteOptions;
use qubit_local_files::options::LocalWritePrecondition;
use qubit_local_files::recovery::LocalRecoveryFile;
use sha2::Sha256;
use tempfile::tempdir;

fn assert_send_sync<T: Send + Sync>() {}

fn into_send_sync_error<E: Error + Send + Sync + 'static>(error: E) -> Box<dyn Error + Send + Sync> {
    Box::new(error)
}

fn read_recovery_file(file: &LocalRecoveryFile) -> Vec<u8> {
    let mut reader = file.open_reader().expect("open retained recovery file");
    let mut contents = Vec::new();
    let mut buffer = [0_u8; 128];
    loop {
        match reader.read(&mut buffer).expect("read retained recovery file") {
            0 => return contents,
            count => contents.extend_from_slice(&buffer[..count]),
        }
    }
}

#[test]
fn test_conditional_recovery_types_are_send_sync() {
    assert_send_sync::<qubit_local_files::recovery::LocalRecoveryFile>();
    assert_send_sync::<qubit_local_files::error::LocalConditionalCommitError>();
    assert_send_sync::<qubit_local_files::outcome::LocalConditionalWriteOutcome<Sha256>>();
}

#[test]
fn conditional_commit_failure_can_move_across_threads_and_keeps_recovery_file() {
    let directory = tempdir().expect("create temporary directory");
    let target = directory
        .path()
        .canonicalize()
        .expect("canonicalize temporary directory")
        .join("target");
    std::fs::write(&target, b"preexisting").expect("create conflicting target");

    let mut filesystem = LocalFileSystem::host().expect("open host filesystem");
    filesystem
        .set_symlink_policy(qubit_local_files::policy::LocalSymlinkPolicy::Reject)
        .expect("reject symlinks");
    let mut writer = filesystem
        .open_conditional_writer::<Sha256>(
            &target,
            LocalWritePrecondition::Absent,
            &LocalWriteOptions::new(LocalWriteMode::CreateOrReplace),
        )
        .expect("open conditional writer");
    writer.write_fully(b"candidate").expect("write candidate");
    let failure = writer.commit().expect_err("existing target must conflict");
    let boxed_failure: Box<dyn Error + Send + Sync> = into_send_sync_error(failure);
    let failure = boxed_failure
        .downcast::<qubit_local_files::error::LocalConditionalCommitError>()
        .expect("boxed failure retains its concrete type");
    let target_bytes = std::fs::read(&target).expect("read preexisting target");
    assert_eq!(target_bytes, b"preexisting");
    let before_transfer: Vec<_> = failure
        .recovery_files()
        .iter()
        .map(|file| {
            let namespace_path = file.namespace_path().to_path_buf();
            let role = file.role();
            let bytes =
                std::fs::read(&namespace_path).expect("read Host recovery file before moving it to another thread");
            (namespace_path, role, bytes)
        })
        .collect();
    let candidate = before_transfer
        .iter()
        .find(|(_, role, _)| *role == qubit_local_files::recovery::LocalRecoveryFileRole::CandidateCopy)
        .expect("candidate copy is retained");
    assert_ne!(candidate.0, target, "candidate must use a distinct path");
    assert_ne!(candidate.2, target_bytes, "candidate must contain different bytes");

    let recovery_file_count = before_transfer.len();
    let recovery_entries = thread::spawn(move || {
        let failure = *failure;
        assert_eq!(
            failure.publication(),
            qubit_local_files::outcome::LocalConditionalPublicationState::NotPublished
        );
        assert_eq!(
            read_recovery_file(failure.candidate().expect("retained candidate")),
            b"candidate"
        );

        let recovery_files = failure.into_recovery_files();
        assert!(!recovery_files.is_empty(), "candidate should transfer to caller");
        assert_eq!(recovery_files.len(), recovery_file_count);
        let transferred: Vec<_> = recovery_files
            .iter()
            .map(|file| (file.namespace_path().to_path_buf(), file.role()))
            .collect();
        drop(recovery_files);
        transferred
    })
    .join()
    .expect("recovery result should move across thread");

    let transferred = recovery_entries;
    for (namespace_path, role, bytes) in before_transfer {
        assert!(transferred.contains(&(namespace_path.clone(), role)));
        assert_eq!(
            std::fs::read(&namespace_path).expect("read recovery file after handles are dropped"),
            bytes,
            "Drop must preserve the {role:?} file at {namespace_path:?}"
        );
    }
    assert_eq!(
        std::fs::read(&target).expect("read target after recovery Drop"),
        target_bytes
    );
}
