// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::fs;
use std::io::Read;

use qubit_local_files::LocalFileSystem;
use qubit_local_files::error::LocalFileErrorKind;
use qubit_local_files::error::LocalFileOperation;
use qubit_local_files::options::LocalCopyOptions;
use qubit_local_files::options::LocalCreateDirectoryOptions;
use qubit_local_files::options::LocalDeleteOptions;
use qubit_local_files::options::LocalReadOptions;
use qubit_local_files::options::LocalRenameOptions;
#[cfg(unix)]
use qubit_local_files::policy::LocalDurabilityRequirement;
#[cfg(target_os = "linux")]
use tempfile::NamedTempFile;
use tempfile::tempdir;

/// Verifies Rooted filesystem diagnostics never expose native authority paths.
#[test]
fn test_local_file_system_debug_redacts_root_paths() {
    let directory = tempdir().expect("temporary directory should be created");
    let sensitive_root = directory.path().join("customer-secret-root-8f921d");
    fs::create_dir(&sensitive_root).expect("sensitive rooted directory should be created");
    let filesystem = LocalFileSystem::rooted(&sensitive_root).expect("Rooted filesystem should open");

    let diagnostic = format!("{filesystem:?}");

    assert!(diagnostic.contains("LocalFileSystem"));
    assert!(diagnostic.contains("Rooted"));
    assert!(diagnostic.contains("<redacted>"));
    assert!(!diagnostic.contains("customer-secret-root-8f921d"));
}

/// Verifies default host copy and rename avoid durability synchronization.
#[cfg(target_os = "linux")]
#[test]
fn test_local_file_system_default_copy_and_rename_skip_sync() {
    const CHILD_ENV: &str = "QUBIT_LOCAL_FILES_DEFAULT_HOST_SYNC_CHILD";
    if std::env::var_os(CHILD_ENV).is_some() {
        let directory = tempdir().expect("temporary directory should be created");
        let copy_source = directory.path().join("copy-source");
        let copy_target = directory.path().join("copy-target");
        fs::write(&copy_source, b"copy").expect("copy source should be written");
        let _ = LocalFileSystem::host()
            .expect("Host filesystem should open")
            .copy_with_options(&copy_source, &copy_target, &LocalCopyOptions::new())
            .expect("default copy should succeed");

        let rename_source = directory.path().join("rename-source");
        let rename_target = directory.path().join("rename-target");
        fs::write(&rename_source, b"rename").expect("rename source should be written");
        let _ = LocalFileSystem::host()
            .expect("Host filesystem should open")
            .rename_with_options(&rename_source, &rename_target, &LocalRenameOptions::new())
            .expect("default rename should succeed");
        return;
    }

    if std::process::Command::new("strace").arg("--version").output().is_err() {
        eprintln!("skipping default host sync trace because strace is unavailable");
        return;
    }
    let trace = NamedTempFile::new().expect("trace file should be created");
    let output = std::process::Command::new("strace")
        .args(["-f", "-e", "trace=fsync", "-o"])
        .arg(trace.path())
        .arg(std::env::current_exe().expect("test executable should resolve"))
        .args([
            "--exact",
            "test_local_file_system_default_copy_and_rename_skip_sync",
            "--nocapture",
        ])
        .env(CHILD_ENV, "1")
        .env("LSAN_OPTIONS", "detect_leaks=0")
        .output()
        .expect("strace should launch the traced child");
    if !output.status.success() {
        eprintln!("skipping default host sync trace: ptrace is unavailable");
        return;
    }
    let trace = fs::read_to_string(trace.path()).expect("trace should be readable");
    assert!(
        !trace.contains("fsync("),
        "default durability must not synchronize: {trace}"
    );
}

/// Verifies explicit parent creation and its structured outcome.
#[test]
fn test_local_file_system_create_directory_reports_created_entries() {
    let directory = tempdir().expect("temporary directory should be created");
    let target = directory.path().join("one/two");

    let outcome = LocalFileSystem::host()
        .expect("Host filesystem should open")
        .create_directory_with_options(&target, &LocalCreateDirectoryOptions::new().with_recursive())
        .expect("recursive directory creation should succeed");

    assert!(outcome.created());
    assert!(target.is_dir());
}

/// Verifies host copy can create missing destination parents on request.
#[test]
fn test_local_file_system_copy_creates_missing_parent() {
    let directory = tempdir().expect("temporary directory should be created");
    let source = directory.path().join("source");
    let target = directory.path().join("nested/target");
    fs::write(&source, b"payload").expect("source fixture should be written");

    let _ = LocalFileSystem::host()
        .expect("Host filesystem should open")
        .copy_with_options(&source, &target, &LocalCopyOptions::new().with_create_parent())
        .expect("copy should create the missing parent");

    assert_eq!(
        b"payload",
        fs::read(&target).expect("copied target should read").as_slice()
    );
}

/// Verifies callers may explicitly accept an already existing directory.
#[test]
fn test_local_file_system_create_directory_accepts_existing_directory() {
    let directory = tempdir().expect("temporary directory should be created");
    let target = directory.path().join("existing");
    fs::create_dir(&target).expect("fixture directory should be created");

    let outcome = LocalFileSystem::host()
        .expect("Host filesystem should open")
        .create_directory_with_options(&target, &LocalCreateDirectoryOptions::new().with_exists_ok())
        .expect("existing directory should be accepted");

    assert!(!outcome.created());
}

/// Verifies that opening a reader rejects directories and reads regular files.
#[test]
fn test_local_file_system_open_reader_requires_regular_file() {
    let directory = tempdir().expect("temporary directory should be created");
    let file = directory.path().join("payload");
    fs::write(&file, b"content").expect("fixture should be written");

    let mut reader = LocalFileSystem::host()
        .expect("Host filesystem should open")
        .open_reader_with_options(&file, &LocalReadOptions::new())
        .expect("regular file should open");
    let mut content = String::new();
    reader
        .read_to_string(&mut content)
        .expect("reader should return fixture content");
    assert_eq!("content", content);

    let error = LocalFileSystem::host()
        .expect("Host filesystem should open")
        .open_reader_with_options(directory.path(), &LocalReadOptions::new())
        .expect_err("directory must not be exposed as a file reader");
    assert_eq!(LocalFileErrorKind::TypeConflict, error.kind());
}

/// Verifies bounded prefix reads do not require the complete file length.
#[test]
fn test_local_file_system_read_prefix_is_bounded() {
    let directory = tempdir().expect("temporary directory should be created");
    let file = directory.path().join("payload");
    fs::write(&file, b"0123456789").expect("fixture should be written");

    let filesystem = LocalFileSystem::host().expect("Host filesystem should open");
    assert_eq!(
        b"0123".as_slice(),
        filesystem
            .read_prefix_with_options(&file, 4, &LocalReadOptions::new())
            .expect("prefix should be readable")
            .as_slice()
    );
    assert!(
        filesystem
            .read_prefix_with_options(&file, 0, &LocalReadOptions::new())
            .expect("zero-length prefix should still open")
            .is_empty()
    );

    let missing = directory.path().join("missing");
    let error = filesystem
        .read_prefix_with_options(&missing, 4, &LocalReadOptions::new())
        .expect_err("missing paths must still be validated");
    assert_eq!(LocalFileOperation::OpenReader, error.operation());
}

/// Verifies no-replace rename and explicit overwrite behavior.
#[test]
fn test_local_file_system_rename_respects_overwrite_policy() {
    let directory = tempdir().expect("temporary directory should be created");
    let source = directory.path().join("source");
    let target = directory.path().join("target");
    fs::write(&source, b"new").expect("source fixture should be written");
    fs::write(&target, b"old").expect("target fixture should be written");

    let error = LocalFileSystem::host()
        .expect("Host filesystem should open")
        .rename_with_options(&source, &target, &LocalRenameOptions::new())
        .expect_err("default rename must not replace an entry");
    assert_eq!(LocalFileErrorKind::AlreadyExists, error.error().kind());
    assert_eq!(
        b"old".as_slice(),
        fs::read(&target).expect("target should remain readable").as_slice(),
    );

    let outcome = LocalFileSystem::host()
        .expect("Host filesystem should open")
        .rename_with_options(&source, &target, &LocalRenameOptions::new().with_overwrite())
        .expect("explicit overwrite should replace the target entry");
    assert!(outcome.atomic());
    assert_eq!(
        b"new".as_slice(),
        fs::read(&target).expect("target should be replaced").as_slice(),
    );
}

/// Verifies rename durability distinguishes preferred downgrade from required
/// partial success.
#[cfg(unix)]
#[test]
fn test_local_file_system_rename_reports_parent_sync_result() {
    use std::os::unix::fs::PermissionsExt;

    let directory = tempdir().expect("temporary directory should be created");
    for requirement in [
        LocalDurabilityRequirement::Preferred,
        LocalDurabilityRequirement::Required,
    ] {
        let source = directory.path().join(format!("source-{requirement:?}"));
        let parent = directory.path().join(format!("parent-{requirement:?}"));
        let target = parent.join("target");
        fs::write(&source, b"payload").expect("source should be written");
        fs::create_dir(&parent).expect("target parent should be created");
        fs::set_permissions(&parent, fs::Permissions::from_mode(0o300))
            .expect("target parent should reject directory open");
        let result = LocalFileSystem::host()
            .expect("Host filesystem should open")
            .rename_with_options(
                &source,
                &target,
                &LocalRenameOptions::new().with_durability(requirement),
            );
        match requirement {
            LocalDurabilityRequirement::Preferred => {
                let outcome = result.expect("preferred durability may report a downgrade");
                assert!(!outcome.durable());
            }
            LocalDurabilityRequirement::Required => {
                let error = result.expect_err("required durability must report failure");
                assert_eq!(LocalFileErrorKind::PublicationIncomplete, error.error().kind(),);
            }
            LocalDurabilityRequirement::NotRequired => unreachable!(),
        }
        fs::set_permissions(&parent, fs::Permissions::from_mode(0o700))
            .expect("target parent permissions should be restored");
        assert_eq!(
            b"payload",
            fs::read(&target).expect("renamed target should remain").as_slice(),
        );
    }
}

/// Verifies file and recursive-directory deletion semantics.
#[test]
fn test_local_file_system_delete_uses_explicit_directory_recursion() {
    let directory = tempdir().expect("temporary directory should be created");
    let tree = directory.path().join("tree");
    fs::create_dir(&tree).expect("tree root should be created");
    fs::write(tree.join("child"), b"x").expect("tree child should be written");

    let error = LocalFileSystem::host()
        .expect("Host filesystem should open")
        .delete_directory_with_options(&tree, &LocalDeleteOptions::new())
        .expect_err("non-recursive deletion should reject a non-empty directory");
    assert!(matches!(
        error.kind(),
        LocalFileErrorKind::Io | LocalFileErrorKind::TypeConflict
    ));

    let outcome = LocalFileSystem::host()
        .expect("Host filesystem should open")
        .delete_directory_with_options(&tree, &LocalDeleteOptions::new().with_recursive())
        .expect("recursive deletion should remove the tree");
    assert!(outcome.deleted());
    assert!(!tree.exists());
}
