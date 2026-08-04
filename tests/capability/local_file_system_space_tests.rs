// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Coverage for dynamic filesystem space values.

use qubit_local_files::LocalFileSystemSpace;

/// Verifies space fields retain independently known observations.
#[test]
fn test_local_file_system_space_preserves_independent_values() {
    let space = LocalFileSystemSpace::new(Some(100), None, Some(50));

    assert_eq!(Some(100), space.capacity_bytes());
    assert_eq!(None, space.free_bytes());
    assert_eq!(Some(50), space.available_bytes());
}
