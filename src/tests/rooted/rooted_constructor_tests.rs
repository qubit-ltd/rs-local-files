// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Crate-local tests for rooted authority constructor error context.

use std::fs;
use std::path::Path;
use std::process::Command;

use tempfile::tempdir;

use crate::LocalFileOperation;
use crate::LocalFileSystem;
use crate::LocalSymlinkPolicy;
use crate::options::LocalDirectoryOpenOptions;
use crate::resolution::ResolutionContract;
use crate::rooted_local_file_system::RootedLocalFileSystem;
use crate::tests::native_io_fault_tests::NativeIoFaultGuard;
use crate::tests::native_io_fault_tests::NativeIoPoint;

#[test]
fn test_deleted_process_cwd_preserves_constructor_and_pair_operation_context() {
    const CHILD: &str = "RS_LOCAL_FILES_DELETED_CWD_CONSTRUCTOR_CHILD";
    const TEST: &str = "tests::rooted::rooted_constructor_tests::test_deleted_process_cwd_preserves_constructor_and_pair_operation_context";
    if std::env::var_os(CHILD).is_none() {
        let output = Command::new(std::env::current_exe().expect("test executable should be available"))
            .args(["--exact", TEST, "--test-threads=1", "--nocapture"])
            .env(CHILD, "1")
            .output()
            .expect("isolated child test should start");
        assert!(
            output.status.success(),
            "isolated child failed: stdout={} stderr={}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        assert!(String::from_utf8_lossy(&output.stdout).contains("DELETED_CWD_CONSTRUCTORS_COMPLETE"));
        return;
    }

    let original = std::env::current_dir().expect("child's original cwd should be available");
    let fixture = tempdir().expect("disposable fixture should exist");
    let cwd = fixture.path().join("removed-current-directory");
    fs::create_dir(&cwd).expect("disposable cwd should exist");
    let host = LocalFileSystem::host().expect("Host construction should not query cwd");
    std::env::set_current_dir(&cwd).expect("only this child should enter the disposable cwd");
    fs::remove_dir(&cwd).expect("only the empty disposable child cwd should be removed");

    let requested = Path::new(".");
    let root_result = RootedLocalFileSystem::open_with_contract(
        requested,
        ResolutionContract::unrestricted(),
        LocalSymlinkPolicy::FollowWithinScope,
    );
    let directory_result = host.open_directory_with_options(requested, &LocalDirectoryOpenOptions::new());
    let source = Path::new("relative-source");
    let target = Path::new("relative-target");
    let copy_result = host.copy(source, target);
    let rename_result = host.rename(source, target);
    // Restore a live cwd before assertions and automatic fixture cleanup.
    std::env::set_current_dir(&original).expect("child should restore its original cwd");

    let root_error = root_result.expect_err("opening a relative root requires a readable process cwd");
    let directory_error = directory_result.expect_err("opening a relative Host directory requires a readable cwd");
    for error in [&root_error, &directory_error] {
        assert_eq!(error.operation(), LocalFileOperation::OpenRoot);
        assert_eq!(error.path(), Some(requested));
        assert_eq!(error.io_error_kind(), std::io::ErrorKind::NotFound);
        assert!(error.io_error().is_some());
    }

    let copy = copy_result.expect_err("relative copy operands require a process cwd snapshot");
    let rename = rename_result.expect_err("relative rename operands require a process cwd snapshot");
    for (error, operation) in [
        (copy.error(), LocalFileOperation::Copy),
        (rename.error(), LocalFileOperation::Rename),
    ] {
        assert_eq!(error.operation(), operation);
        assert_eq!(error.path(), Some(source));
        assert_eq!(error.target(), Some(target));
        assert_eq!(error.current_directory(), None);
        assert_eq!(error.io_error_kind(), std::io::ErrorKind::NotFound);
    }
    println!("DELETED_CWD_CONSTRUCTORS_COMPLETE");
}

#[test]
fn test_native_root_constructor_cursor_failure_retains_requested_root() {
    let path = Path::new("/");
    // Opening the native root has no relative components, so the first cursor is
    // constructor-owned.
    let guard = NativeIoFaultGuard::fail_on(NativeIoPoint::OpenCursor, 1);
    let error = RootedLocalFileSystem::open_with_contract(
        path,
        ResolutionContract::unrestricted(),
        LocalSymlinkPolicy::FollowWithinScope,
    )
    .expect_err("the root constructor must propagate cursor creation failure");

    assert!(guard.fired());
    assert_eq!(error.operation(), LocalFileOperation::OpenRoot);
    assert_eq!(error.path(), Some(path));
    assert_eq!(error.io_error_kind(), std::io::ErrorKind::Other);
    assert_eq!(
        error
            .io_error()
            .expect("native cursor source should survive")
            .to_string(),
        "injected native I/O contract failure"
    );
    drop(guard);
    RootedLocalFileSystem::open(path).expect("dropping the fault guard should restore normal root construction");
}
