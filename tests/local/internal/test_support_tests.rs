// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::fs;

use qubit_local_files::{
    LocalDeleteOptions,
    LocalFileErrorKind,
    LocalFileSystem,
};
use tempfile::tempdir;

/// Verifies a selected native fault is isolated to a child test process.
#[cfg(feature = "internal-test-support")]
#[test]
fn test_test_support_injects_selected_fault_only_in_child_process() {
    const FAULT_ENV: &str = "QUBIT_LOCAL_FILES_TEST_FAULT";
    const CHILD_ENV: &str = "QUBIT_LOCAL_FILES_TEST_FAULT_CHILD";
    const FAULT: &str = "local-fs-delete-file-remove";
    const TEST_NAME: &str = "local::internal::test_support_tests::test_test_support_injects_selected_fault_only_in_child_process";

    match std::env::var(CHILD_ENV).as_deref() {
        Ok("normal") => {
            let directory =
                tempdir().expect("temporary directory should be created");
            let file = directory.path().join("payload");
            fs::write(&file, b"payload").expect("fixture should be written");

            let _ = LocalFileSystem::host()
                .delete_file(&file, &LocalDeleteOptions::new())
                .expect("deletion without selector should succeed");
            return;
        }
        Ok("fault") => {
            let _fault = qubit_local_files::install_test_fault(FAULT)
                .expect("fault controller should install");
            let directory =
                tempdir().expect("temporary directory should be created");
            let file = directory.path().join("payload");
            fs::write(&file, b"payload").expect("fixture should be written");

            let error = LocalFileSystem::host()
                .delete_file(&file, &LocalDeleteOptions::new())
                .expect_err("selected fault should fail deletion");
            assert_eq!(LocalFileErrorKind::Io, error.kind());
            return;
        }
        Ok(mode) => panic!("unexpected child test mode: {mode}"),
        Err(std::env::VarError::NotPresent) => {}
        Err(std::env::VarError::NotUnicode(_)) => {
            panic!("child test mode must be valid UTF-8")
        }
    }

    let executable =
        std::env::current_exe().expect("test executable should be available");
    let normal_status = std::process::Command::new(&executable)
        .arg("--exact")
        .arg(TEST_NAME)
        .arg("--nocapture")
        .env_remove(FAULT_ENV)
        .env(CHILD_ENV, "normal")
        .status()
        .expect("normal child should launch");
    assert!(normal_status.success(), "normal child should pass");

    let fault_status = std::process::Command::new(executable)
        .arg("--exact")
        .arg(TEST_NAME)
        .arg("--nocapture")
        .env(FAULT_ENV, FAULT)
        .env(CHILD_ENV, "fault")
        .status()
        .expect("fault child should launch");
    assert!(fault_status.success(), "fault child should pass");
}

/// Verifies that explicit fault guards control and then release one fault.
#[cfg(feature = "internal-test-support")]
#[test]
fn test_explicit_test_fault_guard_scopes_controller() {
    const FAULT: &str = "local-fs-delete-file-remove";
    let directory = tempdir().expect("temporary directory should be created");
    let file = directory.path().join("payload");
    fs::write(&file, b"payload").expect("fixture should be written");

    {
        let _fault = qubit_local_files::install_test_fault(FAULT)
            .expect("fault controller should install");
        let error = LocalFileSystem::host()
            .delete_file(&file, &LocalDeleteOptions::new())
            .expect_err("explicitly selected fault should fail deletion");
        assert_eq!(LocalFileErrorKind::Io, error.kind());
        assert!(qubit_local_files::install_test_fault("other").is_err());
    }

    let _ = LocalFileSystem::host()
        .delete_file(&file, &LocalDeleteOptions::new())
        .expect("fault controller should be disabled after drop");
}
