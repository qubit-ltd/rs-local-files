// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Capability snapshot behavior tests.

use qubit_local_files::LocalFileSystem;

/// Verifies operation capability queries remain coherent on the host target.
#[test]
fn test_local_file_system_capabilities_report_operation_support() {
    let capabilities = LocalFileSystem::host()
        .expect("Host filesystem should open")
        .capabilities();

    assert!(capabilities.supports_rooted_operations());
    assert!(capabilities.supports_atomic_rename());
    assert!(capabilities.supports_atomic_replace());
    assert_eq!(
        cfg!(any(target_os = "linux", target_os = "macos", windows)),
        capabilities.can_attempt_atomic_temp_persist(),
    );
    assert_eq!(cfg!(unix), capabilities.supports_durable_rename());
    assert_eq!(cfg!(unix), capabilities.supports_durable_file_copy());
    assert_eq!(cfg!(unix), capabilities.supports_durable_write());
}

/// Verifies the compatibility query delegates to the explicitly conditional
/// atomic-persistence query.
#[test]
#[allow(deprecated)]
fn test_atomic_temp_persist_compatibility_query_matches_attempt_capability() {
    let capabilities = LocalFileSystem::host()
        .expect("Host filesystem should open")
        .capabilities();

    assert_eq!(
        capabilities.can_attempt_atomic_temp_persist(),
        capabilities.supports_atomic_temp_persist(),
    );
}
