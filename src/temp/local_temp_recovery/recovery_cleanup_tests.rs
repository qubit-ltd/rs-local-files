// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::fs::File;
use std::os::unix::fs::MetadataExt;
use std::path::PathBuf;

use super::LocalTempRecovery;
use super::NativePrivateEntry;
use crate::LocalFileErrorKind;
use crate::observation::LocalFileIdentity;

fn entry(parent: &File, name: &str, file: Option<File>, identity: Option<LocalFileIdentity>) -> NativePrivateEntry {
    NativePrivateEntry {
        parent: parent.try_clone().expect("clone parent directory"),
        name: name.into(),
        file,
        identity,
        directory: false,
    }
}

fn identity(file: &File) -> LocalFileIdentity {
    let metadata = file.metadata().expect("entry metadata");
    LocalFileIdentity::unix(metadata.dev(), metadata.ino())
}

#[test]
fn test_review_recovery_open_failure_retains_cleanup_responsibility() {
    let fixture = tempfile::tempdir().expect("temporary directory");
    let parent = File::open(fixture.path()).expect("open parent directory");
    let error = crate::temp::internal::privacy_unix::create_private_directory_at_with_open_failure_hook(
        &parent,
        std::ffi::OsStr::new("residual"),
        || Err(std::io::Error::other("injected directory open failure")),
    )
    .expect_err("injected open failure");
    let retained = error.retained.expect("retained cleanup entry");
    assert!(retained.file.is_none());
    assert!(retained.identity.is_none());
    let path = fixture.path().join("residual");
    assert!(path.is_dir(), "mkdir succeeded before open failure");

    let mut recovery = LocalTempRecovery::from_native(path.clone(), retained);
    let cleanup_error = recovery.cleanup().expect_err("unknown identity must be rejected");
    assert_eq!(cleanup_error.kind(), LocalFileErrorKind::RequirementNotMet);
    assert_eq!(cleanup_error.path(), Some(path.as_path()));
    assert!(path.is_dir(), "cleanup must not delete by name");
    assert_eq!(
        recovery.cleanup().expect_err("responsibility remains retained").kind(),
        LocalFileErrorKind::RequirementNotMet
    );
    drop(recovery);
    assert!(path.is_dir(), "drop must not delete an entry with unknown identity");
}

#[test]
fn test_review_recovery_releases_only_successful_prefix() {
    let fixture = tempfile::tempdir().expect("temporary directory");
    let parent = File::open(fixture.path()).expect("open parent directory");
    let first = crate::temp::internal::privacy_unix::create_private_file_at(&parent, std::ffi::OsStr::new("first"))
        .expect("create first file");
    let first_identity = identity(&first);
    let path = fixture.path().join("recovery");
    let mut recovery = LocalTempRecovery::from_native_entries(
        path.clone(),
        vec![
            entry(&parent, "first", Some(first), Some(first_identity)),
            entry(&parent, "unknown", None, None),
        ],
    );

    let error = recovery.cleanup().expect_err("second entry has no identity");
    assert_eq!(error.kind(), LocalFileErrorKind::RequirementNotMet);
    assert!(!fixture.path().join("first").exists());
    assert!(recovery.cleanup().is_err(), "the retained suffix still fails");
    assert!(
        !fixture.path().join("first").exists(),
        "successfully removed prefix is not retried"
    );
}

#[test]
fn test_review_recovery_refuses_replaced_object_and_retries_remaining_entries() {
    let fixture = tempfile::tempdir().expect("temporary directory");
    let parent = File::open(fixture.path()).expect("open parent directory");
    let original =
        crate::temp::internal::privacy_unix::create_private_file_at(&parent, std::ffi::OsStr::new("replaced"))
            .expect("create original file");
    let original_identity = identity(&original);
    std::fs::remove_file(fixture.path().join("replaced")).expect("remove original directory entry");
    std::fs::write(fixture.path().join("replaced"), b"replacement").expect("create replacement");
    let replacement = fixture.path().join("replaced");
    let replacement_identity = {
        let file = File::open(&replacement).expect("open replacement");
        identity(&file)
    };
    assert_ne!(original_identity, replacement_identity);
    let path = PathBuf::from("diagnostic-recovery-path");
    let mut recovery = LocalTempRecovery::from_native_entries(
        path,
        vec![entry(&parent, "replaced", Some(original), Some(original_identity))],
    );

    assert_eq!(
        recovery.cleanup().expect_err("replacement must be preserved").kind(),
        LocalFileErrorKind::RequirementNotMet
    );
    assert_eq!(
        std::fs::read(&replacement).expect("replacement remains"),
        b"replacement"
    );
    assert!(recovery.cleanup().is_err(), "failed entry remains owned for retry");
    drop(recovery);
    assert_eq!(
        std::fs::read(&replacement).expect("replacement survives owner drop"),
        b"replacement"
    );
}

#[test]
fn test_review_recovery_complete_cleanup_is_idempotent() {
    let fixture = tempfile::tempdir().expect("temporary directory");
    let parent = File::open(fixture.path()).expect("open parent directory");
    let mut entries = Vec::new();
    for name in ["one", "two"] {
        let file = crate::temp::internal::privacy_unix::create_private_file_at(&parent, std::ffi::OsStr::new(name))
            .expect("create file");
        let file_identity = identity(&file);
        entries.push(entry(&parent, name, Some(file), Some(file_identity)));
    }
    let mut recovery = LocalTempRecovery::from_native_entries(PathBuf::new(), entries);
    recovery.cleanup().expect("cleanup all entries");
    recovery.cleanup().expect("cleanup after completion is idempotent");
}

#[test]
fn test_recovery_nul_name_refuses_cleanup_without_losing_owned_file() {
    let fixture = tempfile::tempdir().expect("temporary directory");
    let parent = File::open(fixture.path()).expect("open parent directory");
    let path = fixture.path().join("retained");
    std::fs::write(&path, b"retained contents").expect("create original file");
    let file = File::open(&path).expect("retain original handle");
    let expected = identity(&file);
    let native = entry(&parent, "retained\0invalid", Some(file), Some(expected.clone()));
    let mut recovery = LocalTempRecovery::from_native(path.clone(), native);

    for _ in 0..2 {
        let error = recovery
            .cleanup()
            .expect_err("native NUL name cannot identify a cleanup target");
        assert_eq!(error.kind(), LocalFileErrorKind::InvalidPath);
        assert_eq!(error.operation(), crate::LocalFileOperation::Cleanup);
        assert_eq!(recovery.identity(), Some(&expected));
        assert_eq!(
            std::fs::read(&path).expect("original remains owned"),
            b"retained contents"
        );
    }
    drop(recovery);
    assert_eq!(
        std::fs::read(&path).expect("best-effort drop must also refuse invalid name"),
        b"retained contents"
    );
}
