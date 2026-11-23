// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Coverage for dynamic filesystem space values.

use qubit_local_files::capability::LocalFileSystemSpace;

/// Verifies space fields retain independently known observations.
#[test]
fn test_local_file_system_space_preserves_independent_values() {
    let space = LocalFileSystemSpace::new(Some(100), Some(80), Some(50));

    assert_eq!(Some(100), space.capacity_bytes());
    assert_eq!(Some(80), space.free_bytes());
    assert_eq!(Some(50), space.available_bytes());

    let unknown_capacity = LocalFileSystemSpace::new(None, Some(80), Some(50));
    assert_eq!(None, unknown_capacity.capacity_bytes());
    assert_eq!(Some(80), unknown_capacity.free_bytes());
    assert_eq!(Some(50), unknown_capacity.available_bytes());

    let unknown_free = LocalFileSystemSpace::new(Some(100), None, Some(50));
    assert_eq!(Some(100), unknown_free.capacity_bytes());
    assert_eq!(None, unknown_free.free_bytes());
    assert_eq!(Some(50), unknown_free.available_bytes());

    let unknown_available = LocalFileSystemSpace::new(Some(100), Some(80), None);
    assert_eq!(Some(100), unknown_available.capacity_bytes());
    assert_eq!(Some(80), unknown_available.free_bytes());
    assert_eq!(None, unknown_available.available_bytes());
}
