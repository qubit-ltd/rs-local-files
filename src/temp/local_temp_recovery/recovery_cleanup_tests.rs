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
        parent: std::sync::Arc::new(parent.try_clone().expect("clone parent directory")),
        name: name.into(),
        file: file.map(std::sync::Arc::new),
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
fn test_review_recovery_reports_path_of_failing_entry_after_partial_cleanup() {
    let fixture = tempfile::tempdir().expect("temporary directory");
    let parent = File::open(fixture.path()).expect("open parent directory");
    let first = crate::temp::internal::privacy_unix::create_private_file_at(&parent, std::ffi::OsStr::new("first"))
        .expect("create first file");
    let first_identity = identity(&first);
    let first_path = fixture.path().join("first");
    let failing_path = fixture.path().join("failing");
    let mut recovery = LocalTempRecovery::from_native_entries_with_paths(
        first_path.clone(),
        vec![
            (
                first_path.clone(),
                entry(&parent, "first", Some(first), Some(first_identity)),
            ),
            (failing_path.clone(), entry(&parent, "failing", None, None)),
        ],
    );

    let error = recovery.cleanup().expect_err("second entry has no identity");

    assert!(!first_path.exists(), "the first entry was already cleaned");
    assert_eq!(error.operation(), crate::LocalFileOperation::Cleanup);
    assert_eq!(error.path(), Some(failing_path.as_path()));
}

#[test]
fn test_recovery_partial_cleanup_preserves_replacement_and_context() {
    let fixture = tempfile::tempdir().expect("temporary directory");
    let parent = File::open(fixture.path()).expect("open parent directory");
    let first = crate::temp::internal::privacy_unix::create_private_file_at(&parent, std::ffi::OsStr::new("first"))
        .expect("create first file");
    let first_identity = identity(&first);
    let original = crate::temp::internal::privacy_unix::create_private_file_at(&parent, std::ffi::OsStr::new("second"))
        .expect("create second file");
    let original_identity = identity(&original);
    let second_path = fixture.path().join("second");
    std::fs::remove_file(&second_path).expect("remove original second entry");
    std::fs::write(&second_path, b"replacement").expect("create replacement");
    let mut recovery = LocalTempRecovery::from_native_entries_with_paths(
        fixture.path().join("recovery"),
        vec![
            (
                fixture.path().join("first"),
                entry(&parent, "first", Some(first), Some(first_identity)),
            ),
            (
                second_path.clone(),
                entry(&parent, "second", Some(original), Some(original_identity)),
            ),
        ],
    );

    let error = recovery.cleanup().expect_err("replacement must be preserved");

    assert_eq!(error.operation(), crate::LocalFileOperation::Cleanup);
    assert_eq!(error.path(), Some(second_path.as_path()));
    assert!(!fixture.path().join("first").exists(), "successful prefix was released");
    assert_eq!(
        std::fs::read(&second_path).expect("replacement remains"),
        b"replacement"
    );
}

#[test]
fn test_recovery_observation_failure_keeps_source_and_failing_path() {
    use crate::tests::native_io_fault_tests::NativeIoFaultGuard;
    use crate::tests::native_io_fault_tests::NativeIoPoint;

    let fixture = tempfile::tempdir().expect("temporary directory");
    let parent = File::open(fixture.path()).expect("open parent directory");
    let mut entries = Vec::new();
    for name in ["first", "second"] {
        let file = crate::temp::internal::privacy_unix::create_private_file_at(&parent, std::ffi::OsStr::new(name))
            .expect("create recovery file");
        let file_identity = identity(&file);
        entries.push((
            fixture.path().join(name),
            entry(&parent, name, Some(file), Some(file_identity)),
        ));
    }
    let failing_path = fixture.path().join("second");
    let mut recovery = LocalTempRecovery::from_native_entries_with_paths(fixture.path().join("recovery"), entries);
    // Each entry observes its retained handle; the second handle observation fails.
    let fault = NativeIoFaultGuard::fail_on(NativeIoPoint::Observation, 2);

    let error = recovery
        .cleanup()
        .expect_err("second retained identity observation fails");

    assert!(fault.fired());
    assert_eq!(error.operation(), crate::LocalFileOperation::Cleanup);
    assert_eq!(error.path(), Some(failing_path.as_path()));
    assert_eq!(error.io_error_kind(), std::io::ErrorKind::Other);
    assert!(error.to_string().contains("injected native I/O contract failure"));
    assert!(
        !fixture.path().join("first").exists(),
        "first entry was already released"
    );
    assert!(failing_path.exists(), "the second retained entry remains");
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

#[cfg(unix)]
#[test]
fn test_recovery_refuses_symlink_replacement() {
    let fixture = tempfile::tempdir().expect("temporary directory");
    let parent = File::open(fixture.path()).expect("open parent directory");
    let original =
        crate::temp::internal::privacy_unix::create_private_file_at(&parent, std::ffi::OsStr::new("replaced"))
            .expect("create original file");
    let original_identity = identity(&original);
    let target = fixture.path().join("target");
    std::fs::write(&target, b"keep target").expect("create symlink target");
    let replacement = fixture.path().join("replaced");
    std::fs::remove_file(&replacement).expect("remove original directory entry");
    std::os::unix::fs::symlink(&target, &replacement).expect("replace original with symlink");
    let mut recovery = LocalTempRecovery::from_native(
        replacement.clone(),
        entry(&parent, "replaced", Some(original), Some(original_identity)),
    );

    assert_eq!(
        recovery
            .cleanup()
            .expect_err("replacement symlink must be preserved")
            .kind(),
        LocalFileErrorKind::RequirementNotMet
    );
    assert!(
        std::fs::symlink_metadata(&replacement)
            .expect("replacement symlink remains")
            .file_type()
            .is_symlink()
    );
    assert_eq!(std::fs::read(&target).expect("target remains"), b"keep target");
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

#[test]
fn test_recovery_cleanup_without_owner_read() {
    const CHILD: &str = "QUBIT_RECOVERY_WITHOUT_OWNER_READ_CHILD";
    const PARENT: &str = "QUBIT_RECOVERY_WITHOUT_OWNER_READ_PARENT";
    const SENTINEL: &str = "QUBIT_RECOVERY_WITHOUT_OWNER_READ_SENTINEL";

    if std::env::var_os(CHILD).is_some() {
        use std::os::unix::fs::PermissionsExt;

        struct UmaskGuard(libc::mode_t);
        impl Drop for UmaskGuard {
            fn drop(&mut self) {
                unsafe { libc::umask(self.0) };
            }
        }

        let parent_path = PathBuf::from(std::env::var_os(PARENT).expect("parent path"));
        let parent = File::open(&parent_path).expect("open private parent");
        let previous = unsafe { libc::umask(0o400) };
        let umask = UmaskGuard(previous);
        let error = crate::temp::internal::privacy_unix::create_private_file_at(&parent, std::ffi::OsStr::new("probe"))
            .expect_err("umask should prevent the required 0600 mode");
        drop(umask);

        assert_eq!(error.source.kind(), std::io::ErrorKind::PermissionDenied);
        let retained = error.retained.expect("created file and identity are retained");
        let entry_path = parent_path.join("probe");
        let metadata = std::fs::symlink_metadata(&entry_path).expect("retained file exists");
        assert_eq!(metadata.permissions().mode() & 0o777, 0o200);
        assert!(retained.identity.is_some(), "created file identity is retained");
        let mut recovery = LocalTempRecovery::from_native(entry_path.clone(), retained);

        recovery.cleanup().expect("cleanup must not require owner-read access");
        assert!(!entry_path.exists(), "the retained object is removed");
        std::fs::write(std::env::var_os(SENTINEL).expect("sentinel path"), b"completed")
            .expect("write completion sentinel");
        return;
    }

    let fixture = tempfile::tempdir().expect("private parent");
    let sentinel = fixture.path().join("child-completed");
    let executable = std::env::current_exe().expect("current test executable");
    let exact_name = module_path!()
        .strip_prefix("qubit_local_files::")
        .expect("test module path starts with crate name")
        .to_owned()
        + "::test_recovery_cleanup_without_owner_read";
    let output = std::process::Command::new(executable)
        .arg("--exact")
        .arg(exact_name)
        .arg("--nocapture")
        .env(CHILD, "1")
        .env(PARENT, fixture.path())
        .env(SENTINEL, &sentinel)
        .output()
        .expect("run isolated child test");
    assert!(
        output.status.success(),
        "child failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(sentinel.is_file(), "child must execute and write its sentinel");
    assert_eq!(
        std::fs::read_dir(fixture.path())
            .expect("parent remains readable")
            .count(),
        1,
        "child must leave no recovery object or sandbox"
    );
}
