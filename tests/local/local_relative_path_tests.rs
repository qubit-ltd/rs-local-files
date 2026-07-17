// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::io::ErrorKind;
use std::path::Path;

use qubit_local_files::LocalRelativePath;

/// Verifies that normal relative components are retained as the sole path
/// state.
#[test]
fn test_new_accepts_normal_relative_components() {
    let path = LocalRelativePath::new("目录/data.bin")
        .expect("normal relative components should be accepted");

    assert_eq!(Path::new("目录/data.bin"), path.as_path());
}

/// Verifies rejection of every platform-independent non-normal component.
#[test]
fn test_new_rejects_non_normal_paths() {
    for invalid in ["", ".", "./file", "a/./file", "..", "a/../file"] {
        let error = LocalRelativePath::new(invalid)
            .expect_err("non-normal relative path should be rejected");
        assert_eq!(
            ErrorKind::InvalidInput,
            error.kind(),
            "unexpected error for {invalid:?}: {error}",
        );
    }

    let absolute = std::env::temp_dir().join("absolute.bin");
    let error = LocalRelativePath::new(&absolute)
        .expect_err("absolute paths should be rejected");
    assert_eq!(ErrorKind::InvalidInput, error.kind());
}

/// Verifies that Unix byte paths containing NUL are rejected before FFI use.
#[cfg(unix)]
#[test]
fn test_new_rejects_unix_nul() {
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt;

    let path = OsString::from_vec(b"safe\0unsafe".to_vec());
    let error =
        LocalRelativePath::new(path).expect_err("NUL should be rejected");

    assert_eq!(ErrorKind::InvalidInput, error.kind());
}

/// Verifies that Windows wide paths containing NUL are rejected before FFI
/// use.
#[cfg(windows)]
#[test]
fn test_new_rejects_windows_nul() {
    use super::test_support::path_with_interior_nul;

    let path = path_with_interior_nul(Path::new("safe"), "unsafe");
    let error =
        LocalRelativePath::new(path).expect_err("NUL should be rejected");

    assert_eq!(ErrorKind::InvalidInput, error.kind());
}
