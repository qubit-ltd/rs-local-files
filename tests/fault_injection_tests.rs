//! Instance-local fault plan behavior.

#![cfg(feature = "test-support")]

use std::io::ErrorKind;

use qubit_local_files::LocalFileSystem;
use qubit_local_files::LocalFileSystemBuilder;
use qubit_local_files::TestFaultPlan;
use qubit_local_files::TestFaultPoint;

#[test]
fn metadata_fault_does_not_cross_filesystem_instances() {
    let executable = std::env::current_exe().expect("current executable");
    let failing = LocalFileSystemBuilder::host()
        .test_faults(TestFaultPlan::fail_once(TestFaultPoint::Metadata, ErrorKind::Other))
        .build()
        .expect("faulted filesystem");
    let healthy = LocalFileSystem::host();

    assert!(failing.metadata(&executable).is_err());
    assert!(healthy.metadata(&executable).is_ok());
}
