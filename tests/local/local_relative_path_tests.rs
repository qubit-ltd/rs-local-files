// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::io::ErrorKind;
use std::path::{
    Path,
    PathBuf,
};

use super::api_tests::LocalRelativePath;
use proptest::{
    collection,
    prop_assert_eq,
    proptest,
};

proptest! {
    /// Verifies that arbitrary normal components remain valid and unchanged.
    #[test]
    fn test_new_accepts_generated_normal_components(
        components in collection::vec("[A-Za-z0-9_-]{1,16}", 1..8),
    ) {
        let mut path = PathBuf::new();
        for component in &components {
            path.push(component);
        }

        let relative = LocalRelativePath::new(&path)
            .expect("generated normal components should be accepted");

        prop_assert_eq!(path.as_path(), relative.as_path());
    }

    /// Verifies that generated parent components cannot escape their prefix.
    #[test]
    fn test_new_rejects_generated_parent_components(
        prefix in collection::vec("[A-Za-z0-9_-]{1,16}", 0..8),
        suffix in "[A-Za-z0-9_-]{1,16}",
    ) {
        let mut path = PathBuf::new();
        for component in prefix {
            path.push(component);
        }
        path.push("..");
        path.push(suffix);

        let error = LocalRelativePath::new(path)
            .expect_err("generated parent component should be rejected");

        prop_assert_eq!(ErrorKind::InvalidInput, error.kind());
    }

    /// Verifies that generated interior NUL bytes are rejected before FFI use.
    #[test]
    fn test_new_rejects_generated_nul(component in "[A-Za-z0-9_-]{0,16}") {
        let path = format!("{component}\0tail");
        let error = LocalRelativePath::new(path)
            .expect_err("generated NUL should be rejected");

        prop_assert_eq!(ErrorKind::InvalidInput, error.kind());
    }
}

/// Verifies that normal relative components are retained as the sole path
/// state.
#[test]
fn test_new_accepts_normal_relative_components() {
    let path = LocalRelativePath::new("目录/data.bin")
        .expect("normal relative components should be accepted");

    assert_eq!(Path::new("目录/data.bin"), path.as_path());
}

/// Verifies that validated paths compose normal relative descendants.
#[test]
fn test_join_accepts_normal_relative_descendants() {
    let parent =
        LocalRelativePath::new("parent").expect("the parent should validate");

    let joined = parent
        .join("child/value.bin")
        .expect("the descendant should validate");
    let component = parent
        .join_component(std::ffi::OsStr::new("child"))
        .expect("the child component should validate");

    assert_eq!(Path::new("parent/child/value.bin"), joined.as_path());
    assert_eq!(Path::new("parent/child"), component.as_path());
}

/// Verifies that path composition rejects non-normal descendants.
#[test]
fn test_join_rejects_invalid_descendants() {
    let parent =
        LocalRelativePath::new("parent").expect("the parent should validate");

    for invalid in ["", ".", "..", "../escape", "/absolute"] {
        let error = parent
            .join(invalid)
            .expect_err("an invalid descendant should be rejected");
        assert_eq!(ErrorKind::InvalidInput, error.kind());
    }
    assert!(
        parent
            .join_component(std::ffi::OsStr::new("child/grandchild"))
            .is_err()
    );
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
