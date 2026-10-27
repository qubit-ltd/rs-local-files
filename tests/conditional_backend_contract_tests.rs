// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

#![cfg(feature = "digest")]

mod support;

use std::path::Path;
use std::path::PathBuf;
use std::process::Command;

use qubit_io::Output;
#[cfg(unix)]
use qubit_io::Streams;
use qubit_local_files::LocalFileSystem;
#[cfg(unix)]
use qubit_local_files::options::LocalReadOptions;
use qubit_local_files::options::LocalWriteMode;
use qubit_local_files::options::LocalWriteOptions;
use qubit_local_files::options::LocalWritePrecondition;
use qubit_local_files::outcome::LocalWritePublicationMethod;
use sha2::Digest;
use sha2::Sha256;
use support::conditional_temp_directory::tempdir;

#[cfg(unix)]
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
    let open_error = filesystem
        .open_conditional_writer::<Sha256>(
            Path::new("missing/model"),
            LocalWritePrecondition::Absent,
            &LocalWriteOptions::new(LocalWriteMode::CreateNew),
        )
        .expect_err("missing parent should fail while opening");
    assert_eq!(Some(Path::new("/dir")), open_error.current_directory());
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
    filesystem
        .set_current_directory(Path::new("/other"))
        .expect("virtual PWD should change before commit");
    let error = writer
        .commit()
        .expect_err("stale content condition should retain the candidate");
    assert_eq!(Some(Path::new("/dir")), error.error().current_directory());
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

#[cfg(unix)]
#[test]
fn rooted_abort_invalid_state_keeps_the_open_time_pwd_snapshot() {
    let parent = tempdir().expect("fixture parent should be created");
    let root_path = parent.path().join("root");
    std::fs::create_dir(&root_path).expect("root should be created");
    std::fs::create_dir(root_path.join("dir")).expect("virtual directory should be created");
    std::fs::create_dir(root_path.join("other")).expect("alternate directory should be created");
    let mut filesystem = LocalFileSystem::rooted(&root_path).expect("root authority should open");
    filesystem
        .set_symlink_policy(qubit_local_files::policy::LocalSymlinkPolicy::Reject)
        .expect("conditional writes should reject symlinks");
    filesystem
        .set_current_directory(Path::new("/dir"))
        .expect("virtual PWD should change");
    let mut writer = filesystem
        .open_conditional_writer::<Sha256>(
            Path::new("model"),
            LocalWritePrecondition::Absent,
            &LocalWriteOptions::new(LocalWriteMode::CreateNew),
        )
        .expect("writer should open relative to /dir");
    filesystem
        .set_current_directory(Path::new("/other"))
        .expect("virtual PWD should change after open");
    let _ = writer.abort().expect("first abort should succeed");

    let error = writer.abort().expect_err("aborted writer cannot be aborted again");
    assert_eq!(Some(Path::new("/dir")), error.current_directory());
}

#[test]
fn host_relative_commit_error_keeps_the_open_time_pwd_snapshot() {
    const CHILD_SENTINEL_ENV: &str = "QUBIT_LOCAL_FILES_HOST_CWD_SCENARIO_SENTINEL";
    const CHILD_SENTINEL_CONTENT: &[u8] = b"host-relative-cwd-scenario-complete";

    if let Some(sentinel) = std::env::var_os(CHILD_SENTINEL_ENV) {
        let original_directory = std::env::current_dir().expect("current directory should be readable");
        let directory = tempdir().expect("fixture parent should be created");
        let fixture_path = directory.path().to_path_buf();
        let restore = CurrentDirectoryRestore::new(original_directory);
        host_relative_commit_scenario(directory.path());
        restore
            .restore()
            .expect("child process current directory should be restored");
        drop(restore);
        drop(directory);
        assert!(
            !fixture_path.exists(),
            "child fixture should be deleted before writing the sentinel",
        );
        std::fs::write(sentinel, CHILD_SENTINEL_CONTENT)
            .expect("child should write its completion sentinel after cleanup");
        return;
    }

    let original_directory = std::env::current_dir().expect("current directory should be readable");
    let directory = tempdir().expect("fixture parent should be created");
    let sentinel = directory.path().join("child-complete");
    assert!(sentinel.is_absolute(), "child sentinel path should be absolute");
    let status = Command::new(std::env::current_exe().expect("test executable should be available"))
        .args([
            "--exact",
            "host_relative_commit_error_keeps_the_open_time_pwd_snapshot",
            "--nocapture",
        ])
        .env(CHILD_SENTINEL_ENV, &sentinel)
        .status()
        .expect("child test process should start");
    assert!(status.success(), "child test process failed with {status}");
    assert_eq!(
        CHILD_SENTINEL_CONTENT,
        std::fs::read(&sentinel).expect("child should leave its completion sentinel"),
        "child should execute the scenario, restore CWD, and clean its fixture",
    );
    assert_eq!(
        original_directory,
        std::env::current_dir().expect("current directory should be readable")
    );
}

struct CurrentDirectoryRestore {
    original: PathBuf,
}

impl CurrentDirectoryRestore {
    fn new(original: PathBuf) -> Self {
        Self { original }
    }

    fn restore(&self) -> std::io::Result<()> {
        std::env::set_current_dir(&self.original)
    }
}

impl Drop for CurrentDirectoryRestore {
    fn drop(&mut self) {
        let _ = self.restore();
    }
}

fn host_relative_commit_scenario(directory: &Path) {
    let first = directory.join("dir");
    let second = directory.join("other");
    std::fs::create_dir(&first).expect("first directory should be created");
    std::fs::create_dir(&second).expect("alternate directory should be created");
    std::fs::write(first.join("model"), b"old").expect("target should be written");
    let mut filesystem = LocalFileSystem::host().expect("Host filesystem should open");
    filesystem
        .set_symlink_policy(qubit_local_files::policy::LocalSymlinkPolicy::Reject)
        .expect("conditional writes should reject symlinks");
    filesystem
        .set_current_directory(&first)
        .expect("Host PWD should change");
    let mut writer = filesystem
        .open_conditional_writer::<Sha256>(
            Path::new("model"),
            LocalWritePrecondition::Content(Sha256::digest(b"stale")),
            &LocalWriteOptions::new(LocalWriteMode::CreateOrReplace),
        )
        .expect("relative writer should resolve under its captured PWD");
    writer.write_fully(b"candidate").expect("candidate should accept bytes");
    filesystem
        .set_current_directory(&second)
        .expect("Host PWD should change after writer creation");

    let error = writer.commit().expect_err("stale content should fail");

    assert_eq!(Some(first.as_path()), error.error().current_directory());
}

#[test]
fn host_absolute_commit_error_has_no_pwd_fallback() {
    let directory = tempdir().expect("fixture directory should be created");
    let target = directory.path().join("existing");
    std::fs::write(&target, b"old").expect("target should be created");
    let mut filesystem = LocalFileSystem::host().expect("Host filesystem should open");
    filesystem
        .set_symlink_policy(qubit_local_files::policy::LocalSymlinkPolicy::Reject)
        .expect("conditional writes should reject symlinks");
    let mut writer = filesystem
        .open_conditional_writer::<Sha256>(
            &target,
            LocalWritePrecondition::Absent,
            &LocalWriteOptions::new(LocalWriteMode::CreateOrReplace),
        )
        .expect("absolute Host writer should open");
    writer.write_fully(b"candidate").expect("candidate should accept bytes");
    let error = writer
        .commit()
        .expect_err("existing target should reject absent condition");
    assert_eq!(None, error.error().current_directory());
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
