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
use std::io::ErrorKind;
use std::io::IoSlice;
use std::io::Seek;
use std::io::SeekFrom;
use std::io::Write;
use std::path::Path;
use std::path::PathBuf;
#[cfg(not(windows))]
use std::process::Command;

use qubit_local_files::LocalFileSystem;
use qubit_local_files::error::LocalFileErrorKind;
#[cfg(feature = "internal-test-support")]
use qubit_local_files::error::LocalFileOperation;
use qubit_local_files::options::LocalPersistOptions;
use qubit_local_files::options::LocalTempFileOptions;
use qubit_local_files::outcome::LocalPersistCleanupState;
use qubit_local_files::outcome::LocalPersistFailureState;
use qubit_local_files::outcome::LocalPersistMethod;
#[cfg(feature = "internal-test-support")]
use qubit_local_files::test_support::install_test_fault;
use tempfile::tempdir;

fn rooted_host_path(root: &Path, virtual_path: &Path) -> PathBuf {
    root.join(
        virtual_path
            .strip_prefix(Path::new(std::path::MAIN_SEPARATOR_STR))
            .expect("Rooted public paths are virtual absolute"),
    )
}

#[cfg(feature = "internal-test-support")]
fn run_in_test_fault_process<F>(test_name: &str, fault: &str, action: F)
where
    F: FnOnce(),
{
    const TEST_FAULT_ENV: &str = "QUBIT_LOCAL_FILES_TEST_FAULT";
    const TEST_FAULT_CHILD_ENV: &str = "QUBIT_LOCAL_FILES_TEST_FAULT_CHILD";
    if std::env::var_os(TEST_FAULT_ENV).is_some_and(|selected| selected == std::ffi::OsStr::new(fault)) {
        let _fault = install_test_fault(fault).expect("test fault controller should install");
        action();
        return;
    }
    if std::env::var_os(TEST_FAULT_CHILD_ENV).is_some() {
        return;
    }
    let executable = std::env::current_exe().expect("test executable should be available");
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

/// Runs a current-directory failure scenario in a child process so changing
/// the process directory cannot affect concurrent tests.
#[cfg(not(windows))]
fn run_in_deleted_current_directory_process(test_name: &str, action: impl FnOnce()) {
    const CHILD_ENV: &str = "QUBIT_LOCAL_FILES_DELETED_CWD_TEST";
    if env::var_os(CHILD_ENV).is_some() {
        action();
        return;
    }

    let executable = env::current_exe().expect("test executable should be available");
    let status = Command::new(executable)
        .arg("--exact")
        .arg(test_name)
        .arg("--nocapture")
        .env(CHILD_ENV, "1")
        .status()
        .expect("deleted-current-directory child should launch");
    assert!(status.success(), "deleted-current-directory child should pass");
}

/// Verifies closing file I/O does not release the retained persistence
/// responsibility.
#[test]
fn test_local_temp_file_close_retains_path_and_persist_responsibility() {
    let parent = tempdir().expect("temporary parent should be created");
    let target = parent.path().join("persisted");
    let mut temporary = LocalFileSystem::host()
        .expect("Host filesystem should open")
        .create_temp_file_with_options(&LocalTempFileOptions::new().with_parent(parent.path()))
        .expect("temporary file should be created");
    let path = temporary.path().to_path_buf();

    temporary.close();

    assert_eq!(path, temporary.path());
    assert_eq!(
        ErrorKind::BrokenPipe,
        temporary
            .seek(SeekFrom::Start(0))
            .expect_err("closed file should reject seeks")
            .kind()
    );
    let outcome = temporary.persist(&target).expect("closed file should persist");
    assert_eq!(target, outcome.path());
    assert!(target.exists());
}

/// Verifies explicit options create a missing host temporary-file parent.
#[test]
fn test_local_temp_file_create_parent_creates_missing_host_parent() {
    let root = tempdir().expect("temporary root should be created");
    let parent = root.path().join("missing").join("parent");

    let temporary = LocalFileSystem::host()
        .expect("Host filesystem should open")
        .create_temp_file_with_options(&LocalTempFileOptions::new().with_parent(&parent).with_create_parent())
        .expect("explicit parent creation should create the temporary parent");

    assert!(parent.is_dir());
    drop(temporary);
}

/// Verifies closed temporary-file handles report the stream-closure error.
#[test]
fn test_local_temp_file_closed_handle_reports_broken_pipe() {
    let parent = tempdir().expect("temporary parent should be created");
    let mut temporary = LocalFileSystem::host()
        .expect("Host filesystem should open")
        .create_temp_file_with_options(&LocalTempFileOptions::new().with_parent(parent.path()))
        .expect("temporary file should be created");

    temporary.close();

    let error = temporary
        .as_file_mut()
        .expect_err("closed temporary file must reject handle access");
    assert_eq!(ErrorKind::BrokenPipe, error.kind());
}

/// Verifies keeping a temporary file retains its contents after its guard is
/// consumed and disables cleanup for the generated sandbox.
#[test]
fn test_local_temp_file_keep_retains_contents_after_guard_is_consumed() {
    let parent = tempdir().expect("temporary parent should be created");
    let path = {
        let mut temporary = LocalFileSystem::host()
            .expect("Host filesystem should open")
            .create_temp_file_with_options(&LocalTempFileOptions::new().with_parent(parent.path()))
            .expect("temporary file should be created");
        temporary
            .write_all(b"payload")
            .expect("temporary file should accept bytes");
        let outcome = temporary.keep().expect("temporary file should publish");
        assert!(!outcome.durable());
        outcome.into_parts().0
    };

    assert_eq!(
        b"payload",
        fs::read(&path)
            .expect("kept temporary file should remain readable")
            .as_slice()
    );
    fs::remove_file(&path).expect("kept temporary file should be removable");
    fs::remove_dir(
        path.parent()
            .expect("kept temporary file should retain its sandbox parent"),
    )
    .expect("empty temporary sandbox should be removable");
}

/// Verifies a generated keep collision preserves the guard for a later retry.
#[test]
fn test_local_temp_file_keep_conflict_retains_resource_for_retry() {
    let parent = tempdir().expect("temporary parent should be created");
    let temporary = LocalFileSystem::host()
        .expect("Host filesystem should open")
        .create_temp_file_with_options(&LocalTempFileOptions::new().with_parent(parent.path()))
        .expect("temporary file should be created");
    let source = temporary.path().to_path_buf();
    let target = source
        .parent()
        .and_then(Path::parent)
        .expect("temporary resource should have a publication parent")
        .join(source.file_name().expect("temporary resource should have a name"));
    fs::write(&target, b"existing").expect("generated target should be reservable");

    let error = temporary
        .keep()
        .expect_err("occupied generated target should reject keep");
    assert_eq!(LocalPersistFailureState::NotPublished, error.state());
    let (_, temporary, requested, resolved, _) = error.into_parts();
    assert_eq!(target, requested);
    assert_eq!(Some(target.clone()), resolved);

    fs::remove_file(&target).expect("fixture collision should be removable");
    let outcome = temporary.keep().expect("retained temporary file should retry keep");
    assert_eq!(&target, outcome.path());
    fs::remove_file(target).expect("published fixture should be removable");
}

/// Verifies a temporary file is isolated in a private cleanup sandbox.
#[cfg(not(windows))]
#[test]
fn test_local_temp_file_uses_private_cleanup_sandbox() {
    let parent = tempdir().expect("temporary parent should be created");
    let temporary = LocalFileSystem::host()
        .expect("Host filesystem should open")
        .create_temp_file_with_options(&LocalTempFileOptions::new().with_parent(parent.path()))
        .expect("temporary file should be created");
    let resource_path = temporary.path().to_path_buf();
    let sandbox = resource_path
        .parent()
        .expect("temporary file should have a sandbox parent")
        .to_path_buf();

    let canonical_parent = fs::canonicalize(parent.path()).expect("temporary parent should canonicalize");
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

/// Verifies detailed persistence reports the actual atomic rename outcome.
#[test]
fn test_local_temp_file_persist_reports_atomic_rename() {
    let parent = tempdir().expect("temporary parent should be created");
    let target = parent.path().join("persisted");
    let temporary = LocalFileSystem::host()
        .expect("Host filesystem should open")
        .create_temp_file_with_options(&LocalTempFileOptions::new().with_parent(parent.path()))
        .expect("temporary file should be created");

    let outcome = temporary
        .persist_with(&target, LocalPersistOptions::new())
        .expect("temporary file should persist");

    assert_eq!(target, outcome.path());
    assert!(outcome.atomic());
    assert!(!outcome.durable());
    assert_eq!(LocalPersistMethod::AtomicRename, outcome.method());
}

/// Verifies a relative temporary parent remains bound after the current
/// directory changes.
#[cfg(not(windows))]
#[test]
fn test_local_temp_file_relative_parent_remains_bound_after_current_directory_change() {
    const TEST_NAME: &str = "test_local_temp_file_relative_parent_remains_bound_after_current_directory_change";
    run_in_deleted_current_directory_process(TEST_NAME, || {
        let creation = tempdir().expect("creation directory should be created");
        let later = tempdir().expect("later directory should be created");
        let original = env::current_dir().expect("original current directory should be available");
        env::set_current_dir(creation.path()).expect("creation directory should become current");

        let mut temporary = LocalFileSystem::host()
            .expect("Host filesystem should open")
            .create_temp_file_with_options(
                &LocalTempFileOptions::new()
                    .with_parent(Path::new("temporary"))
                    .with_create_parent(),
            )
            .expect("temporary file should be created");
        let path = temporary.path().to_path_buf();

        assert!(path.is_absolute());
        assert!(path.starts_with(fs::canonicalize(creation.path()).expect("creation directory should canonicalize")));
        env::set_current_dir(later.path()).expect("later directory should become current");
        temporary.cleanup().expect("bound temporary file should clean up");
        assert!(!path.exists());

        env::set_current_dir(original).expect("original current directory should be restored");
    });
}

/// Verifies temporary-file creation rejects a zero collision-retry budget
/// before creating a parent entry.
#[test]
fn test_local_temp_file_rejects_zero_creation_attempts() {
    let parent = tempdir().expect("temporary parent should be created");
    let error = LocalFileSystem::host()
        .expect("Host filesystem should open")
        .create_temp_file_with_options(
            &LocalTempFileOptions::new()
                .with_parent(parent.path())
                .with_max_attempts(0),
        )
        .expect_err("zero creation attempts must be rejected");

    assert_eq!(LocalFileErrorKind::InvalidOptions, error.kind());
}

/// Verifies open temporary files implement the ordinary seekable write stream
/// contract before their handles are closed.
#[test]
fn test_local_temp_file_reads_back_written_content_before_cleanup() {
    let parent = tempdir().expect("temporary parent should be created");
    let mut temporary = LocalFileSystem::host()
        .expect("Host filesystem should open")
        .create_temp_file_with_options(&LocalTempFileOptions::new().with_parent(parent.path()))
        .expect("temporary file should be created");
    let path = temporary.path().to_path_buf();

    assert_eq!(
        7,
        temporary
            .write_vectored(&[IoSlice::new(b"pay"), IoSlice::new(b"load")])
            .expect("temporary file should accept vectored bytes")
    );
    let offset = temporary
        .stream_position()
        .expect("temporary file should report its current offset");
    temporary.flush().expect("temporary file should flush its bytes");

    assert_eq!(7, offset);
    assert_eq!(
        b"payload",
        fs::read(&path).expect("temporary path should read").as_slice()
    );
    temporary.cleanup().expect("temporary file should be removed");
    assert!(!path.exists());
}

/// Verifies mutable native-file access remains available until an explicit
/// close and writes through the temporary-file guard.
#[test]
fn test_local_temp_file_exposes_mutable_open_file_handle() {
    let parent = tempdir().expect("temporary parent should be created");
    let mut temporary = LocalFileSystem::host()
        .expect("Host filesystem should open")
        .create_temp_file_with_options(&LocalTempFileOptions::new().with_parent(parent.path()))
        .expect("temporary file should be created");
    let path = temporary.path().to_path_buf();

    temporary
        .as_file_mut()
        .expect("open temporary file should expose its native handle")
        .write_all(b"payload")
        .expect("native handle should write through the guard");
    temporary
        .flush()
        .expect("temporary file should flush native-handle writes");

    assert_eq!(
        b"payload",
        fs::read(&path).expect("temporary path should read").as_slice()
    );
    temporary.cleanup().expect("temporary file should clean up");
}

/// Verifies parent preparation failures retain a cleanup-safe temporary file.
#[test]
fn test_local_temp_file_persist_rejects_non_directory_parent_and_retains_cleanup() {
    let parent = tempdir().expect("temporary parent should be created");
    let blocked_parent = parent.path().join("blocked");
    fs::write(&blocked_parent, b"not a directory").expect("blocked parent fixture should be written");
    let temporary = LocalFileSystem::host()
        .expect("Host filesystem should open")
        .create_temp_file_with_options(&LocalTempFileOptions::new().with_parent(parent.path()))
        .expect("temporary file should be created");
    let source = temporary.path().to_path_buf();

    let error = temporary
        .persist(blocked_parent.join("target"))
        .expect_err("a file cannot serve as a target parent");
    let (_io, mut temporary, _requested, _resolved, _stage) = error.into_parts();
    temporary
        .cleanup()
        .expect("parent preparation failure must retain cleanup authority");

    assert!(!source.exists());
}

/// Verifies a known host type conflict preserves cleanup ownership.
#[cfg(not(windows))]
#[test]
fn test_local_temp_file_known_persist_conflict_retains_cleanup() {
    let parent = tempdir().expect("temporary parent should be created");
    let target = parent.path().join("target-directory");
    fs::create_dir(&target).expect("target directory fixture should exist");
    let temporary = LocalFileSystem::host()
        .expect("Host filesystem should open")
        .create_temp_file_with_options(&LocalTempFileOptions::new().with_parent(parent.path()))
        .expect("temporary file should be created");
    let source = temporary.path().to_path_buf();

    let error = temporary
        .persist_with(&target, LocalPersistOptions::new().with_overwrite())
        .expect_err("a file cannot replace a directory");
    assert_eq!(LocalPersistFailureState::NotPublished, error.state());
    let (_io, mut temporary, _requested, _resolved, _stage) = error.into_parts();
    temporary
        .cleanup()
        .expect("known type conflicts must retain cleanup authority");

    assert!(!source.exists());
}

/// Verifies a native persistence-install failure records indeterminate state.
#[cfg(feature = "internal-test-support")]
#[test]
fn test_local_temp_file_persist_reports_indeterminate_install() {
    const TEST_NAME: &str = "test_local_temp_file_persist_reports_indeterminate_install";
    const TEST_FAULT_ENV: &str = "QUBIT_LOCAL_FILES_TEST_FAULT";
    const TEST_FAULT_CHILD_ENV: &str = "QUBIT_LOCAL_FILES_TEST_FAULT_CHILD";
    if std::env::var_os(TEST_FAULT_CHILD_ENV).is_some()
        && std::env::var_os(TEST_FAULT_ENV)
            .is_none_or(|selected| selected != std::ffi::OsStr::new("persist-install-indeterminate"))
    {
        return;
    }
    if std::env::var_os(TEST_FAULT_ENV)
        .is_none_or(|selected| selected != std::ffi::OsStr::new("persist-install-indeterminate"))
    {
        let status = std::process::Command::new(std::env::current_exe().expect("test executable should be available"))
            .arg("--exact")
            .arg(TEST_NAME)
            .arg("--nocapture")
            .env(TEST_FAULT_ENV, "persist-install-indeterminate")
            .env(TEST_FAULT_CHILD_ENV, "1")
            .status()
            .expect("test fault child should launch");
        assert!(status.success(), "test fault child should pass");
        return;
    }

    let _fault = install_test_fault("persist-install-indeterminate").expect("test fault controller should install");

    let parent = tempdir().expect("temporary parent should be created");
    let temporary = LocalFileSystem::host()
        .expect("Host filesystem should open")
        .create_temp_file_with_options(&LocalTempFileOptions::new().with_parent(parent.path()))
        .expect("temporary file should be created");
    let target = parent.path().join("indeterminate-target");
    let error = temporary
        .persist(&target)
        .expect_err("injected install failure should be reported");
    assert_eq!(LocalPersistFailureState::Indeterminate, error.state());
}

/// Verifies a Rooted absolute persistence target is re-anchored at the virtual
/// root instead of being interpreted in the Host namespace.
#[test]
fn test_rooted_temp_file_reanchors_absolute_persist_target() {
    let parent = tempdir().expect("root parent should be created");
    let rooted = LocalFileSystem::rooted(parent.path()).expect("root authority should open");
    let temporary = rooted
        .create_temp_file_with_options(&LocalTempFileOptions::new())
        .expect("rooted temporary file should be created");
    let outcome = temporary
        .persist(Path::new("/absolute-target"))
        .expect("rooted persistence should accept virtual absolute targets");

    assert_eq!(Path::new("/absolute-target"), outcome.path());
    assert!(parent.path().join("absolute-target").is_file());
}

/// Verifies rooted temporary-file overwrite persistence replaces the final
/// entry within the opened root.
#[cfg(not(windows))]
#[test]
fn test_rooted_temp_file_persist_with_overwrite_replaces_target() {
    let parent = tempdir().expect("root parent should be created");
    let rooted = LocalFileSystem::rooted(parent.path()).expect("root authority should open");
    fs::write(parent.path().join("target"), b"existing").expect("target fixture should be written");
    let mut temporary = rooted
        .create_temp_file_with_options(&LocalTempFileOptions::new())
        .expect("rooted temporary file should be created");
    temporary
        .write_all(b"replacement")
        .expect("rooted temporary file should accept bytes");

    let persisted = temporary
        .persist_with(
            std::path::Path::new("target"),
            LocalPersistOptions::new().with_overwrite(),
        )
        .expect("rooted overwrite should publish the temporary file");

    assert_eq!(std::path::Path::new("/target"), persisted.path());
    assert_eq!(
        b"replacement",
        fs::read(parent.path().join("target"))
            .expect("target should read")
            .as_slice(),
    );
}

/// Verifies rooted temporary files can publish to an absent relative target
/// and release automatic cleanup after publication.
#[cfg(not(windows))]
#[test]
fn test_rooted_temp_file_persist_publishes_absent_target() {
    let parent = tempdir().expect("root parent should be created");
    let rooted = LocalFileSystem::rooted(parent.path()).expect("root authority should open");
    let mut temporary = rooted
        .create_temp_file_with_options(&LocalTempFileOptions::new())
        .expect("rooted temporary file should be created");
    temporary
        .write_all(b"payload")
        .expect("rooted temporary file should accept bytes");
    let source = temporary.path().to_path_buf();

    let outcome = temporary
        .persist(std::path::Path::new("published"))
        .expect("rooted temporary file should publish");
    assert_eq!(std::path::Path::new("/published"), outcome.path());
    assert!(!rooted_host_path(parent.path(), &source).exists());
    assert_eq!(
        b"payload",
        fs::read(parent.path().join("published"))
            .expect("published file should read")
            .as_slice(),
    );
}

/// Verifies rooted temporary-file cleanup removes the retained relative path.
#[test]
fn test_rooted_temp_file_cleanup_removes_entry() {
    let parent = tempdir().expect("root parent should be created");
    let rooted = LocalFileSystem::rooted(parent.path()).expect("root authority should open");
    let mut temporary = rooted
        .create_temp_file_with_options(&LocalTempFileOptions::new())
        .expect("rooted temporary file should be created");
    let path = temporary.path().to_path_buf();

    temporary.cleanup().expect("rooted temporary file should clean up");

    assert!(!rooted_host_path(parent.path(), &path).exists());
}

/// Verifies rooted temporary files expose their stream, retain a relative
/// path when kept, and never remove a replacement that takes their name.
#[cfg(not(windows))]
#[test]
fn test_rooted_temp_file_stream_keep_and_cleanup_rejects_replacement() {
    let parent = tempdir().expect("root parent should be created");
    let rooted = LocalFileSystem::rooted(parent.path()).expect("root authority should open");
    let kept_path = {
        let mut temporary = rooted
            .create_temp_file_with_options(&LocalTempFileOptions::new())
            .expect("rooted temporary file should be created");
        assert_eq!(
            7,
            temporary
                .write_vectored(&[IoSlice::new(b"root"), IoSlice::new(b"ed!")])
                .expect("rooted temporary file should accept vectored bytes"),
        );
        temporary.flush().expect("rooted temporary file should flush");
        temporary
            .seek(SeekFrom::Start(0))
            .expect("rooted temporary file should seek");
        assert!(
            temporary
                .as_file_mut()
                .expect("rooted temporary file should expose its stream")
                .metadata()
                .expect("rooted temporary stream metadata should read")
                .is_file(),
        );
        temporary
            .keep()
            .expect("rooted temporary file should publish")
            .into_parts()
            .0
    };
    assert_eq!(
        b"rooted!",
        fs::read(rooted_host_path(parent.path(), &kept_path))
            .expect("kept rooted temporary file should remain")
            .as_slice(),
    );
    fs::remove_file(rooted_host_path(parent.path(), &kept_path))
        .expect("kept rooted temporary file should be removable");

    let mut temporary = rooted
        .create_temp_file_with_options(&LocalTempFileOptions::new())
        .expect("second rooted temporary file should be created");
    let path = temporary.path().to_path_buf();
    let original = parent.path().join("original");
    let host_path = rooted_host_path(parent.path(), &path);
    fs::rename(&host_path, &original).expect("original rooted temporary file should be retained");
    fs::write(&host_path, b"replacement").expect("replacement rooted file should be created");
    let error = temporary
        .cleanup()
        .expect_err("rooted cleanup must reject a replacement entry");
    assert_eq!(LocalFileErrorKind::InvalidPath, error.kind());
    assert_eq!(
        b"replacement",
        fs::read(&host_path)
            .expect("replacement rooted file should remain")
            .as_slice(),
    );
    fs::remove_file(&host_path).expect("replacement rooted file should be removable");
    fs::remove_file(original).expect("original rooted file should be removable");
}

/// Verifies rooted temporary files retain cleanup authority after a no-replace
/// conflict and reject lexical escape targets.
#[test]
fn test_rooted_temp_file_conflicts_and_invalid_targets_retain_cleanup() {
    let parent = tempdir().expect("root parent should be created");
    let rooted = LocalFileSystem::rooted(parent.path()).expect("root authority should open");
    fs::write(parent.path().join("occupied"), b"existing").expect("occupied target should be written");
    let temporary = rooted
        .create_temp_file_with_options(&LocalTempFileOptions::new())
        .expect("rooted temporary file should be created");
    let source = temporary.path().to_path_buf();

    let error = temporary
        .persist(std::path::Path::new("occupied"))
        .expect_err("default persistence must retain an occupied target");
    let (_io, temporary, _requested, resolved, _stage) = error.into_parts();
    assert_eq!(Some(std::path::Path::new("/occupied")), resolved.as_deref());

    let error = temporary
        .persist(std::path::Path::new("../escape"))
        .expect_err("rooted persistence must reject lexical escapes");
    let (_io, mut temporary, _requested, resolved, _stage) = error.into_parts();
    assert_eq!(None, resolved);
    temporary
        .cleanup()
        .expect("conflicted rooted file should remain cleanup-safe");
    assert!(!rooted_host_path(parent.path(), &source).exists());
}

/// Verifies dropping an externally removed temporary file is best effort.
#[test]
fn test_local_temp_file_drop_tolerates_missing_entry() {
    let parent = tempdir().expect("temporary parent should be created");
    let temporary = LocalFileSystem::host()
        .expect("Host filesystem should open")
        .create_temp_file_with_options(&LocalTempFileOptions::new().with_parent(parent.path()))
        .expect("temporary file should be created");
    let path = temporary.path().to_path_buf();
    fs::remove_file(&path).expect("fixture should remove the temporary file");

    drop(temporary);

    assert!(!path.exists());
}

/// Verifies an absolute-parent Host temp file does not capture a PWD merely to
/// support a later relative persistence target.
#[cfg(not(windows))]
#[test]
fn test_local_temp_file_persist_reports_deleted_current_directory() {
    const TEST_NAME: &str = "test_local_temp_file_persist_reports_deleted_current_directory";
    run_in_deleted_current_directory_process(TEST_NAME, || {
        let original = env::current_dir().expect("original current directory should be available");
        let parent = tempdir().expect("temporary parent should be created");
        let cwd = parent.path().join("deleted-current-directory");
        fs::create_dir(&cwd).expect("current-directory fixture should exist");
        env::set_current_dir(&cwd).expect("current directory should change to the fixture");
        let temporary = LocalFileSystem::host()
            .expect("Host filesystem should open without capturing the fixture PWD")
            .create_temp_file_with_options(&LocalTempFileOptions::new().with_parent(parent.path()))
            .expect("temporary file should be created");
        let source = temporary.path().to_path_buf();

        fs::remove_dir(&cwd).expect("current-directory fixture should be removed externally");
        let error = temporary
            .persist(std::path::Path::new("relative-target"))
            .expect_err("a relative target requires a creation-time PWD snapshot");
        env::set_current_dir(&original).expect("original current directory should be restored");

        let (io, mut temporary, _requested, resolved, _stage) = error.into_parts();
        assert_eq!(LocalFileErrorKind::InvalidPath, io.kind());
        assert_eq!(None, resolved.as_deref());
        temporary
            .cleanup()
            .expect("target resolution failure should retain cleanup authority");
        assert!(!source.exists());
    });
}

/// Verifies keeping a temporary file disables its automatic cleanup.
#[test]
fn test_local_temp_file_keep_retains_path_after_drop() {
    let parent = tempdir().expect("temporary parent should be created");
    let path = LocalFileSystem::host()
        .expect("Host filesystem should open")
        .create_temp_file_with_options(&LocalTempFileOptions::new().with_parent(parent.path()))
        .expect("temporary file should be created")
        .keep()
        .expect("temporary file should publish")
        .into_parts()
        .0;

    assert!(path.exists());
    fs::remove_file(path).expect("kept fixture should be removed manually");
}

/// Verifies failed no-replace persistence preserves cleanup responsibility and
/// explicit replacement publishes the temporary content.
#[test]
fn test_local_temp_file_persist_respects_conflict_and_overwrite_policy() {
    let parent = tempdir().expect("temporary parent should be created");
    let target = parent.path().join("target");
    fs::write(&target, b"existing").expect("target fixture should exist");
    let mut temporary = LocalFileSystem::host()
        .expect("Host filesystem should open")
        .create_temp_file_with_options(&LocalTempFileOptions::new().with_parent(parent.path()))
        .expect("temporary file should be created");
    temporary
        .write_all(b"replacement")
        .expect("temporary file should accept bytes");

    let error = temporary
        .persist(&target)
        .expect_err("default persistence must not replace an existing file");
    assert_eq!(target, error.requested_target());
    let (_io, temporary, _requested, _resolved, _stage) = error.into_parts();
    assert!(temporary.path().exists());

    let persisted = temporary
        .persist_with(&target, LocalPersistOptions::new().with_overwrite())
        .expect("explicit overwrite should publish the temporary file");
    assert_eq!(target, persisted.path());
    assert_eq!(
        b"replacement",
        fs::read(&target).expect("target should read").as_slice()
    );
}

/// Verifies closing a temporary file rejects further stream access while still
/// permitting cleanup.
#[test]
fn test_local_temp_file_close_rejects_stream_access_and_allows_cleanup() {
    let parent = tempdir().expect("temporary parent should be created");
    let mut temporary = LocalFileSystem::host()
        .expect("Host filesystem should open")
        .create_temp_file_with_options(&LocalTempFileOptions::new().with_parent(parent.path()))
        .expect("temporary file should be created");
    let path = temporary.path().to_path_buf();

    temporary.close();
    assert!(temporary.write_all(b"unavailable").is_err());
    assert!(temporary.write_vectored(&[IoSlice::new(b"unavailable")]).is_err());
    assert!(temporary.flush().is_err());
    assert!(temporary.seek(SeekFrom::Start(0)).is_err());
    assert!(temporary.as_file_mut().is_err());
    temporary.cleanup().expect("closed temporary file should clean up");
    assert!(!path.exists());
}

/// Verifies cleanup reports and retries a sandbox removal failure.
#[cfg(feature = "internal-test-support")]
#[test]
fn test_local_temp_file_cleanup_reports_and_retries_sandbox_failure() {
    run_in_test_fault_process(
        "test_local_temp_file_cleanup_reports_and_retries_sandbox_failure",
        "temp-file-sandbox-remove",
        || {
            let parent = tempdir().expect("temporary parent should be created");
            let mut temporary = LocalFileSystem::host()
                .expect("Host filesystem should open")
                .create_temp_file_with_options(&LocalTempFileOptions::new().with_parent(parent.path()))
                .expect("temporary file should be created");
            let resource = temporary.path().to_path_buf();
            let sandbox = resource
                .parent()
                .expect("temporary file should have a sandbox")
                .to_path_buf();

            let error = temporary.cleanup().expect_err("sandbox failure should be reported");
            assert_eq!(LocalFileOperation::Cleanup, error.operation());
            assert!(!resource.exists());
            assert!(sandbox.exists());
            temporary.cleanup().expect("sandbox cleanup should be retryable");
            assert!(!sandbox.exists());
        },
    );
}

/// Verifies keep reports residual sandbox cleanup without losing publication.
#[cfg(feature = "internal-test-support")]
#[test]
fn test_local_temp_file_keep_reports_residual_sandbox_cleanup() {
    run_in_test_fault_process(
        "test_local_temp_file_keep_reports_residual_sandbox_cleanup",
        "temp-file-sandbox-remove",
        || {
            let parent = tempdir().expect("temporary parent should be created");
            let outcome = LocalFileSystem::host()
                .expect("Host filesystem should open")
                .create_temp_file_with_options(&LocalTempFileOptions::new().with_parent(parent.path()))
                .expect("temporary file should be created")
                .keep()
                .expect("publication should succeed despite sandbox cleanup failure");
            assert_eq!(LocalPersistCleanupState::ResidualSandbox, outcome.cleanup_state());
            assert!(outcome.cleanup_error().is_some());
            let (path, _) = outcome.into_parts();
            fs::remove_file(&path).expect("published file should remain removable");
        },
    );
}

/// Verifies cleanup rejects a path that no longer names the created entry.
///
/// This fixture installs a separately created replacement and therefore
/// verifies the ordinary distinct-identity case, not identity reuse.
#[test]
fn test_local_temp_file_cleanup_rejects_replaced_entry() {
    let parent = tempdir().expect("temporary parent should be created");
    let mut temporary = LocalFileSystem::host()
        .expect("Host filesystem should open")
        .create_temp_file_with_options(&LocalTempFileOptions::new().with_parent(parent.path()))
        .expect("temporary file should be created");
    let path = temporary.path().to_path_buf();
    let replacement = parent.path().join("replacement-file");
    fs::write(&replacement, b"restored").expect("replacement fixture should be created first");
    fs::remove_file(&path).expect("fixture should remove the temporary file");
    fs::rename(&replacement, &path).expect("fixture should atomically install the replacement");
    let error = temporary
        .cleanup()
        .expect_err("cleanup must reject the replacement entry");
    assert_eq!(LocalFileErrorKind::InvalidPath, error.kind());
    assert_eq!(fs::read(&path).expect("replacement must remain"), b"restored");
}

/// Verifies silent best-effort drop does not remove a different replacement.
#[test]
fn test_local_temp_file_drop_tolerates_replaced_directory() {
    let parent = tempdir().expect("temporary parent should be created");
    let temporary = LocalFileSystem::host()
        .expect("Host filesystem should open")
        .create_temp_file_with_options(&LocalTempFileOptions::new().with_parent(parent.path()))
        .expect("temporary file should be created");
    let path = temporary.path().to_path_buf();
    fs::remove_file(&path).expect("fixture should remove the temporary file");
    fs::create_dir(&path).expect("fixture should replace the file with a directory");

    drop(temporary);

    assert!(path.is_dir());
    fs::remove_dir(path).expect("replacement directory should be removed");
}

/// Verifies persistence never publishes a same-kind entry that replaced the
/// temporary file path after creation.
#[test]
fn test_local_temp_file_persist_rejects_replaced_file() {
    let parent = tempdir().expect("temporary parent should be created");
    let target = parent.path().join("persisted");
    let temporary = LocalFileSystem::host()
        .expect("Host filesystem should open")
        .create_temp_file_with_options(&LocalTempFileOptions::new().with_parent(parent.path()))
        .expect("temporary file should be created");
    let path = temporary.path().to_path_buf();
    let original = parent.path().join("original");
    fs::rename(&path, &original).expect("fixture should retain the original temporary file");
    fs::write(&path, b"replacement").expect("fixture should replace the temporary file");

    let error = temporary
        .persist(&target)
        .expect_err("persistence must reject a replaced temporary file");
    let (_io, temporary, _requested, _resolved, _stage) = error.into_parts();
    drop(temporary);

    assert!(!target.exists());
    assert_eq!(
        b"replacement",
        fs::read(&path).expect("replacement should remain").as_slice()
    );
    fs::remove_file(path).expect("replacement fixture should be removed");
    fs::remove_file(original).expect("original fixture should be removed");
}
