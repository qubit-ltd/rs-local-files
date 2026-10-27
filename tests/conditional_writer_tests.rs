// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

#![cfg(feature = "digest")]

mod support;

use digest::FixedOutput;
use digest::HashMarker;
use digest::OutputSizeUser;
use digest::Update;
use digest::consts::U32;
use qubit_io::Output;
use qubit_local_files::LocalFileSystem;
use qubit_local_files::error::LocalFileErrorKind;
#[cfg(any(unix, windows))]
use qubit_local_files::error::LocalFileOperation;
#[cfg(windows)]
use qubit_local_files::options::LocalWriteMetadataPolicy;
use qubit_local_files::options::LocalWriteMode;
use qubit_local_files::options::LocalWriteOptions;
use qubit_local_files::options::LocalWritePrecondition;
use sha2::Digest;
use sha2::Sha256;
use sha2::Sha512;
use support::conditional_temp_directory::tempdir;

struct NoCloneNoDebugSha256(Sha256);
impl Default for NoCloneNoDebugSha256 {
    fn default() -> Self {
        Self(Sha256::new())
    }
}
impl HashMarker for NoCloneNoDebugSha256 {}
impl OutputSizeUser for NoCloneNoDebugSha256 {
    type OutputSize = U32;
}
impl Update for NoCloneNoDebugSha256 {
    fn update(&mut self, data: &[u8]) {
        Update::update(&mut self.0, data);
    }
}
impl FixedOutput for NoCloneNoDebugSha256 {
    fn finalize_into(self, out: &mut digest::Output<Self>) {
        out.copy_from_slice(&self.0.finalize());
    }
}

#[test]
fn absent_save_returns_actual_digest() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("model");
    let mut fs = LocalFileSystem::host().unwrap();
    fs.set_symlink_policy(qubit_local_files::policy::LocalSymlinkPolicy::Reject)
        .unwrap();
    let mut writer = fs
        .open_conditional_writer::<Sha256>(
            &path,
            LocalWritePrecondition::Absent,
            &LocalWriteOptions::new(LocalWriteMode::CreateOrReplace),
        )
        .unwrap();
    writer.write_fully(b"B").unwrap();
    let saved = writer.commit().unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), b"B");
    assert_eq!(saved.digest(), &Sha256::digest(b"B"));
    assert!(saved.cleanup_errors().is_empty());
    assert!(saved.retained_files().is_empty());
}

#[test]
fn opening_host_conditional_writer_reports_missing_parent_context() {
    let directory = tempdir().expect("fixture directory should be created");
    let target = directory.path().join("missing-parent").join("target");
    let mut filesystem = LocalFileSystem::host().expect("Host filesystem should open");
    filesystem
        .set_symlink_policy(qubit_local_files::policy::LocalSymlinkPolicy::Reject)
        .expect("conditional writes should use a non-following policy");

    let error = filesystem
        .open_conditional_writer::<Sha256>(
            &target,
            LocalWritePrecondition::Absent,
            &LocalWriteOptions::new(LocalWriteMode::CreateNew),
        )
        .expect_err("a missing parent should prevent writer creation");

    assert_eq!(std::io::ErrorKind::NotFound, error.io_error_kind());
    assert_eq!(Some(target.as_path()), error.path());
}

#[test]
#[cfg(unix)]
fn opening_host_conditional_writer_rejects_intermediate_symlink() {
    let directory = tempdir().expect("fixture directory should be created");
    let real = directory.path().join("real");
    std::fs::create_dir(&real).expect("real parent should be created");
    std::fs::write(real.join("existing"), b"original").expect("existing target should be written");
    std::os::unix::fs::symlink(&real, directory.path().join("link")).expect("parent symlink should be created");
    let mut filesystem = LocalFileSystem::host().expect("Host filesystem should open");
    filesystem
        .set_symlink_policy(qubit_local_files::policy::LocalSymlinkPolicy::Reject)
        .expect("conditional writes should use a non-following policy");

    for (name, precondition) in [
        ("absent", LocalWritePrecondition::Absent),
        ("existing", LocalWritePrecondition::Content(Sha256::digest(b"original"))),
    ] {
        let target = directory.path().join("link").join(name);
        let error = filesystem
            .open_conditional_writer::<Sha256>(
                &target,
                precondition,
                &LocalWriteOptions::new(LocalWriteMode::CreateOrReplace),
            )
            .expect_err("an intermediate symlink must be rejected before staging");

        assert_eq!(LocalFileErrorKind::Unsupported, error.kind());
        assert_eq!(LocalFileOperation::OpenWriter, error.operation());
        assert_eq!(Some(target.as_path()), error.path());
        assert_eq!(
            1,
            std::fs::read_dir(&real)
                .expect("real parent should be readable")
                .count()
        );
        assert_eq!(
            b"original",
            std::fs::read(real.join("existing"))
                .expect("target should be readable")
                .as_slice()
        );
    }
}

#[test]
#[cfg(unix)]
fn opening_host_conditional_writer_rejects_symlink_before_creating_missing_parent() {
    let directory = tempdir().expect("fixture directory should be created");
    let real = directory.path().join("real");
    std::fs::create_dir(&real).expect("real parent should be created");
    std::os::unix::fs::symlink(&real, directory.path().join("link")).expect("parent symlink should be created");
    let target = directory.path().join("link/missing/target");
    let mut filesystem = LocalFileSystem::host().expect("Host filesystem should open");
    filesystem
        .set_symlink_policy(qubit_local_files::policy::LocalSymlinkPolicy::Reject)
        .expect("conditional writes should use a non-following policy");

    let error = filesystem
        .open_conditional_writer::<Sha256>(
            &target,
            LocalWritePrecondition::Absent,
            &LocalWriteOptions::new(LocalWriteMode::CreateNew).with_create_parent(),
        )
        .expect_err("symlink rejection must precede parent creation");

    assert_eq!(LocalFileErrorKind::Unsupported, error.kind());
    assert_eq!(LocalFileOperation::OpenWriter, error.operation());
    assert_eq!(Some(target.as_path()), error.path());
    assert_eq!(
        0,
        std::fs::read_dir(&real)
            .expect("real parent should be readable")
            .count()
    );
}

#[test]
#[cfg(unix)]
fn opening_rooted_conditional_writer_reports_non_directory_parent() {
    let directory = tempdir().expect("fixture directory should be created");
    std::fs::write(directory.path().join("blocker"), b"file").expect("parent blocker should be written");
    let mut filesystem = LocalFileSystem::rooted(directory.path()).expect("Rooted filesystem should open");
    filesystem
        .set_symlink_policy(qubit_local_files::policy::LocalSymlinkPolicy::Reject)
        .expect("conditional writes should use a non-following policy");
    let target = std::path::Path::new("/blocker/target");

    let error = filesystem
        .open_conditional_writer::<Sha256>(
            target,
            LocalWritePrecondition::Absent,
            &LocalWriteOptions::new(LocalWriteMode::CreateNew),
        )
        .expect_err("a file cannot serve as a rooted parent directory");

    assert_eq!(std::io::ErrorKind::InvalidInput, error.io_error_kind());
    assert_eq!(Some(target), error.path());
}

#[test]
fn writer_exposes_paths_debug_state_and_all_output_entry_points() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("output-contract");
    let mut fs = LocalFileSystem::host().unwrap();
    fs.set_symlink_policy(qubit_local_files::policy::LocalSymlinkPolicy::Reject)
        .unwrap();
    let mut writer = fs
        .open_conditional_writer::<Sha256>(
            &path,
            LocalWritePrecondition::Absent,
            &LocalWriteOptions::new(LocalWriteMode::CreateNew),
        )
        .unwrap();

    assert_eq!(writer.path(), path.as_path());
    assert_eq!(writer.diagnostic_path(), Some(path.as_path()));
    assert_eq!(writer.state(), qubit_local_files::outcome::LocalWriterState::Open);
    assert!(format!("{writer:?}").contains("LocalConditionalWriter"));
    assert_eq!(Output::write(&mut writer, b"A").unwrap(), 1);
    let vectored = Output::write_vectored(&mut writer, &[b"B", b"C"]).unwrap();
    assert!((1..=2).contains(&vectored));
    if vectored < 2 {
        writer.write_fully(b"C").unwrap();
    }
    // SAFETY: index 1 and count 2 select the valid `xy` slice.
    assert_eq!(
        unsafe { Output::write_unchecked(&mut writer, b"x y", 1, 2) }.unwrap(),
        2
    );
    Output::flush(&mut writer).unwrap();

    let saved = writer.commit().unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), b"ABC y");
    assert_eq!(saved.write_outcome().bytes_written(), 5);
}

#[test]
#[cfg(unix)]
fn content_precondition_replaces_rooted_file_and_returns_digest() {
    let dir = tempdir().unwrap();
    let native_path = dir.path().join("rooted-model");
    std::fs::write(&native_path, b"old").unwrap();
    let mut fs = LocalFileSystem::rooted(dir.path()).unwrap();
    fs.set_symlink_policy(qubit_local_files::policy::LocalSymlinkPolicy::Reject)
        .unwrap();
    let mut writer = fs
        .open_conditional_writer::<Sha256>(
            std::path::Path::new("/rooted-model"),
            LocalWritePrecondition::Content(Sha256::digest(b"old")),
            &LocalWriteOptions::new(LocalWriteMode::CreateOrReplace),
        )
        .unwrap();
    writer.write_fully(b"new").unwrap();
    let saved = writer.commit().unwrap();
    assert_eq!(std::fs::read(native_path).unwrap(), b"new");
    assert_eq!(saved.digest(), &Sha256::digest(b"new"));
}

#[test]
#[cfg(windows)]
fn windows_open_rejects_rooted_content_without_artifacts() {
    let dir = tempdir().unwrap();
    let mut fs = LocalFileSystem::rooted(dir.path()).unwrap();
    fs.set_symlink_policy(qubit_local_files::policy::LocalSymlinkPolicy::Reject)
        .unwrap();
    for (parent, create_parent) in [("missing-parent", false), ("created-parent", true)] {
        let target = std::path::PathBuf::from(format!("/{parent}/target"));
        let options = LocalWriteOptions::new(LocalWriteMode::CreateOrReplace);
        let options = if create_parent {
            options.with_create_parent()
        } else {
            options
        };

        let error = fs
            .open_conditional_writer::<Sha256>(
                &target,
                LocalWritePrecondition::Content(Sha256::digest(b"old")),
                &options,
            )
            .expect_err("Rooted Content must be rejected while opening");

        assert_eq!(LocalFileErrorKind::Unsupported, error.kind());
        assert_eq!(LocalFileOperation::OpenWriter, error.operation());
        assert_eq!(Some(target.as_path()), error.path());
        assert_eq!(
            Some("this Windows conditional Content configuration is unsupported"),
            error.reason()
        );
        assert!(!dir.path().join(parent).exists(), "the parent must not be created");
        assert_eq!(
            0,
            std::fs::read_dir(dir.path()).unwrap().count(),
            "no destination or staging artifact may be created",
        );
    }
}

#[test]
#[cfg(windows)]
fn windows_open_rejects_host_content_use_staging_without_artifacts() {
    let dir = tempdir().unwrap();
    let mut fs = LocalFileSystem::host().unwrap();
    fs.set_symlink_policy(qubit_local_files::policy::LocalSymlinkPolicy::Reject)
        .unwrap();
    for (parent, create_parent) in [("missing-parent", false), ("created-parent", true)] {
        let missing_parent = dir.path().join(parent);
        let target = missing_parent.join("target");
        let options = LocalWriteOptions::new(LocalWriteMode::CreateOrReplace)
            .with_metadata_policy(LocalWriteMetadataPolicy::UseStaging);
        let options = if create_parent {
            options.with_create_parent()
        } else {
            options
        };

        let error = fs
            .open_conditional_writer::<Sha256>(
                &target,
                LocalWritePrecondition::Content(Sha256::digest(b"old")),
                &options,
            )
            .expect_err("Host Content with UseStaging must be rejected while opening");

        assert_eq!(LocalFileErrorKind::Unsupported, error.kind());
        assert_eq!(LocalFileOperation::OpenWriter, error.operation());
        assert_eq!(Some(target.as_path()), error.path());
        assert_eq!(
            Some("this Windows conditional Content configuration is unsupported"),
            error.reason()
        );
        assert!(!missing_parent.exists(), "the parent must not be created");
        assert_eq!(
            0,
            std::fs::read_dir(dir.path()).unwrap().count(),
            "no destination or staging artifact may be created",
        );
    }
}

#[test]
#[cfg(windows)]
fn windows_open_accepts_absent_condition() {
    let dir = tempdir().unwrap();
    let mut fs = LocalFileSystem::host().unwrap();
    fs.set_symlink_policy(qubit_local_files::policy::LocalSymlinkPolicy::Reject)
        .unwrap();
    let target = dir.path().join("absent");
    let writer = fs
        .open_conditional_writer::<Sha256>(
            &target,
            LocalWritePrecondition::Absent,
            &LocalWriteOptions::new(LocalWriteMode::CreateNew),
        )
        .expect("Host Absent must remain supported");
    drop(writer);
    assert!(!target.exists());

    let mut rooted = LocalFileSystem::rooted(dir.path()).unwrap();
    rooted
        .set_symlink_policy(qubit_local_files::policy::LocalSymlinkPolicy::Reject)
        .unwrap();
    let rooted_target = std::path::Path::new("/rooted-absent");
    let writer = rooted
        .open_conditional_writer::<Sha256>(
            rooted_target,
            LocalWritePrecondition::Absent,
            &LocalWriteOptions::new(LocalWriteMode::CreateNew),
        )
        .expect("Rooted Absent must remain supported");
    drop(writer);
    assert!(!dir.path().join("rooted-absent").exists());
}

#[test]
#[cfg(windows)]
fn windows_open_accepts_host_preserve_existing_content() {
    let dir = tempdir().unwrap();
    let mut fs = LocalFileSystem::host().unwrap();
    fs.set_symlink_policy(qubit_local_files::policy::LocalSymlinkPolicy::Reject)
        .unwrap();
    let target = dir.path().join("content");
    std::fs::write(&target, b"old").unwrap();
    let mut writer = fs
        .open_conditional_writer::<Sha256>(
            &target,
            LocalWritePrecondition::Content(Sha256::digest(b"old")),
            &LocalWriteOptions::new(LocalWriteMode::CreateOrReplace)
                .with_metadata_policy(LocalWriteMetadataPolicy::PreserveExisting),
        )
        .expect("Host Content with PreserveExisting must remain supported");
    writer.write_fully(b"new").unwrap();
    let saved = writer.commit().unwrap();
    assert_eq!(b"new", std::fs::read(target).unwrap().as_slice());
    assert_eq!(saved.digest(), &Sha256::digest(b"new"));
}

#[test]
fn append_and_follow_policy_are_rejected_before_open() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("existing");
    std::fs::write(&path, b"unchanged").unwrap();
    let mut fs = LocalFileSystem::host().unwrap();
    fs.set_symlink_policy(qubit_local_files::policy::LocalSymlinkPolicy::Reject)
        .unwrap();
    assert!(
        fs.open_conditional_writer::<Sha256>(
            &path,
            LocalWritePrecondition::Absent,
            &LocalWriteOptions::new(LocalWriteMode::Append),
        )
        .is_err()
    );
    assert_eq!(std::fs::read(&path).unwrap(), b"unchanged");
    fs.set_symlink_policy(qubit_local_files::policy::LocalSymlinkPolicy::FollowAcrossScope)
        .unwrap();
    assert!(
        fs.open_conditional_writer::<Sha256>(
            &path,
            LocalWritePrecondition::Absent,
            &LocalWriteOptions::new(LocalWriteMode::CreateNew),
        )
        .is_err()
    );
    assert_eq!(std::fs::read(&path).unwrap(), b"unchanged");
    fs.set_symlink_policy(qubit_local_files::policy::LocalSymlinkPolicy::FollowWithinScope)
        .unwrap();
    assert!(
        fs.open_conditional_writer::<Sha256>(
            &path,
            LocalWritePrecondition::Absent,
            &LocalWriteOptions::new(LocalWriteMode::CreateNew),
        )
        .is_err()
    );
    assert_eq!(std::fs::read(&path).unwrap(), b"unchanged");
}

#[test]
fn dropping_uncommitted_writer_does_not_publish_candidate() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("not-published");
    let mut fs = LocalFileSystem::host().unwrap();
    fs.set_symlink_policy(qubit_local_files::policy::LocalSymlinkPolicy::Reject)
        .unwrap();
    let mut writer = fs
        .open_conditional_writer::<Sha256>(
            &path,
            LocalWritePrecondition::Absent,
            &LocalWriteOptions::new(LocalWriteMode::CreateNew),
        )
        .unwrap();
    writer.write_fully(b"candidate").unwrap();
    drop(writer);
    assert!(!path.exists());
}

#[test]
fn commit_after_abort_reports_invalid_state() {
    let dir = tempdir().unwrap();
    let target = dir.path().join("aborted");
    std::fs::write(&target, b"original").expect("original target should be written");
    let mut fs = LocalFileSystem::host().unwrap();
    fs.set_symlink_policy(qubit_local_files::policy::LocalSymlinkPolicy::Reject)
        .unwrap();
    let mut writer = fs
        .open_conditional_writer::<Sha256>(
            &target,
            LocalWritePrecondition::Content(Sha256::digest(b"original")),
            &LocalWriteOptions::new(LocalWriteMode::CreateOrReplace),
        )
        .unwrap();
    assert_eq!(
        writer.abort().unwrap().state(),
        qubit_local_files::outcome::LocalWriterState::Aborted
    );
    let error = match writer.commit() {
        Ok(_) => panic!("commit after abort must fail"),
        Err(error) => error,
    };
    assert_eq!(error.error().kind(), LocalFileErrorKind::InvalidState);
    assert_eq!(
        error.reason(),
        qubit_local_files::error::LocalConditionalFailureReason::InvalidState
    );
    assert_eq!(
        error.stage(),
        qubit_local_files::error::LocalConditionalWriteStage::Prepare
    );
    assert_eq!(
        error.publication(),
        qubit_local_files::outcome::LocalConditionalPublicationState::NotPublished
    );
    assert_eq!(std::fs::read(target).unwrap(), b"original");
}

#[test]
fn commit_after_abort_failure_reports_invalid_state() {
    let dir = tempdir().expect("fixture should be created");
    let target = dir.path().join("abort-failed");
    std::fs::write(&target, b"original").expect("original target should be written");
    let mut filesystem = LocalFileSystem::host().expect("Host filesystem should open");
    filesystem
        .set_symlink_policy(qubit_local_files::policy::LocalSymlinkPolicy::Reject)
        .expect("conditional writes should use a non-following policy");
    let before: std::collections::HashSet<_> = std::fs::read_dir(dir.path())
        .expect("fixture should be readable")
        .map(|entry| entry.expect("fixture entry should be readable").path())
        .collect();
    let mut writer = filesystem
        .open_conditional_writer::<Sha256>(
            &target,
            LocalWritePrecondition::Content(Sha256::digest(b"original")),
            &LocalWriteOptions::new(LocalWriteMode::CreateOrReplace),
        )
        .expect("conditional writer should open");
    writer.write_fully(b"candidate").expect("candidate should be written");
    let staged: Vec<_> = std::fs::read_dir(dir.path())
        .expect("fixture should be readable")
        .map(|entry| entry.expect("fixture entry should be readable").path())
        .filter(|path| !before.contains(path))
        .collect();
    assert_eq!(staged.len(), 1, "one private staging entry should be created");
    std::fs::remove_file(&staged[0]).expect("staging entry should be externally removed");
    assert!(writer.abort().is_err(), "abort should report the missing staged entry");

    let error = writer.commit().expect_err("commit after abort failure must fail");
    assert_eq!(error.error().kind(), LocalFileErrorKind::InvalidState);
    assert_eq!(
        error.reason(),
        qubit_local_files::error::LocalConditionalFailureReason::InvalidState
    );
    assert_eq!(
        error.stage(),
        qubit_local_files::error::LocalConditionalWriteStage::Prepare
    );
    assert_eq!(
        error.publication(),
        qubit_local_files::outcome::LocalConditionalPublicationState::NotPublished
    );
    assert_eq!(std::fs::read(target).unwrap(), b"original");
}

#[cfg(feature = "test-support")]
#[test]
fn commit_after_permanent_write_failure_reports_invalid_state() {
    use qubit_local_files::test_support::TestConditionalHooks;
    use qubit_local_files::test_support::TestConditionalPoint;

    let dir = tempdir().expect("fixture should be created");
    let target = dir.path().join("write-failed");
    std::fs::write(&target, b"original").expect("original target should be written");
    let mut filesystem = LocalFileSystem::host().expect("Host filesystem should open");
    filesystem
        .set_symlink_policy(qubit_local_files::policy::LocalSymlinkPolicy::Reject)
        .expect("conditional writes should use a non-following policy");
    let filesystem = filesystem
        .with_conditional_test_hooks(TestConditionalHooks::once(TestConditionalPoint::BeforeWrite, || {
            Err(std::io::Error::other("injected permanent write failure"))
        }));
    let mut writer = filesystem
        .open_conditional_writer::<Sha256>(
            &target,
            LocalWritePrecondition::Content(Sha256::digest(b"original")),
            &LocalWriteOptions::new(LocalWriteMode::CreateOrReplace),
        )
        .expect("conditional writer should open");

    let write_error = writer
        .write_fully(b"candidate")
        .expect_err("injected permanent write failure should reach the caller");
    assert_eq!(write_error.kind(), std::io::ErrorKind::Other);
    let error = writer
        .commit()
        .expect_err("commit after permanent write failure must fail");
    assert_eq!(error.error().kind(), LocalFileErrorKind::InvalidState);
    assert_eq!(
        error.reason(),
        qubit_local_files::error::LocalConditionalFailureReason::InvalidState
    );
    assert_eq!(
        error.stage(),
        qubit_local_files::error::LocalConditionalWriteStage::Prepare
    );
    assert_eq!(
        error.publication(),
        qubit_local_files::outcome::LocalConditionalPublicationState::NotPublished
    );
    assert_eq!(std::fs::read(target).unwrap(), b"original");
}

#[test]
fn empty_and_large_candidates_return_sha512_digest() {
    let dir = tempdir().unwrap();
    let mut fs = LocalFileSystem::host().unwrap();
    fs.set_symlink_policy(qubit_local_files::policy::LocalSymlinkPolicy::Reject)
        .unwrap();
    let empty_path = dir.path().join("empty");
    let empty = fs
        .open_conditional_writer::<Sha512>(
            &empty_path,
            LocalWritePrecondition::Absent,
            &LocalWriteOptions::new(LocalWriteMode::CreateNew),
        )
        .unwrap()
        .commit()
        .unwrap();
    assert_eq!(std::fs::read(&empty_path).unwrap(), b"");
    assert_eq!(empty.digest(), &Sha512::digest(b""));

    let large_path = dir.path().join("large");
    let content = vec![0x5a; 1024 * 1024];
    let mut writer = fs
        .open_conditional_writer::<Sha512>(
            &large_path,
            LocalWritePrecondition::Absent,
            &LocalWriteOptions::new(LocalWriteMode::CreateNew),
        )
        .unwrap();
    writer.write_fully(&content).unwrap();
    let saved = writer.commit().unwrap();
    assert_eq!(saved.digest(), &Sha512::digest(&content));
    assert_eq!(saved.write_outcome().bytes_written(), content.len());
}

#[test]
fn no_clone_no_debug_digest_works_through_public_api() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("custom-digest");
    let mut fs = LocalFileSystem::host().unwrap();
    fs.set_symlink_policy(qubit_local_files::policy::LocalSymlinkPolicy::Reject)
        .unwrap();
    let mut writer = fs
        .open_conditional_writer::<NoCloneNoDebugSha256>(
            &path,
            LocalWritePrecondition::Absent,
            &LocalWriteOptions::new(LocalWriteMode::CreateNew),
        )
        .unwrap();
    writer.write_fully(b"typed").unwrap();
    let saved = writer.commit().unwrap();
    assert_eq!(saved.digest().as_slice(), Sha256::digest(b"typed").as_slice());
}

#[test]
fn conditional_commit_error_debug_and_error_do_not_require_digest_debug() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("missing-target");
    let mut fs = LocalFileSystem::host().unwrap();
    fs.set_symlink_policy(qubit_local_files::policy::LocalSymlinkPolicy::Reject)
        .unwrap();
    let mut writer = fs
        .open_conditional_writer::<NoCloneNoDebugSha256>(
            &path,
            LocalWritePrecondition::Content(NoCloneNoDebugSha256::digest(b"expected")),
            &LocalWriteOptions::new(LocalWriteMode::CreateOrReplace),
        )
        .unwrap();
    writer.write_fully(b"candidate").unwrap();
    let error = match writer.commit() {
        Ok(_) => panic!("Content condition for a missing target must fail"),
        Err(error) => error,
    };
    let _: &dyn std::error::Error = &error;
    let _debug = format!("{error:?}");
}

#[test]
fn content_precondition_rejects_create_new_before_open() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("existing");
    std::fs::write(&path, b"old").unwrap();
    let mut fs = LocalFileSystem::host().unwrap();
    fs.set_symlink_policy(qubit_local_files::policy::LocalSymlinkPolicy::Reject)
        .unwrap();
    assert!(
        fs.open_conditional_writer::<Sha256>(
            &path,
            LocalWritePrecondition::Content(Sha256::digest(b"old")),
            &LocalWriteOptions::new(LocalWriteMode::CreateNew),
        )
        .is_err()
    );
    assert_eq!(std::fs::read(path).unwrap(), b"old");
}

#[test]
fn content_commit_failure_reports_host_target_path() {
    let dir = tempdir().unwrap();
    let missing = dir.path().join("missing");
    let mismatched = dir.path().join("mismatched");
    std::fs::write(&mismatched, b"current").unwrap();
    let mut fs = LocalFileSystem::host().unwrap();
    fs.set_symlink_policy(qubit_local_files::policy::LocalSymlinkPolicy::Reject)
        .unwrap();

    for (path, expected) in [
        (&missing, b"expected".as_slice()),
        (&mismatched, b"expected".as_slice()),
    ] {
        let mut writer = fs
            .open_conditional_writer::<Sha256>(
                path,
                LocalWritePrecondition::Content(Sha256::digest(expected)),
                &LocalWriteOptions::new(LocalWriteMode::CreateOrReplace),
            )
            .unwrap();
        writer.write_fully(b"candidate").unwrap();
        let error = match writer.commit() {
            Ok(_) => panic!("invalid Content precondition must fail"),
            Err(error) => error,
        };
        assert_eq!(error.error().path(), Some(path.as_path()));
        assert!(std::error::Error::source(error.error()).is_some());
    }
}

#[test]
#[cfg(not(windows))]
fn rooted_content_commit_failure_reports_namespace_path_only() {
    let dir = tempdir().unwrap();
    let native_path = dir.path().join("model");
    std::fs::write(&native_path, b"current").unwrap();
    let namespace_path = std::path::Path::new("/model");
    let mut fs = LocalFileSystem::rooted(dir.path()).unwrap();
    fs.set_symlink_policy(qubit_local_files::policy::LocalSymlinkPolicy::Reject)
        .unwrap();
    let mut writer = fs
        .open_conditional_writer::<Sha256>(
            namespace_path,
            LocalWritePrecondition::Content(Sha256::digest(b"expected")),
            &LocalWriteOptions::new(LocalWriteMode::CreateOrReplace),
        )
        .unwrap();
    writer.write_fully(b"candidate").unwrap();
    let error = match writer.commit() {
        Ok(_) => panic!("mismatched Content precondition must fail"),
        Err(error) => error,
    };
    assert_eq!(error.error().path(), Some(namespace_path));
    assert!(!error.error().path().unwrap().starts_with(dir.path()));
    assert!(std::error::Error::source(error.error()).is_some());
}
