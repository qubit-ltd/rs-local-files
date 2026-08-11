// =============================================================================

#[cfg(feature = "internal-test-support")]
use qubit_local_files::install_test_fault;
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

/// Verifies that a fault guard cannot be nested for different selectors.
#[cfg(feature = "internal-test-support")]
#[test]
fn test_test_fault_guard_blocks_nested_installations() {
    let _guard = install_test_fault("root-authority-path")
        .expect("fault controller should be installed");

    assert!(install_test_fault("other").is_err());
}
