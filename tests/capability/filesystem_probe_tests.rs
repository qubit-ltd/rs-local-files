// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
// =============================================================================
//! Public behavior coverage for filesystem probing.

use qubit_local_files::LocalFileSystem;

/// Verifies a probe of a missing path uses its existing ancestor.
#[test]
fn test_filesystem_probe_uses_nearest_existing_ancestor() {
    let root = tempfile::tempdir().expect("temporary root should be created");
    let space = LocalFileSystem::host()
        .space_at(&root.path().join("missing/child"))
        .expect("missing descendants should use the existing root");

    #[cfg(unix)]
    assert!(space.capacity_bytes().is_some());
}
