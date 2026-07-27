// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use qubit_local_files::rooted::Permissions;

/// Verifies portable read-only permissions do not invent Unix mode bits.
#[test]
fn test_rooted_permissions_from_read_only() {
    let permissions = Permissions::from_read_only(true);

    assert!(permissions.is_read_only());
    assert_eq!(None, permissions.unix_mode());
}

/// Verifies Unix modes retain exact portable and special bits.
#[test]
fn test_rooted_permissions_from_unix_mode() {
    let permissions = Permissions::from_unix_mode(0o10_640);

    assert!(!permissions.is_read_only());
    assert_eq!(Some(0o640), permissions.unix_mode());
}
