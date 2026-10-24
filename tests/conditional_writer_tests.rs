// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

#![cfg(feature = "digest")]

use digest::FixedOutput;
use digest::HashMarker;
use digest::OutputSizeUser;
use digest::Update;
use digest::consts::U32;
use qubit_io::Output;
use qubit_local_files::LocalFileSystem;
use qubit_local_files::options::LocalWriteMode;
use qubit_local_files::options::LocalWriteOptions;
use qubit_local_files::options::LocalWritePrecondition;
use sha2::Digest;
use sha2::Sha256;
use sha2::Sha512;

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
    let dir = tempfile::tempdir().unwrap();
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
    let directory = tempfile::tempdir().expect("fixture directory should be created");
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
fn opening_rooted_conditional_writer_reports_non_directory_parent() {
    let directory = tempfile::tempdir().expect("fixture directory should be created");
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
    let dir = tempfile::tempdir().unwrap();
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
    let dir = tempfile::tempdir().unwrap();
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
fn rooted_content_precondition_is_unsupported_and_retains_candidate() {
    let dir = tempfile::tempdir().unwrap();
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

    let failure = writer.commit().expect_err("Windows Rooted Content is unsupported");

    assert_eq!(std::fs::read(native_path).unwrap(), b"old");
    assert_eq!(
        failure.publication(),
        qubit_local_files::outcome::LocalConditionalPublicationState::NotPublished
    );
    assert!(
        failure
            .error()
            .to_string()
            .contains("Windows Rooted conditional Content is unsupported")
    );
    assert!(failure.candidate().is_some());
}

#[test]
fn append_and_follow_policy_are_rejected_before_open() {
    let dir = tempfile::tempdir().unwrap();
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
    let dir = tempfile::tempdir().unwrap();
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
    let dir = tempfile::tempdir().unwrap();
    let mut fs = LocalFileSystem::host().unwrap();
    fs.set_symlink_policy(qubit_local_files::policy::LocalSymlinkPolicy::Reject)
        .unwrap();
    let mut writer = fs
        .open_conditional_writer::<Sha256>(
            &dir.path().join("aborted"),
            LocalWritePrecondition::Absent,
            &LocalWriteOptions::new(LocalWriteMode::CreateNew),
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
    assert_eq!(format!("{:?}", error.error().kind()), "InvalidState");
}

#[test]
fn empty_and_large_candidates_return_sha512_digest() {
    let dir = tempfile::tempdir().unwrap();
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
    let dir = tempfile::tempdir().unwrap();
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
    let dir = tempfile::tempdir().unwrap();
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
    let dir = tempfile::tempdir().unwrap();
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
    let dir = tempfile::tempdir().unwrap();
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
fn rooted_content_commit_failure_reports_namespace_path_only() {
    let dir = tempfile::tempdir().unwrap();
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
