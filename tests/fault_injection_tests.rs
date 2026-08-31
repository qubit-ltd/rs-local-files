// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Instance-local fault plan behavior.

#![cfg(feature = "test-support")]

use std::io::ErrorKind;

use qubit_local_files::LocalFileSystem;
use qubit_local_files::TestFaultPlan;
use qubit_local_files::TestFaultPoint;

#[test]
fn metadata_fault_does_not_cross_filesystem_instances() {
    let executable = std::env::current_exe().expect("current executable");
    let failing = LocalFileSystem::host()
        .expect("Host filesystem should open")
        .with_test_faults(Some(TestFaultPlan::fail_once(
            TestFaultPoint::Metadata,
            ErrorKind::Other,
        )));
    let healthy = LocalFileSystem::host().expect("Host filesystem should open");

    assert!(failing.metadata(&executable).is_err());
    assert!(healthy.metadata(&executable).is_ok());
}
