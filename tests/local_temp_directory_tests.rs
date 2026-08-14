// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

#[cfg(not(windows))]
use std::env;
use std::fs;
use std::path::Path;
#[cfg(not(windows))]
use std::process::Command;

#[cfg(feature = "internal-test-support")]
fn run_in_test_fault_process<F>(test_name: &str, fault: &str, action: F)
where
    F: FnOnce(),
{
    const TEST_FAULT_ENV: &str = "QUBIT_LOCAL_FILES_TEST_FAULT";
    const TEST_FAULT_CHILD_ENV: &str = "QUBIT_LOCAL_FILES_TEST_FAULT_CHILD";
    if std::env::var_os(TEST_FAULT_ENV)
        .is_some_and(|selected| selected == std::ffi::OsStr::new(fault))
    {
        let _fault = install_test_fault(fault)
            .expect("test fault controller should install");
        action();
        return;
    }
    if std::env::var_os(TEST_FAULT_CHILD_ENV).is_some() {
        return;
    }
    let executable =
        std::env::current_exe().expect("test executable should be available");
    let status = std::process::Command::new(executable)
        .arg("--exact")
        .arg(test_name)
        .arg("--nocapture")
        .env(TEST_FAULT_ENV, fault)
        .env(TEST_FAULT_CHILD_ENV, "1")
        .status()
        .expect("test fault child should launch");
    assert!(status.success(), "test fault child should pass");
}

use qubit_local_files::LocalFileErrorKind;
#[cfg(feature = "internal-test-support")]
use qubit_local_files::LocalFileOperation;
use qubit_local_files::LocalFileSystem;
#[cfg(not(windows))]
use qubit_local_files::LocalPersistFailureState;
use qubit_local_files::LocalPersistOptions;
use qubit_local_files::LocalTempDirectoryOptions;
#[cfg(feature = "internal-test-support")]
use qubit_local_files::install_test_fault;
use tempfile::tempdir;

/// Runs a current-directory failure scenario in a child process so changing
/// the process directory cannot affect concurrent tests.
#[cfg(not(windows))]
fn run_in_deleted_current_directory_process(
    test_name: &str,
    action: impl FnOnce(),
) {
    const CHILD_ENV: &str = "QUBIT_LOCAL_FILES_DELETED_CWD_TEST";
    if env::var_os(CHILD_ENV).is_some() {
        action();
        return;
    }

    let executable =
        env::current_exe().expect("test executable should be available");
    let status = Command::new(executable)
        .arg("--exact")
        .arg(test_name)
        .arg("--nocapture")
        .env(CHILD_ENV, "1")
        .status()
        .expect("deleted-current-directory child should launch");
    assert!(
        status.success(),
        "deleted-current-directory child should pass"
    );
}

/// Verifies temporary-directory child helpers reject lexical escape shapes.
#[test]
fn test_local_temp_directory_child_helpers_reject_escape_paths() {
    let parent = tempdir().expect("temporary parent should be created");
    let temporary = LocalFileSystem::host()
        .create_temp_directory(
            &LocalTempDirectoryOptions::new().with_parent(parent.path()),
        )
        .expect("temporary directory should be created");

    assert!(temporary.child(Path::new("nested/file")).is_err());
    assert!(temporary.child(Path::new(".")).is_err());
    assert!(temporary.child(parent.path()).is_err());
    assert!(temporary.descendant(Path::new("../escape")).is_err());
    assert!(temporary.descendant(Path::new(".")).is_err());
}

/// Verifies temporary-directory creation rejects a zero collision-retry
/// budget before reserving an entry.
#[test]
fn test_local_temp_directory_rejects_zero_creation_attempts() {
    let parent = tempdir().expect("temporary parent should be created");
    let error = LocalFileSystem::host()
        .create_temp_directory(
            &LocalTempDirectoryOptions::new()
                .with_parent(parent.path())
                .with_max_attempts(0),
        )
        .expect_err("zero creation attempts must be rejected");

    assert_eq!(LocalFileErrorKind::InvalidOptions, error.kind());
}

/// Verifies directory persistence honors the requested replacement policy.
#[test]
fn test_local_temp_directory_persist_with_overwrite_replaces_empty_destination()
{
    let parent = tempdir().expect("temporary parent should be created");
    let temporary = LocalFileSystem::host()
        .create_temp_directory(
            &LocalTempDirectoryOptions::new().with_parent(parent.path()),
        )
        .expect("temporary directory should be created");
    let target = parent.path().join("published");
    std::fs::create_dir(&target).expect("empty destination should be created");

    let persisted = temporary
        .persist_with(&target, LocalPersistOptions::new().with_overwrite())
        .expect("overwrite persistence should replace an empty directory");

    assert_eq!(target, persisted.path());
    assert!(persisted.path().is_dir());
}

/// Verifies a relative temporary parent remains bound after the current
/// directory changes.
#[cfg(not(windows))]
#[test]
fn test_local_temp_directory_relative_parent_remains_bound_after_current_directory_change()
 {
    const TEST_NAME: &str = "test_local_temp_directory_relative_parent_remains_bound_after_current_directory_change";
    run_in_deleted_current_directory_process(TEST_NAME, || {
        let creation = tempdir().expect("creation directory should be created");
        let later = tempdir().expect("later directory should be created");
        let original = env::current_dir()
            .expect("original current directory should be available");
        env::set_current_dir(creation.path())
            .expect("creation directory should become current");

        let mut temporary = LocalFileSystem::host()
            .create_temp_directory(
                &LocalTempDirectoryOptions::new()
                    .with_parent(Path::new("temporary")),
            )
            .expect("temporary directory should be created");
        let path = temporary.path().to_path_buf();

        assert!(path.is_absolute());
        assert!(
            path.starts_with(
                fs::canonicalize(creation.path())
                    .expect("creation directory should canonicalize")
            )
        );
        env::set_current_dir(later.path())
            .expect("later directory should become current");
        temporary
            .cleanup()
            .expect("bound temporary directory should clean up");
        assert!(!path.exists());

        env::set_current_dir(original)
            .expect("original current directory should be restored");
    });
}

/// Verifies host no-replace persistence publishes a temporary directory to a
/// previously absent destination.
#[test]
fn test_local_temp_directory_persist_publishes_absent_destination() {
    let parent = tempdir().expect("temporary parent should be created");
    let target = parent.path().join("published");
    let temporary = LocalFileSystem::host()
        .create_temp_directory(
            &LocalTempDirectoryOptions::new().with_parent(parent.path()),
        )
        .expect("temporary directory should be created");
    fs::write(temporary.path().join("payload"), b"contents")
        .expect("temporary directory should accept a child");

    let persisted = temporary
        .persist(&target)
        .expect("no-replace persistence should publish an absent destination");

    assert_eq!(target, persisted.path());
    assert_eq!(
        b"contents",
        fs::read(persisted.path().join("payload"))
            .expect("published child should read")
            .as_slice()
    );
}

/// Verifies a successfully persisted directory no longer owns the former
/// temporary path when its guard is dropped.
#[test]
fn test_local_temp_directory_persist_releases_cleanup_ownership() {
    let parent = tempdir().expect("temporary parent should be created");
    let target = parent.path().join("published");
    let temporary = LocalFileSystem::host()
        .create_temp_directory(
            &LocalTempDirectoryOptions::new().with_parent(parent.path()),
        )
        .expect("temporary directory should be created");
    let source = temporary.path().to_path_buf();

    let outcome = temporary
        .persist(&target)
        .expect("temporary directory should persist");
    assert_eq!(target, outcome.path());

    assert!(!source.exists());
    assert!(target.is_dir());
}

/// Verifies cleanup rejects a path that no longer names the created directory.
#[test]
fn test_local_temp_directory_cleanup_rejects_replaced_entry() {
    let parent = tempdir().expect("temporary parent should be created");
    let mut temporary = LocalFileSystem::host()
        .create_temp_directory(
            &LocalTempDirectoryOptions::new().with_parent(parent.path()),
        )
        .expect("temporary directory should be created");
    let path = temporary.path().to_path_buf();
    let replacement = parent.path().join("replacement-directory");
    std::fs::create_dir(&replacement)
        .expect("replacement directory should be created first");
    std::fs::remove_dir(&path)
        .expect("fixture should remove the temporary directory");
    std::fs::rename(&replacement, &path)
        .expect("fixture should atomically install the replacement directory");
    let error = temporary
        .cleanup()
        .expect_err("cleanup must reject the replacement directory");
    assert_eq!(LocalFileErrorKind::InvalidPath, error.kind());
    assert!(path.is_dir());
}

/// Verifies child and descendant helpers resolve safe paths below the temporary
/// directory without creating entries themselves.
#[test]
fn test_local_temp_directory_resolves_safe_children() {
    let parent = tempdir().expect("temporary parent should be created");
    let temporary = LocalFileSystem::host()
        .create_temp_directory(
            &LocalTempDirectoryOptions::new().with_parent(parent.path()),
        )
        .expect("temporary directory should be created");

    assert_eq!(
        temporary.path().join("child"),
        temporary
            .child(Path::new("child"))
            .expect("one component should resolve")
    );
    assert_eq!(
        temporary.path().join("nested/entry"),
        temporary
            .descendant(Path::new("nested/entry"))
            .expect("safe descendant should resolve")
    );
}

/// Verifies keeping a temporary directory leaves its complete tree intact.
#[test]
fn test_local_temp_directory_keep_retains_tree_after_drop() {
    let parent = tempdir().expect("temporary parent should be created");
    let path = LocalFileSystem::host()
        .create_temp_directory(
            &LocalTempDirectoryOptions::new().with_parent(parent.path()),
        )
        .expect("temporary directory should be created")
        .keep();
    fs::write(path.join("child"), b"payload")
        .expect("kept directory should accept a child");

    assert!(path.join("child").is_file());
    assert!(path.parent().is_some_and(Path::is_dir));
    fs::remove_dir_all(path).expect("kept fixture should be removed manually");
}

/// Verifies a temporary directory is isolated in a private cleanup sandbox.
#[cfg(not(windows))]
#[test]
fn test_local_temp_directory_uses_private_cleanup_sandbox() {
    let parent = tempdir().expect("temporary parent should be created");
    let temporary = LocalFileSystem::host()
        .create_temp_directory(
            &LocalTempDirectoryOptions::new().with_parent(parent.path()),
        )
        .expect("temporary directory should be created");
    let resource_path = temporary.path().to_path_buf();
    let sandbox = resource_path
        .parent()
        .expect("temporary directory should have a sandbox parent")
        .to_path_buf();

    let canonical_parent = fs::canonicalize(parent.path())
        .expect("temporary parent should canonicalize");
    assert!(resource_path.starts_with(&canonical_parent));
    assert_ne!(sandbox, canonical_parent);
    assert!(sandbox.is_dir());
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;

        assert_eq!(
            0o700,
            fs::metadata(&sandbox)
                .expect("sandbox metadata should be readable")
                .permissions()
                .mode()
                & 0o777
        );
    }

    drop(temporary);
    assert!(!sandbox.exists());
}

/// Verifies no-replace directory persistence retains the temporary directory
/// for a later explicit overwrite attempt.
#[test]
fn test_local_temp_directory_persist_conflict_retains_resource_for_overwrite() {
    let parent = tempdir().expect("temporary parent should be created");
    let target = parent.path().join("target");
    fs::create_dir(&target).expect("target fixture should exist");
    let temporary = LocalFileSystem::host()
        .create_temp_directory(
            &LocalTempDirectoryOptions::new().with_parent(parent.path()),
        )
        .expect("temporary directory should be created");
    let source = temporary.path().to_path_buf();
    fs::write(source.join("payload"), b"replacement")
        .expect("temporary directory should hold content");

    let error = temporary
        .persist(&target)
        .expect_err("default persistence must not replace a destination");
    assert_eq!(target, error.requested_target());
    let (_io, temporary, _requested, _resolved, _stage) = error.into_parts();
    assert!(source.exists());

    let persisted = temporary
        .persist_with(&target, LocalPersistOptions::new().with_overwrite())
        .expect("explicit overwrite should publish the temporary tree");
    assert_eq!(target, persisted.path());
    assert_eq!(
        b"replacement",
        fs::read(target.join("payload"))
            .expect("target child should read")
            .as_slice()
    );
}

/// Verifies parent preparation failures retain a cleanup-safe temporary
/// directory.
#[test]
fn test_local_temp_directory_persist_rejects_non_directory_parent_and_cleans_up()
 {
    let parent = tempdir().expect("temporary parent should be created");
    let blocked_parent = parent.path().join("blocked");
    fs::write(&blocked_parent, b"not a directory")
        .expect("blocked parent fixture should be written");
    let temporary = LocalFileSystem::host()
        .create_temp_directory(
            &LocalTempDirectoryOptions::new().with_parent(parent.path()),
        )
        .expect("temporary directory should be created");
    let source = temporary.path().to_path_buf();

    let error = temporary
        .persist(blocked_parent.join("target"))
        .expect_err("a file cannot serve as a target parent");
    let (_io, mut temporary, _requested, _resolved, _stage) =
        error.into_parts();
    temporary
        .cleanup()
        .expect("parent preparation failure must retain cleanup authority");

    assert!(!source.exists());
}

/// Verifies a known directory type conflict preserves cleanup ownership.
#[cfg(not(windows))]
#[test]
fn test_local_temp_directory_known_persist_conflict_retains_cleanup() {
    let parent = tempdir().expect("temporary parent should be created");
    let target = parent.path().join("target-file");
    fs::write(&target, b"not a directory")
        .expect("target file fixture should exist");
    let temporary = LocalFileSystem::host()
        .create_temp_directory(
            &LocalTempDirectoryOptions::new().with_parent(parent.path()),
        )
        .expect("temporary directory should be created");
    let source = temporary.path().to_path_buf();

    let error = temporary
        .persist_with(&target, LocalPersistOptions::new().with_overwrite())
        .expect_err("a directory cannot replace a file");
    assert_eq!(LocalPersistFailureState::NotPublished, error.state());
    let (_io, mut temporary, _requested, _resolved, _stage) =
        error.into_parts();
    temporary
        .cleanup()
        .expect("known type conflicts must retain cleanup authority");

    assert!(!source.exists());
}

/// Verifies rooted temporary-directory persistence rejects host-absolute
/// targets and retains root-bound cleanup authority.
#[test]
fn test_rooted_temp_directory_rejects_absolute_persist_target_and_cleans_up() {
    let parent = tempdir().expect("root parent should be created");
    let rooted = LocalFileSystem::rooted(parent.path())
        .expect("root authority should open");
    let temporary = rooted
        .create_temp_directory(&LocalTempDirectoryOptions::new())
        .expect("rooted temporary directory should be created");
    let relative_source = temporary.path().to_path_buf();

    let error = temporary
        .persist(parent.path().join("absolute-target"))
        .expect_err("rooted persistence must reject absolute targets");
    let (_io, mut temporary, _requested, _resolved, _stage) =
        error.into_parts();
    temporary
        .cleanup()
        .expect("rooted temporary directory should clean up");

    assert!(!parent.path().join(relative_source).exists());
}

/// Verifies rooted temporary directories support both fresh publication and
/// explicit replacement through the authority retained at creation time.
#[cfg(not(windows))]
#[test]
fn test_rooted_temp_directory_persist_supports_new_and_overwrite_targets() {
    let parent = tempdir().expect("root parent should be created");
    let rooted = LocalFileSystem::rooted(parent.path())
        .expect("root authority should open");
    let temporary = rooted
        .create_temp_directory(&LocalTempDirectoryOptions::new())
        .expect("first rooted temporary directory should be created");

    let outcome = temporary
        .persist(Path::new("fresh-target"))
        .expect("rooted directory should publish to an absent target");
    assert_eq!(Path::new("fresh-target"), outcome.path());
    assert!(parent.path().join("fresh-target").is_dir());

    fs::create_dir(parent.path().join("replacement-target"))
        .expect("empty replacement target should be created");
    let temporary = rooted
        .create_temp_directory(&LocalTempDirectoryOptions::new())
        .expect("second rooted temporary directory should be created");

    let outcome = temporary
        .persist_with(
            Path::new("replacement-target"),
            LocalPersistOptions::new().with_overwrite(),
        )
        .expect("rooted overwrite should replace the empty target");
    assert_eq!(Path::new("replacement-target"), outcome.path());
    assert!(parent.path().join("replacement-target").is_dir());
}

/// Verifies rooted temporary directories remove their tree through the
/// retained root authority.
#[test]
fn test_rooted_temp_directory_cleanup_removes_descendants() {
    let parent = tempdir().expect("root parent should be created");
    let rooted = LocalFileSystem::rooted(parent.path())
        .expect("root authority should open");
    let mut temporary = rooted
        .create_temp_directory(&LocalTempDirectoryOptions::new())
        .expect("rooted temporary directory should be created");
    let path = temporary.path().to_path_buf();
    let host_path = parent.path().join(&path);
    fs::create_dir(host_path.join("nested"))
        .expect("rooted temporary directory should accept descendants");
    fs::write(host_path.join("nested/payload"), b"payload")
        .expect("rooted temporary descendant should accept content");

    temporary
        .cleanup()
        .expect("rooted cleanup should remove the complete tree");

    assert!(!parent.path().join(path).exists());
}

/// Verifies rooted temporary directories retain cleanup authority after a
/// no-replace publication conflict and reject lexical escape targets.
#[test]
fn test_rooted_temp_directory_conflicts_and_invalid_targets_retain_cleanup() {
    let parent = tempdir().expect("root parent should be created");
    let rooted = LocalFileSystem::rooted(parent.path())
        .expect("root authority should open");
    fs::create_dir(parent.path().join("occupied"))
        .expect("occupied directory should be created");
    let temporary = rooted
        .create_temp_directory(&LocalTempDirectoryOptions::new())
        .expect("rooted temporary directory should be created");
    let source = temporary.path().to_path_buf();

    let error = temporary
        .persist(Path::new("occupied"))
        .expect_err("default persistence must retain an occupied target");
    let (_io, temporary, _requested, resolved, _stage) = error.into_parts();
    assert_eq!(Some(Path::new("occupied")), resolved.as_deref());

    let error = temporary
        .persist(Path::new("../escape"))
        .expect_err("rooted persistence must reject lexical escapes");
    let (_io, mut temporary, _requested, resolved, _stage) = error.into_parts();
    assert_eq!(None, resolved);
    temporary
        .cleanup()
        .expect("conflicted rooted directory should remain cleanup-safe");
    assert!(!parent.path().join(source).exists());
}

/// Verifies directory cleanup reports and retries a sandbox removal failure.
#[cfg(feature = "internal-test-support")]
#[test]
fn test_local_temp_directory_cleanup_reports_and_retries_sandbox_failure() {
    run_in_test_fault_process(
        "test_local_temp_directory_cleanup_reports_and_retries_sandbox_failure",
        "temp-directory-sandbox-remove",
        || {
            let parent = tempdir().expect("temporary parent should be created");
            let mut temporary = LocalFileSystem::host()
                .create_temp_directory(
                    &LocalTempDirectoryOptions::new()
                        .with_parent(parent.path()),
                )
                .expect("temporary directory should be created");
            let resource = temporary.path().to_path_buf();
            let sandbox = resource
                .parent()
                .expect("temporary directory should have a sandbox")
                .to_path_buf();

            let error = temporary
                .cleanup()
                .expect_err("sandbox failure should be reported");
            assert_eq!(LocalFileOperation::Cleanup, error.operation());
            assert!(!resource.exists());
            assert!(sandbox.exists());
            temporary
                .cleanup()
                .expect("sandbox cleanup should be retryable");
            assert!(!sandbox.exists());
        },
    );
}

/// Verifies dropping an externally removed directory is best effort and does
/// not remove an unrelated replacement entry.
#[test]
fn test_local_temp_directory_drop_tolerates_missing_entry() {
    let parent = tempdir().expect("temporary parent should be created");
    let temporary = LocalFileSystem::host()
        .create_temp_directory(
            &LocalTempDirectoryOptions::new().with_parent(parent.path()),
        )
        .expect("temporary directory should be created");
    let path = temporary.path().to_path_buf();
    fs::remove_dir(&path)
        .expect("fixture should remove the temporary directory");

    drop(temporary);

    assert!(!path.exists());
}

/// Verifies drop logs and tolerates a cleanup failure without removing a file
/// that replaced the temporary directory path.
#[test]
fn test_local_temp_directory_drop_tolerates_replaced_file() {
    let parent = tempdir().expect("temporary parent should be created");
    let temporary = LocalFileSystem::host()
        .create_temp_directory(
            &LocalTempDirectoryOptions::new().with_parent(parent.path()),
        )
        .expect("temporary directory should be created");
    let path = temporary.path().to_path_buf();
    fs::remove_dir(&path)
        .expect("fixture should remove the temporary directory");
    fs::write(&path, b"replacement")
        .expect("fixture should replace the directory with a file");

    drop(temporary);

    assert!(path.is_file());
    fs::remove_file(path).expect("replacement file should be removed");
}

/// Verifies cleanup never removes a same-kind directory that replaced the
/// temporary directory path after creation.
#[test]
fn test_local_temp_directory_cleanup_rejects_replaced_directory() {
    let parent = tempdir().expect("temporary parent should be created");
    let mut temporary = LocalFileSystem::host()
        .create_temp_directory(
            &LocalTempDirectoryOptions::new().with_parent(parent.path()),
        )
        .expect("temporary directory should be created");
    let path = temporary.path().to_path_buf();
    let original = parent.path().join("original");
    fs::rename(&path, &original)
        .expect("fixture should retain the original temporary directory");
    fs::create_dir(&path)
        .expect("fixture should replace the temporary directory");

    assert!(temporary.cleanup().is_err());
    drop(temporary);

    assert!(path.is_dir());
    fs::remove_dir(path).expect("replacement fixture should be removed");
    fs::remove_dir(original).expect("original fixture should be removed");
}

/// Verifies relative host persistence reports target-resolution failure when
/// the process current directory was removed externally.
#[cfg(not(windows))]
#[test]
fn test_local_temp_directory_persist_reports_deleted_current_directory() {
    const TEST_NAME: &str =
        "test_local_temp_directory_persist_reports_deleted_current_directory";
    run_in_deleted_current_directory_process(TEST_NAME, || {
        let original = env::current_dir()
            .expect("original current directory should be available");
        let parent = tempdir().expect("temporary parent should be created");
        let cwd = parent.path().join("deleted-current-directory");
        fs::create_dir(&cwd).expect("current-directory fixture should exist");
        let temporary = LocalFileSystem::host()
            .create_temp_directory(
                &LocalTempDirectoryOptions::new().with_parent(parent.path()),
            )
            .expect("temporary directory should be created");
        let source = temporary.path().to_path_buf();

        env::set_current_dir(&cwd)
            .expect("current directory should change to the fixture");
        fs::remove_dir(&cwd)
            .expect("current-directory fixture should be removed externally");
        let error = temporary.persist(Path::new("relative-target")).expect_err(
            "deleted current directory must prevent target resolution",
        );
        env::set_current_dir(&original)
            .expect("original current directory should be restored");

        let (_io, mut temporary, _requested, resolved, _stage) =
            error.into_parts();
        assert_eq!(None, resolved);
        temporary.cleanup().expect(
            "target resolution failure should retain cleanup authority",
        );
        assert!(!source.exists());
    });
}

/// Verifies Windows path prefixes are rejected as temporary-directory children.
#[cfg(windows)]
#[test]
fn test_local_temp_directory_child_rejects_prefix() {
    let parent = tempdir().expect("temporary parent should be created");
    let temporary = LocalFileSystem::host()
        .create_temp_directory(
            &LocalTempDirectoryOptions::new().with_parent(parent.path()),
        )
        .expect("temporary directory should be created");

    assert!(temporary.child(Path::new(r"C:\escape")).is_err());
    assert!(temporary.descendant(Path::new(r"C:\escape")).is_err());
}
