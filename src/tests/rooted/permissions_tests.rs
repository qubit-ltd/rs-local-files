// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Crate-private contract tests for rooted `Permissions`.

use crate::rooted::Permissions;

#[test]
fn test_rooted_permissions_resolve_portable_and_unix_permissions() {
    let read_only = Permissions::from_read_only(true);
    assert!(read_only.is_read_only());
    assert_eq!(read_only.unix_mode(), None);
    #[cfg(unix)]
    assert_eq!(read_only.resolve_unix_mode(0o777), 0o555);

    let writable = Permissions::from_read_only(false);
    assert!(!writable.is_read_only());
    #[cfg(unix)]
    assert_eq!(writable.resolve_unix_mode(0o444), 0o644);

    let exact = Permissions::from_unix_mode(0o17777);
    assert_eq!(exact.unix_mode(), Some(0o7777));
    assert!(!exact.is_read_only());
    #[cfg(unix)]
    assert_eq!(exact.resolve_unix_mode(0), 0o7777);
}
