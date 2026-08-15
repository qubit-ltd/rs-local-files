//! Internal tests for immutable core fault isolation.

use std::io::ErrorKind;

use crate::LocalFileSystemBuilder;
use crate::TestFaultPlan;
use crate::TestFaultPoint;

#[test]
fn fault_plans_are_instance_local() {
    let failing = LocalFileSystemBuilder::host()
        .test_faults(TestFaultPlan::fail_once(
            TestFaultPoint::Metadata,
            ErrorKind::Other,
        ))
        .build()
        .expect("faulted filesystem");
    let healthy = LocalFileSystemBuilder::host()
        .build()
        .expect("healthy filesystem");

    assert!(
        failing
            .core
            .fail_if_requested(TestFaultPoint::Metadata)
            .is_err()
    );
    assert!(
        healthy
            .core
            .fail_if_requested(TestFaultPoint::Metadata)
            .is_ok()
    );
}
