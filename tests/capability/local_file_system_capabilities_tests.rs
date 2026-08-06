// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
// =============================================================================
//! Capability snapshot behavior tests.

use qubit_local_files::LocalFileSystem;

/// Verifies operation capability queries remain coherent on the host target.
#[test]
fn test_local_file_system_capabilities_report_operation_protocols() {
    let capabilities = LocalFileSystem::host().capabilities();

    assert!(capabilities.supports_rooted_operations());
    assert!(capabilities.supports_atomic_rename());
    assert!(capabilities.supports_atomic_replace());
    assert!(capabilities.supports_atomic_temp_persist());
    assert_eq!(cfg!(unix), capabilities.supports_durable_rename());
    assert_eq!(cfg!(unix), capabilities.supports_durable_file_copy());
}
