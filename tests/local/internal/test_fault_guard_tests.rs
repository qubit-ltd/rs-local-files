// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

#[cfg(feature = "test-support")]
use qubit_local_files::test_support::install_test_fault;

/// Verifies that a fault guard cannot be nested for different selectors.
#[cfg(feature = "test-support")]
#[test]
fn test_test_fault_guard_blocks_nested_installations() {
    let _guard = install_test_fault("root-authority-path").expect("fault controller should be installed");

    assert!(install_test_fault("other").is_err());
}

/// Verifies independent test threads serialize their process-wide selectors
/// instead of observing a spurious nested-installation failure.
#[cfg(feature = "test-support")]
#[test]
fn test_test_fault_guard_serializes_parallel_test_threads() {
    use std::sync::mpsc;
    use std::time::Duration;

    let first = install_test_fault("first-thread").expect("first controller should install");
    let (attempt_sender, attempt_receiver) = mpsc::channel();
    let (result_sender, result_receiver) = mpsc::channel();
    let thread = std::thread::spawn(move || {
        attempt_sender.send(()).expect("attempt signal should be delivered");
        let result = install_test_fault("second-thread");
        result_sender
            .send(result.map(|_guard| ()))
            .expect("installation result should be delivered");
    });
    attempt_receiver
        .recv()
        .expect("second thread should reach installation");
    assert!(
        result_receiver.recv_timeout(Duration::from_millis(50)).is_err(),
        "another test thread should wait while the controller is owned",
    );

    drop(first);
    assert!(
        result_receiver
            .recv_timeout(Duration::from_secs(1))
            .expect("second thread should resume after release")
            .is_ok(),
    );
    thread.join().expect("parallel controller thread should finish");
}
