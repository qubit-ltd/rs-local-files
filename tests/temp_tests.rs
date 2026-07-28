// =============================================================================

#![cfg(coverage)]
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

/// Verifies temporary directories are exposed through the responsibility
/// module.
#[test]
fn test_temp_directory_is_created() {
    let directory = qubit_local_files::temp::TempDir::new()
        .expect("a temporary directory should exist");
    assert!(directory.path().is_dir());
}
