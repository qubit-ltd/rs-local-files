//! Internal tests for immutable core fault isolation.

use std::io::ErrorKind;

use crate::LocalFileSystem;
use crate::TestFaultPlan;
use crate::TestFaultPoint;

#[test]
fn fault_plans_are_instance_local() {
    let failing = LocalFileSystem::host()
        .expect("Host filesystem should open")
        .with_test_faults(Some(TestFaultPlan::fail_once(
            TestFaultPoint::Metadata,
            ErrorKind::Other,
        )));
    let healthy = LocalFileSystem::host().expect("Host filesystem should open");

    assert!(failing.core.fail_if_requested(TestFaultPoint::Metadata).is_err());
    assert!(healthy.core.fail_if_requested(TestFaultPoint::Metadata).is_ok());
}
