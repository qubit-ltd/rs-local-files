// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::time::Duration;

use qubit_local_files::LocalAtomicWriteOptions;

#[test]
fn test_local_atomic_write_options_control_parent_creation() {
    let default_options = LocalAtomicWriteOptions::new();
    let parent_options = default_options.with_parent();
    let timeout_options =
        default_options.with_open_retry_timeout(Duration::ZERO);

    assert!(!default_options.creates_parent());
    assert!(parent_options.creates_parent());
    assert_eq!(None, default_options.open_retry_timeout());
    assert_eq!(Some(Duration::ZERO), timeout_options.open_retry_timeout());
    assert_eq!(default_options, LocalAtomicWriteOptions::default());
}
