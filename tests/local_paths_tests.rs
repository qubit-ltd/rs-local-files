// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::{
    ffi::OsStr,
    path::{
        Path,
        PathBuf,
    },
};

use qubit_local_files::{
    LocalFileErrorKind,
    LocalFileNames,
    LocalFileOperation,
    LocalPaths,
};

/// Verifies that a group of relative host paths is bound against one cwd
/// snapshot.
#[test]
fn test_local_paths_bind_host_paths_uses_absolute_paths() {
    let [source, target] =
        LocalPaths::bind_host_paths([Path::new("source"), Path::new("target")])
            .expect("relative paths should bind against the current directory");

    assert!(source.is_absolute());
    assert!(target.is_absolute());
    assert_eq!(source.parent(), target.parent());
}

/// Verifies lexical containment for normalized native paths.
#[test]
fn test_local_paths_is_lexically_within_accepts_descendant() {
    assert!(
        LocalPaths::is_lexically_within(
            Path::new("/root/a"),
            Path::new("/root")
        )
        .expect("normalized paths should be comparable"),
    );
}

/// Verifies that dot components are rejected instead of silently normalized.
#[test]
fn test_local_paths_is_lexically_within_rejects_dot_components() {
    let error = LocalPaths::is_lexically_within(
        Path::new("/root/../escape"),
        Path::new("/root"),
    )
    .expect_err("parent traversal must be rejected");

    assert_eq!(LocalFileErrorKind::InvalidInput, error.kind());
    assert_eq!(LocalFileOperation::ComposePath, error.operation());
}

/// Verifies that native filename access does not require UTF-8 conversion.
#[test]
fn test_local_file_names_returns_native_components() {
    let path = PathBuf::from("archive.tar.gz");

    assert_eq!(
        Some(OsStr::new("archive.tar.gz")),
        LocalFileNames::file_name(&path)
    );
    assert_eq!(
        Some(OsStr::new("archive.tar")),
        LocalFileNames::file_stem(&path)
    );
    assert_eq!(
        Some(OsStr::new("archive")),
        LocalFileNames::file_prefix(&path)
    );
    assert_eq!(Some(OsStr::new("gz")), LocalFileNames::extension(&path));
}

/// Verifies conservative portable filename validation.
#[test]
fn test_local_file_names_rejects_reserved_portable_name() {
    let error = LocalFileNames::validate_portable(OsStr::new("CON.txt"))
        .expect_err("Windows reserved names must be rejected");

    assert_eq!(LocalFileErrorKind::InvalidInput, error.kind());
    assert_eq!(LocalFileOperation::ValidateName, error.operation());
}
