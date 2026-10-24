// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

#![cfg(feature = "digest")]

use std::path::Path;

use qubit_io::Output;
use qubit_io::Streams;
use qubit_local_files::LocalFileSystem;
use qubit_local_files::options::LocalReadOptions;
use qubit_local_files::options::LocalWriteMode;
use qubit_local_files::options::LocalWriteOptions;
use qubit_local_files::options::LocalWritePrecondition;
use qubit_local_files::outcome::LocalWritePublicationMethod;
use sha2::Digest;
use sha2::Sha256;
use tempfile::tempdir;

fn read_all(mut reader: impl qubit_io::Input<Item = u8>) -> Vec<u8> {
    let mut bytes = Vec::new();
    Streams::copy_input_to_output(&mut reader, &mut bytes).expect("file contents should be readable");
    bytes
}

#[test]
#[cfg(unix)]
fn rooted_recovery_coordinate_is_absolute_and_reusable_after_pwd_change() {
    let parent = tempdir().expect("fixture parent should be created");
    let root_path = parent.path().join("root");
    std::fs::create_dir(&root_path).expect("root should be created");
    std::fs::create_dir(root_path.join("dir")).expect("virtual directory should be created");
    std::fs::create_dir(root_path.join("other")).expect("alternate directory should be created");
    std::fs::write(root_path.join("dir/model"), b"old").expect("old target should be written");

    let mut filesystem = LocalFileSystem::rooted(&root_path).expect("root authority should open");
    filesystem
        .set_symlink_policy(qubit_local_files::policy::LocalSymlinkPolicy::Reject)
        .expect("Rooted conditional writes should reject symlinks");
    filesystem
        .set_current_directory(Path::new("/dir"))
        .expect("virtual PWD should change");
    let mut writer = filesystem
        .open_conditional_writer::<Sha256>(
            Path::new("model"),
            LocalWritePrecondition::Content(Sha256::digest(b"expected stale content")),
            &LocalWriteOptions::new(LocalWriteMode::CreateOrReplace),
        )
        .expect("conditional candidate should open");
    writer
        .write_fully(b"candidate B")
        .expect("candidate should accept bytes");
    let error = writer
        .commit()
        .expect_err("stale content condition should retain the candidate");
    let candidate = error.candidate().expect("candidate recovery handle should be present");
    let candidate_path = candidate.namespace_path().to_path_buf();

    assert!(
        candidate_path.is_absolute(),
        "Rooted recovery paths should be namespace absolute"
    );
    assert_eq!(
        Path::new("/dir"),
        candidate_path.parent().expect("candidate should have a parent")
    );
    assert_eq!(
        None,
        candidate.diagnostic_path(),
        "virtual paths are not physical diagnostics"
    );
    assert_eq!(
        read_all(candidate.open_reader().expect("retained access should read candidate")),
        b"candidate B"
    );

    filesystem
        .set_current_directory(Path::new("/other"))
        .expect("virtual PWD should change after the failed write");
    assert_eq!(
        11,
        filesystem
            .metadata(&candidate_path)
            .expect("absolute namespace coordinate should resolve through the same Rooted instance")
            .len(),
    );
    let reader = filesystem
        .open_reader_with_options(&candidate_path, &LocalReadOptions::new())
        .expect("same-instance reader should reopen the retained candidate");
    assert_eq!(read_all(reader), b"candidate B");
}

#[test]
fn successful_absent_publication_reports_atomic_rename() {
    let directory = tempdir().expect("fixture directory should be created");
    let target = directory.path().join("new-target");
    let mut filesystem = LocalFileSystem::host().expect("Host filesystem should open");
    filesystem
        .set_symlink_policy(qubit_local_files::policy::LocalSymlinkPolicy::Reject)
        .expect("conditional writes should reject symlinks");
    let mut writer = filesystem
        .open_conditional_writer::<Sha256>(
            &target,
            LocalWritePrecondition::Absent,
            &LocalWriteOptions::new(LocalWriteMode::CreateNew),
        )
        .expect("conditional writer should open");
    writer.write_fully(b"new").expect("candidate should accept bytes");
    let outcome = writer.commit().expect("absent target should publish");

    assert_eq!(
        LocalWritePublicationMethod::AtomicRename,
        outcome.write_outcome().publication_method()
    );
}

#[cfg(unix)]
#[test]
fn successful_content_publication_reports_atomic_exchange() {
    let directory = tempdir().expect("fixture directory should be created");
    let target = directory.path().join("target");
    std::fs::write(&target, b"old").expect("old target should be written");
    let mut filesystem = LocalFileSystem::host().expect("Host filesystem should open");
    filesystem
        .set_symlink_policy(qubit_local_files::policy::LocalSymlinkPolicy::Reject)
        .expect("conditional writes should reject symlinks");
    let mut writer = filesystem
        .open_conditional_writer::<Sha256>(
            &target,
            LocalWritePrecondition::Content(Sha256::digest(b"old")),
            &LocalWriteOptions::new(LocalWriteMode::CreateOrReplace),
        )
        .expect("conditional writer should open");
    writer.write_fully(b"new").expect("candidate should accept bytes");
    let outcome = writer.commit().expect("matching content should publish");

    assert_eq!(
        LocalWritePublicationMethod::AtomicExchange,
        outcome.write_outcome().publication_method()
    );
}

#[cfg(windows)]
#[test]
fn windows_host_content_publication_reports_replace_with_backup() {
    let directory = tempdir().expect("fixture directory should be created");
    let target = directory.path().join("target");
    std::fs::write(&target, b"old").expect("old target should be written");
    let mut filesystem = LocalFileSystem::host().expect("Host filesystem should open");
    filesystem
        .set_symlink_policy(qubit_local_files::policy::LocalSymlinkPolicy::Reject)
        .expect("conditional writes should reject symlinks");
    let mut writer = filesystem
        .open_conditional_writer::<Sha256>(
            &target,
            LocalWritePrecondition::Content(Sha256::digest(b"old")),
            &LocalWriteOptions::new(LocalWriteMode::CreateOrReplace),
        )
        .expect("conditional writer should open");
    writer.write_fully(b"new").expect("candidate should accept bytes");
    let outcome = writer.commit().expect("matching content should publish");

    assert_eq!(
        LocalWritePublicationMethod::AtomicReplaceWithBackup,
        outcome.write_outcome().publication_method(),
    );
}
