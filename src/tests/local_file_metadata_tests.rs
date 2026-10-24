// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Crate-local contract tests for normalized metadata values.

use std::time::SystemTime;
use std::time::UNIX_EPOCH;

use crate::LocalFileKind;
use crate::LocalFileMetadata;
use crate::LocalFilePermissions;

#[test]
fn test_metadata_accessors_preserve_native_observations() {
    let accessed_at = UNIX_EPOCH + std::time::Duration::from_secs(1);
    let modified_at = UNIX_EPOCH + std::time::Duration::from_secs(2);
    let created_at = UNIX_EPOCH + std::time::Duration::from_secs(3);
    let permissions = LocalFilePermissions::new(true, Some(0o444));
    let metadata = LocalFileMetadata::from_parts(
        LocalFileKind::File,
        7,
        Some(accessed_at),
        Some(modified_at),
        Some(created_at),
        permissions,
    );

    assert_eq!(LocalFileKind::File, metadata.kind());
    assert_eq!(7, metadata.len());
    assert!(!metadata.is_empty());
    assert_eq!(Some(accessed_at), metadata.accessed_at());
    assert_eq!(Some(modified_at), metadata.modified_at());
    assert_eq!(Some(created_at), metadata.created_at());
    assert_eq!(permissions, metadata.permissions());
    assert!(metadata.permissions().is_read_only());
    assert_eq!(Some(0o444), metadata.permissions().unix_mode());
}

#[test]
fn test_empty_metadata_and_platform_independent_permissions() {
    let metadata = LocalFileMetadata::from_parts(
        LocalFileKind::Other,
        0,
        None,
        None,
        None,
        LocalFilePermissions::new(false, None),
    );

    assert!(metadata.is_empty());
    assert_eq!(0, metadata.len());
    assert_eq!(None::<SystemTime>, metadata.accessed_at());
    assert_eq!(None::<SystemTime>, metadata.modified_at());
    assert_eq!(None::<SystemTime>, metadata.created_at());
    assert!(!metadata.permissions().is_read_only());
    assert_eq!(None, metadata.permissions().unix_mode());
}
