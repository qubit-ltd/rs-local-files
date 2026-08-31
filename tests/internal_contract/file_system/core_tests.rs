// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Internal tests for immutable core fault isolation.

use std::io::ErrorKind;
use std::path::Path;

use qubit_local_files::LocalFileSystem;
use qubit_local_files::TestFaultPlan;
use qubit_local_files::TestFaultPoint;

#[test]
fn fault_plans_are_instance_local() {
    let failing = LocalFileSystem::host()
        .expect("Host filesystem should open")
        .with_test_faults(Some(TestFaultPlan::fail_once(
            TestFaultPoint::Metadata,
            ErrorKind::Other,
        )));
    let healthy = LocalFileSystem::host().expect("Host filesystem should open");

    assert!(failing.metadata(Path::new("Cargo.toml")).is_err());
    assert!(healthy.metadata(Path::new("Cargo.toml")).is_ok());
}
