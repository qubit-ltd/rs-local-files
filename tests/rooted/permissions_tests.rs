// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use qubit_local_files::rooted::{
    Path,
    Permissions,
    Root,
};
#[cfg(unix)]
use std::os::unix::fs::PermissionsExt;

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

/// Verifies writable portable permissions and mode masking retain their
/// distinct public representations.
#[test]
fn test_rooted_permissions_represent_writable_and_masked_modes() {
    let writable = Permissions::from_read_only(false);
    let read_only = Permissions::from_unix_mode(0o10_555);

    assert!(!writable.is_read_only());
    assert_eq!(None, writable.unix_mode());
    assert!(read_only.is_read_only());
    assert_eq!(Some(0o555), read_only.unix_mode());
}

/// Verifies portable permission values resolve against an existing Unix mode
/// when they are applied through a rooted authority.
#[cfg(unix)]
#[test]
fn test_rooted_permissions_apply_portable_read_only_and_writable_updates() {
    let directory = tempfile::tempdir().expect("temporary root should open");
    let native_path = directory.path().join("payload");
    std::fs::write(&native_path, b"payload")
        .expect("payload fixture should be written");
    std::fs::set_permissions(
        &native_path,
        std::fs::Permissions::from_mode(0o641),
    )
    .expect("payload fixture permissions should be set");
    let root =
        Root::open(directory.path()).expect("root authority should open");
    let path =
        Path::new("payload").expect("relative payload path should validate");

    root.set_permissions(&path, Permissions::from_read_only(true))
        .expect("portable read-only permissions should apply");
    assert_eq!(
        0o441,
        std::fs::metadata(&native_path)
            .expect("payload metadata should be readable")
            .permissions()
            .mode()
            & 0o777
    );

    root.set_permissions(&path, Permissions::from_read_only(false))
        .expect("portable writable permissions should apply");
    assert_eq!(
        0o641,
        std::fs::metadata(&native_path)
            .expect("payload metadata should be readable")
            .permissions()
            .mode()
            & 0o777
    );
}

/// Verifies explicitly supplied Unix bits, including special bits, replace
/// the current mode without being reduced to the portable read-only view.
#[cfg(unix)]
#[test]
fn test_rooted_permissions_apply_explicit_unix_mode() {
    let directory = tempfile::tempdir().expect("temporary root should open");
    let native_path = directory.path().join("payload");
    std::fs::write(&native_path, b"payload")
        .expect("payload fixture should be written");
    let root =
        Root::open(directory.path()).expect("root authority should open");
    let path =
        Path::new("payload").expect("relative payload path should validate");

    root.set_permissions(&path, Permissions::from_unix_mode(0o6751))
        .expect("explicit Unix permissions should apply");

    assert_eq!(
        0o6751,
        std::fs::metadata(&native_path)
            .expect("payload metadata should be readable")
            .permissions()
            .mode()
            & 0o7777,
    );
}
