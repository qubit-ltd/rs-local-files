// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::ffi::OsStr;
#[cfg(unix)]
use std::ffi::OsString;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Mutex;

use qubit_local_files::LocalFileErrorKind;
use qubit_local_files::LocalFileErrorSource;
use qubit_local_files::LocalFileNames;
use qubit_local_files::LocalFileOperation;
use qubit_local_files::LocalFileSystemScope;
use qubit_local_files::LocalPaths;

/// Serializes current-directory-sensitive assertions within this test target.
static CURRENT_DIRECTORY_LOCK: Mutex<()> = Mutex::new(());

/// Verifies that a group of relative host paths is bound against one cwd
/// snapshot.
#[test]
fn test_local_paths_bind_host_paths_uses_absolute_paths() {
    let _lock = CURRENT_DIRECTORY_LOCK
        .lock()
        .expect("current-directory test lock should be available");
    let [source, target] =
        LocalPaths::bind_host_paths([Path::new("source"), Path::new("target")])
            .expect("relative paths should bind against the current directory");

    assert!(source.is_absolute());
    assert!(target.is_absolute());
    assert_eq!(source.parent(), target.parent());
}

/// Verifies canonical component decoders accept iterators without requiring a
/// caller-owned vector.
#[test]
fn test_canonical_component_decoders_accept_iterators() {
    let relative = LocalPaths::from_canonical_components(
        LocalFileSystemScope::Rooted,
        ["safe", "a%25b"],
    )
    .expect("relative iterator components should decode");
    assert_eq!(Path::new("safe/a%b"), relative);

    #[cfg(unix)]
    {
        let absolute = LocalPaths::from_canonical_components(
            LocalFileSystemScope::Host,
            ["tmp", "safe"],
        )
        .expect("absolute iterator components should decode");
        assert_eq!(Path::new("/tmp/safe"), absolute);
    }
}

/// Verifies Unix absolute canonical components round-trip through native paths.
#[cfg(unix)]
#[test]
fn test_host_canonical_components_round_trip_unix_path() {
    let native = LocalPaths::from_canonical_components(
        LocalFileSystemScope::Host,
        ["tmp", "a%25b"],
    )
    .expect("canonical absolute path should decode");
    assert_eq!(native, Path::new("/tmp/a%b"));
    assert_eq!(
        LocalPaths::to_canonical_components(
            LocalFileSystemScope::Host,
            &native
        )
        .expect("native absolute path should encode"),
        vec!["tmp".to_owned(), "a%25b".to_owned()],
    );
}

/// Verifies Windows drive-rooted canonical components round-trip through native
/// paths.
#[cfg(windows)]
#[test]
fn test_host_canonical_components_round_trip_windows_drive_path() {
    let native = LocalPaths::from_canonical_components(
        LocalFileSystemScope::Host,
        ["C:", "work", "file"],
    )
    .expect("canonical Windows absolute path should decode");
    assert_eq!(native, Path::new(r"C:\work\file"));
    assert_eq!(
        LocalPaths::to_canonical_components(
            LocalFileSystemScope::Host,
            &native
        )
        .expect("native Windows absolute path should encode"),
        vec!["C:".to_owned(), "work".to_owned(), "file".to_owned(),],
    );
}

/// Verifies Windows absolute conversion rejects unsupported root authorities.
#[cfg(windows)]
#[test]
fn test_host_canonical_components_rejects_windows_unsupported_roots() {
    assert_windows_unsupported_absolute_path(Path::new(r"\\server\share\file"));
    assert_windows_unsupported_absolute_path(Path::new(r"\\?\C:\work\file"));
    assert_windows_unsupported_absolute_path(Path::new(r"\work\file"));
}

/// Asserts that a Windows root form cannot become a canonical host absolute
/// path.
///
/// # Parameters
///
/// - `path`: Windows native path using an unsupported root authority.
///
/// # Panics
///
/// Panics when conversion unexpectedly succeeds or returns an unrelated error
/// kind.
#[cfg(windows)]
fn assert_windows_unsupported_absolute_path(path: &Path) {
    let error =
        LocalPaths::to_canonical_components(LocalFileSystemScope::Host, path)
            .expect_err("unsupported Windows root authority must be rejected");
    assert!(matches!(
        error.kind(),
        LocalFileErrorKind::Unsupported | LocalFileErrorKind::InvalidPath,
    ));
}

/// Verifies canonical relative paths cannot escape through a parent component.
#[test]
fn test_rooted_canonical_components_reject_parent_escape() {
    let error = LocalPaths::from_canonical_components(
        LocalFileSystemScope::Rooted,
        vec!["safe", ".."],
    )
    .expect_err("parent traversal must be rejected");

    assert_eq!(LocalFileOperation::ComposePath, error.operation());
}

/// Verifies malformed canonical text retains the path-codec cause rather than
/// degrading it to an untyped invalid-input error.
#[test]
fn test_rooted_canonical_components_retain_path_codec_failure() {
    let error = LocalPaths::from_canonical_components(
        LocalFileSystemScope::Rooted,
        vec!["bad%"],
    )
    .expect_err("malformed percent escape must be rejected");

    assert_eq!(LocalFileErrorKind::InvalidPath, error.kind());
    assert_eq!(LocalFileOperation::ComposePath, error.operation());
    assert!(matches!(
        error.typed_source(),
        Some(LocalFileErrorSource::PathCodec(_))
    ));
}

/// Verifies Host canonical components reject authority-changing components.
#[test]
fn test_absolute_conversion_rejects_relative_shape() {
    assert!(
        LocalPaths::from_canonical_components(
            LocalFileSystemScope::Host,
            vec!["a", "%2F"],
        )
        .is_err()
    );
}

/// Verifies canonical relative components round-trip through native paths.
#[test]
fn test_rooted_canonical_components_round_trip() {
    let native = LocalPaths::from_canonical_components(
        LocalFileSystemScope::Rooted,
        vec!["safe", "a%25b"],
    )
    .expect("canonical relative path should decode");
    assert_eq!(native, Path::new("safe/a%b"));
    assert_eq!(
        LocalPaths::to_canonical_components(
            LocalFileSystemScope::Rooted,
            &native
        )
        .expect("native relative path should encode"),
        vec!["safe".to_owned(), "a%25b".to_owned()],
    );
}

/// Verifies an empty rooted component sequence represents the opened root.
#[test]
fn test_rooted_canonical_components_round_trip_authority_root() {
    let native = LocalPaths::from_canonical_components(
        LocalFileSystemScope::Rooted,
        std::iter::empty::<&str>(),
    )
    .expect("empty rooted components should decode to the authority root");
    assert!(native.as_os_str().is_empty());
    assert_eq!(
        Vec::<String>::new(),
        LocalPaths::to_canonical_components(
            LocalFileSystemScope::Rooted,
            &native,
        )
        .expect("the rooted authority root should encode as empty"),
    );
}

/// Verifies native NUL components become structured path-codec errors rather
/// than panicking during public canonicalization.
#[cfg(unix)]
#[test]
fn test_rooted_canonical_components_reject_native_nul() {
    use std::os::unix::ffi::OsStringExt;

    let native =
        PathBuf::from(OsString::from_vec(vec![b's', 0, b'a', b'f', b'e']));
    let error = LocalPaths::to_canonical_components(
        LocalFileSystemScope::Rooted,
        &native,
    )
    .expect_err("native NUL must be reported as a path error");

    assert_eq!(LocalFileErrorKind::InvalidPath, error.kind());
    assert_eq!(LocalFileOperation::ComposePath, error.operation());
    assert!(matches!(
        error.typed_source(),
        Some(LocalFileErrorSource::PathCodec(_))
    ));
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

    assert_eq!(LocalFileErrorKind::InvalidPath, error.kind());
    assert_eq!(LocalFileOperation::ComposePath, error.operation());
}

/// Verifies that native filename access does not require UTF-8 conversion.
#[test]
fn test_local_file_names_returns_native_components() {
    let path = PathBuf::from("archive.tar.gz");

    assert_eq!(Some(OsStr::new("archive.tar.gz")), path.file_name());
    assert_eq!(Some(OsStr::new("archive.tar")), path.file_stem());
    assert_eq!(Some(OsStr::new("archive")), path.file_prefix());
    assert_eq!(Some(OsStr::new("gz")), path.extension());
}

/// Verifies conservative portable filename validation.
#[test]
fn test_local_file_names_rejects_reserved_portable_name() {
    let error = LocalFileNames::validate_portable(OsStr::new("CON.txt"))
        .expect_err("Windows reserved names must be rejected");

    assert_eq!(LocalFileErrorKind::InvalidPath, error.kind());
    assert_eq!(LocalFileOperation::ValidateName, error.operation());

    for invalid in [
        "",
        ".",
        "..",
        "name ",
        "name.",
        "name\n",
        "name/name",
        "name\\name",
        "name<name",
        "name>name",
        "name:name",
        "name\"name",
        "name|name",
        "name?name",
        "name*name",
        "COM1",
        "lpt³.log",
    ] {
        assert!(
            LocalFileNames::validate_portable(OsStr::new(invalid)).is_err(),
            "expected portable name to be rejected: {invalid:?}",
        );
    }
    assert!(
        LocalFileNames::validate_portable(OsStr::new(&"x".repeat(256)))
            .is_err()
    );
}
