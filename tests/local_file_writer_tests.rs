// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

#[cfg(unix)]
use std::env;
use std::fs;
#[cfg(unix)]
use std::io::IoSlice;
use std::io::Write;
#[cfg(unix)]
use std::process::Command;

use qubit_local_files::LocalAtomicityRequirement;
#[cfg(unix)]
use qubit_local_files::LocalDurabilityRequirement;
use qubit_local_files::LocalFileErrorKind;
use qubit_local_files::LocalFileSystem;
#[cfg(unix)]
use qubit_local_files::LocalFileWriter;
use qubit_local_files::LocalWriteFailureState;
use qubit_local_files::LocalWriteMode;
use qubit_local_files::LocalWriteOptions;
use qubit_local_files::LocalWritePublicationMethod;
use qubit_local_files::LocalWriterState;
use tempfile::tempdir;

/// Environment switch used by the file-size-limit subprocess regression.
#[cfg(unix)]
const INDETERMINATE_APPEND_CASE: &str = "QUBIT_LOCAL_FILES_INDETERMINATE_APPEND_CASE";

/// Verifies staged replacement is invisible until commit.
#[test]
fn test_local_file_writer_publishes_staged_content_on_commit() {
    let directory = tempdir().expect("temporary directory should be created");
    let target = directory.path().join("target");
    fs::write(&target, b"old").expect("target fixture should be written");

    let mut writer = LocalFileSystem::host()
        .open_writer(&target, &LocalWriteOptions::new(LocalWriteMode::CreateOrReplace))
        .expect("staged writer should open");
    writer.write_all(b"new").expect("staged content should be written");
    assert_eq!(b"old", fs::read(&target).expect("old target should remain").as_slice());

    let outcome = writer.commit().expect("commit should publish staged content");
    assert_eq!(LocalWriterState::Committed, outcome.state());
    assert!(outcome.atomic());
    assert_eq!(LocalWritePublicationMethod::AtomicRename, outcome.publication_method());
    assert_eq!(3, outcome.bytes_written());
    assert_eq!(b"new", fs::read(&target).expect("target should be replaced").as_slice());
}

/// Verifies that overwrite publication follows a target symlink.
#[cfg(unix)]
#[test]
fn test_local_file_writer_follows_target_symlink() {
    use std::os::unix::fs::symlink;

    let directory = tempdir().expect("temporary directory should be created");
    let target = directory.path().join("target");
    let referent = directory.path().join("referent");
    fs::write(&referent, b"original").expect("referent should be written");
    symlink(&referent, &target).expect("target symlink should be created");

    let options = LocalWriteOptions::new(LocalWriteMode::CreateOrReplace);
    let mut writer = LocalFileSystem::host()
        .open_writer(&target, &options)
        .expect("writer should accept a target symlink entry");
    writer.write_all(b"replacement").expect("replacement should be staged");
    let outcome = writer.commit().expect("replacement should publish");
    assert_eq!(LocalWriterState::Committed, outcome.state());

    assert!(
        fs::symlink_metadata(&target)
            .expect("target metadata should be available")
            .file_type()
            .is_symlink()
    );
    assert_eq!(
        b"replacement".to_vec(),
        fs::read(&target).expect("target should contain replacement"),
    );
    assert_eq!(
        b"replacement".to_vec(),
        fs::read(&referent).expect("referent should be updated"),
    );
}

/// Verifies direct append follows a final symbolic-link entry.
#[cfg(unix)]
#[test]
fn test_local_file_writer_append_follows_target_symlink() {
    use std::os::unix::fs::symlink;

    let directory = tempdir().expect("temporary directory should exist");
    let referent = directory.path().join("referent");
    let target = directory.path().join("target");
    fs::write(&referent, b"original").expect("referent should be written");
    symlink(&referent, &target).expect("target symlink should be created");

    let mut writer = LocalFileSystem::host()
        .open_writer(&target, &LocalWriteOptions::new(LocalWriteMode::Append))
        .expect("append should follow a final symlink");
    writer
        .write_all(b"-append")
        .expect("append should write to the referent");
    let outcome = writer.commit().expect("append should commit");
    assert_eq!(LocalWritePublicationMethod::DirectAppend, outcome.publication_method());
    assert_eq!(b"original-append", fs::read(&referent).unwrap().as_slice());
}

/// Verifies Windows append follows a final name-surrogate reparse point.
#[cfg(windows)]
#[test]
fn test_local_file_writer_append_follows_target_symlink_on_windows() {
    use std::io::ErrorKind;
    use std::os::windows::fs::symlink_file;

    let directory = tempdir().expect("temporary directory should be created");
    let referent = directory.path().join("referent");
    let target = directory.path().join("target");
    fs::write(&referent, b"original").expect("referent should be written");
    if let Err(error) = symlink_file(&referent, &target) {
        if error.kind() == ErrorKind::PermissionDenied {
            return;
        }
        panic!("file symlink should be created: {error}");
    }

    let mut writer = LocalFileSystem::host()
        .open_writer(&target, &LocalWriteOptions::new(LocalWriteMode::Append))
        .expect("append should follow a final file symlink");
    writer
        .write_all(b"-append")
        .expect("append should write through the link");
    let _ = writer.commit().expect("append should commit");
    assert_eq!(
        b"original-append",
        fs::read(referent).expect("referent should remain readable").as_slice(),
    );
}

/// Verifies create-new rejects an existing entry before writing.
#[test]
fn test_local_file_writer_create_new_rejects_existing_target() {
    let directory = tempdir().expect("temporary directory should be created");
    let target = directory.path().join("target");
    fs::write(&target, b"old").expect("target fixture should be written");

    let error = LocalFileSystem::host()
        .open_writer(&target, &LocalWriteOptions::new(LocalWriteMode::CreateNew))
        .expect_err("create-new must reject the existing target");

    assert_eq!(LocalFileErrorKind::AlreadyExists, error.kind());
}

/// Verifies a destination created after opening cannot be replaced by
/// create-new commit.
#[test]
fn test_local_file_writer_create_new_preserves_concurrent_target() {
    let directory = tempdir().expect("temporary directory should be created");
    let target = directory.path().join("target");
    let mut writer = LocalFileSystem::host()
        .open_writer(&target, &LocalWriteOptions::new(LocalWriteMode::CreateNew))
        .expect("create-new staging should open for an absent target");
    writer.write_all(b"staged").expect("staged bytes should be written");
    fs::write(&target, b"concurrent").expect("concurrent target should be created");

    let error = writer
        .commit()
        .expect_err("create-new commit must not replace a concurrent target");

    assert_eq!(LocalWriteFailureState::NotPublished, error.state());
    assert_eq!(
        b"concurrent",
        fs::read(&target).expect("concurrent target should remain").as_slice(),
    );
}

/// Verifies abort cleans staging without modifying the destination.
#[test]
fn test_local_file_writer_abort_keeps_original_target() {
    let directory = tempdir().expect("temporary directory should be created");
    let target = directory.path().join("target");
    fs::write(&target, b"old").expect("target fixture should be written");
    let mut writer = LocalFileSystem::host()
        .open_writer(&target, &LocalWriteOptions::new(LocalWriteMode::CreateOrReplace))
        .expect("staged writer should open");
    writer.write_all(b"new").expect("staging write should succeed");

    let outcome = writer.abort().expect("abort should clean staging");

    assert_eq!(LocalWriterState::Aborted, outcome.state());
    assert_eq!(
        b"old",
        fs::read(&target).expect("target should remain unchanged").as_slice()
    );
}

/// Verifies direct append refuses a required atomicity guarantee.
#[test]
fn test_local_file_writer_append_rejects_required_atomicity() {
    let directory = tempdir().expect("temporary directory should be created");
    let target = directory.path().join("target");
    fs::write(&target, b"old").expect("target fixture should be written");

    let error = LocalFileSystem::host()
        .open_writer(
            &target,
            &LocalWriteOptions::new(LocalWriteMode::Append).with_atomicity(LocalAtomicityRequirement::Required),
        )
        .expect_err("direct append cannot provide required atomicity");

    assert_eq!(LocalFileErrorKind::RequirementNotMet, error.kind());
}

/// Verifies preferred durability reports a downgrade after successful
/// publication while required durability reports partial success.
#[cfg(unix)]
#[test]
fn test_local_file_writer_reports_parent_sync_result() {
    use std::os::unix::fs::PermissionsExt;

    let directory = tempdir().expect("temporary directory should be created");
    for requirement in [
        LocalDurabilityRequirement::Preferred,
        LocalDurabilityRequirement::Required,
    ] {
        let parent = directory.path().join(format!("{requirement:?}"));
        fs::create_dir(&parent).expect("target parent should be created");
        let target = parent.join("target");
        let mut writer = LocalFileSystem::host()
            .open_writer(
                &target,
                &LocalWriteOptions::new(LocalWriteMode::CreateNew).with_durability(requirement),
            )
            .expect("staged writer should open before permissions change");
        writer.write_all(b"published").expect("staged bytes should be written");
        fs::set_permissions(&parent, fs::Permissions::from_mode(0o300))
            .expect("parent should reject read-only directory opens");
        match requirement {
            LocalDurabilityRequirement::Preferred => {
                let outcome = writer.commit().expect("preferred durability may downgrade");
                assert!(!outcome.durable());
            }
            LocalDurabilityRequirement::Required => {
                let error = writer
                    .commit()
                    .expect_err("required durability must report sync failure");
                assert_eq!(LocalWriteFailureState::Published, error.state());
                assert_eq!(LocalFileErrorKind::PublicationIncomplete, error.error().kind(),);
            }
            LocalDurabilityRequirement::NotRequired => unreachable!(),
        }
        fs::set_permissions(&parent, fs::Permissions::from_mode(0o700)).expect("parent permissions should be restored");
        assert_eq!(
            b"published",
            fs::read(&target).expect("published target should remain").as_slice(),
        );
    }
}

/// Verifies a stream error permanently prevents append commit or clean abort.
#[cfg(unix)]
#[test]
fn test_local_file_writer_append_preserves_indeterminate_state() {
    if let Ok(case) = env::var(INDETERMINATE_APPEND_CASE) {
        run_indeterminate_append_case(&case);
        return;
    }
    let executable = env::current_exe().expect("current test executable should resolve");
    for case in ["commit", "abort"] {
        let status = Command::new(&executable)
            .arg("--exact")
            .arg("test_local_file_writer_append_preserves_indeterminate_state")
            .arg("--nocapture")
            .env(INDETERMINATE_APPEND_CASE, case)
            .status()
            .expect("indeterminate append child should start");
        assert!(status.success(), "indeterminate append {case} child should succeed");
    }
}

/// Runs one append state transition under a zero-byte process file-size limit.
#[cfg(unix)]
fn run_indeterminate_append_case(case: &str) {
    let directory = tempdir().expect("temporary directory should be created");
    let target = directory.path().join("target");
    fs::write(&target, b"existing").expect("target fixture should be written");
    let mut original_limit = libc::rlimit {
        rlim_cur: 0,
        rlim_max: 0,
    };
    // SAFETY: the child process owns its resource limit. Ignoring SIGXFSZ
    // converts the temporary zero-byte limit into an ordinary write error.
    unsafe {
        libc::signal(libc::SIGXFSZ, libc::SIG_IGN);
        assert_eq!(
            0,
            libc::getrlimit(libc::RLIMIT_FSIZE, &raw mut original_limit),
            "child process file-size limit should be readable",
        );
        let limit = libc::rlimit {
            rlim_cur: 0,
            rlim_max: original_limit.rlim_max,
        };
        assert_eq!(
            0,
            libc::setrlimit(libc::RLIMIT_FSIZE, &raw const limit),
            "child process file-size limit should be installed",
        );
    }
    let mut writer = LocalFileSystem::host()
        .open_writer(&target, &LocalWriteOptions::new(LocalWriteMode::Append))
        .expect("append writer should open before the failing write");
    writer
        .write_all(b"x")
        .expect_err("zero file-size limit should reject append");
    let failure_state =
        std::hint::black_box(LocalFileWriter::failure_state as fn(&LocalFileWriter) -> Option<LocalWriteFailureState>);
    assert_eq!(
        Some(LocalWriteFailureState::Indeterminate),
        std::hint::black_box(failure_state)(&writer),
    );
    let write_after_failure = writer
        .write(b"x")
        .expect_err("indeterminate writer must reject further writes");
    assert_eq!(std::io::ErrorKind::BrokenPipe, write_after_failure.kind());
    let vectored_after_failure = writer
        .write_vectored(&[IoSlice::new(b"x")])
        .expect_err("indeterminate writer must reject further vectored writes");
    assert_eq!(std::io::ErrorKind::BrokenPipe, vectored_after_failure.kind());
    let flush_after_failure = writer
        .flush()
        .expect_err("indeterminate writer must reject further flushes");
    assert_eq!(std::io::ErrorKind::BrokenPipe, flush_after_failure.kind());
    match case {
        "commit" => {
            let error = writer.commit().expect_err("indeterminate writer must not commit");
            assert_eq!(LocalWriteFailureState::Indeterminate, error.state());
        }
        "abort" => {
            let outcome = writer
                .abort()
                .expect("abort should close an indeterminate append writer");
            assert_eq!(LocalWriterState::Aborted, outcome.state());
            assert_eq!(Some(LocalWriteFailureState::Indeterminate), outcome.failure_state(),);
        }
        other => panic!("unexpected append regression case: {other}"),
    }
    // SAFETY: restoring the saved soft limit lets coverage-instrumented child
    // processes flush their profile after the regression assertions complete.
    unsafe {
        assert_eq!(
            0,
            libc::setrlimit(libc::RLIMIT_FSIZE, &raw const original_limit),
            "child process file-size limit should be restored",
        );
    }
}
