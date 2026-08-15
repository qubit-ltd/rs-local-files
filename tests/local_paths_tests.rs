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

use qubit_local_files::LocalFileErrorKind;
use qubit_local_files::LocalFileErrorSource;
use qubit_local_files::LocalFileNames;
use qubit_local_files::LocalFileOperation;
use qubit_local_files::LocalFileSystemScope;
use qubit_local_files::LocalPaths;

/// Verifies rooted path objects preserve authority-relative components and
/// reject native roots.
#[test]
fn test_rooted_paths_reject_native_roots_and_round_trip_components() {
    let paths = LocalPaths::rooted();
    let native = paths
        .from_canonical_components(["reports", "2026", "a.txt"])
        .expect("rooted canonical components should decode");
    assert!(native.is_relative());
    assert_eq!(
        paths
            .to_canonical_components(&native)
            .expect("rooted native components should encode"),
        ["reports", "2026", "a.txt"],
    );
    assert!(paths.to_canonical_components(Path::new("/escape")).is_err());
}

/// Verifies canonical component decoders accept iterators without requiring a
/// caller-owned vector.
#[test]
fn test_canonical_component_decoders_accept_iterators() {
    let relative = LocalPaths::rooted()
        .from_canonical_components(["safe", "a%25b"])
        .expect("relative iterator components should decode");
    assert_eq!(Path::new("safe/a%b"), relative);

    #[cfg(unix)]
    {
        let absolute = LocalPaths::host()
            .from_canonical_components(["tmp", "safe"])
            .expect("absolute iterator components should decode");
        assert_eq!(Path::new("/tmp/safe"), absolute);
    }
}

/// Verifies Unix absolute canonical components round-trip through native paths.
#[cfg(unix)]
#[test]
fn test_host_canonical_components_round_trip_unix_path() {
    let paths = LocalPaths::host();
    let native = paths
        .from_canonical_components(["tmp", "a%25b"])
        .expect("canonical absolute path should decode");
    assert_eq!(native, Path::new("/tmp/a%b"));
    assert_eq!(
        paths
            .to_canonical_components(&native)
            .expect("native absolute path should encode"),
        vec!["tmp".to_owned(), "a%25b".to_owned()],
    );
}

/// Verifies Windows drive-rooted canonical components round-trip through native
/// paths.
#[cfg(windows)]
#[test]
fn test_host_canonical_components_round_trip_windows_drive_path() {
    let paths = LocalPaths::host();
    let native = paths
        .from_canonical_components(["C:", "work", "file"])
        .expect("canonical Windows absolute path should decode");
    assert_eq!(native, Path::new(r"C:\work\file"));
    assert_eq!(
        paths
            .to_canonical_components(&native)
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
    let error = LocalPaths::host()
        .to_canonical_components(path)
        .expect_err("unsupported Windows root authority must be rejected");
    assert!(matches!(
        error.kind(),
        LocalFileErrorKind::Unsupported | LocalFileErrorKind::InvalidPath,
    ));
}

/// Verifies canonical relative paths cannot escape through a parent component.
#[test]
fn test_rooted_canonical_components_reject_parent_escape() {
    let error = LocalPaths::rooted()
        .from_canonical_components(["safe", ".."])
        .expect_err("parent traversal must be rejected");

    assert_eq!(LocalFileOperation::ComposePath, error.operation());
}

/// Verifies malformed canonical text retains the path-codec cause rather than
/// degrading it to an untyped invalid-input error.
#[test]
fn test_rooted_canonical_components_retain_path_codec_failure() {
    let error = LocalPaths::rooted()
        .from_canonical_components(["bad%"])
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
        LocalPaths::host()
            .from_canonical_components(["a", "%2F"])
            .is_err()
    );
}

/// Verifies canonical relative components round-trip through native paths.
#[test]
fn test_rooted_canonical_components_round_trip() {
    let paths = LocalPaths::rooted();
    let native = paths
        .from_canonical_components(["safe", "a%25b"])
        .expect("canonical relative path should decode");
    assert_eq!(native, Path::new("safe/a%b"));
    assert_eq!(
        paths
            .to_canonical_components(&native)
            .expect("native relative path should encode"),
        vec!["safe".to_owned(), "a%25b".to_owned()],
    );
}

/// Verifies an empty rooted component sequence represents the opened root.
#[test]
fn test_rooted_canonical_components_round_trip_authority_root() {
    let paths = LocalPaths::rooted();
    let native = paths
        .from_canonical_components(std::iter::empty::<&str>())
        .expect("empty rooted components should decode to the authority root");
    assert!(native.as_os_str().is_empty());
    assert_eq!(
        Vec::<String>::new(),
        paths
            .to_canonical_components(&native)
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
    let error = LocalPaths::rooted()
        .to_canonical_components(&native)
        .expect_err("native NUL must be reported as a path error");

    assert_eq!(LocalFileErrorKind::InvalidPath, error.kind());
    assert_eq!(LocalFileOperation::ComposePath, error.operation());
    assert!(matches!(
        error.typed_source(),
        Some(LocalFileErrorSource::PathCodec(_))
    ));
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
    let names = LocalFileNames::portable();
    let error = names
        .validate(OsStr::new("CON.txt"))
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
            names.validate(OsStr::new(invalid)).is_err(),
            "expected portable name to be rejected: {invalid:?}",
        );
    }
    assert!(names.validate(OsStr::new(&"x".repeat(256))).is_err());
}

/// Verifies path values expose their bound scope and native filename policy.
#[test]
fn test_local_paths_expose_scope_and_native_file_names() {
    let host = LocalPaths::host();
    let rooted = LocalPaths::rooted();

    assert_eq!(LocalFileSystemScope::Host, host.scope());
    assert_eq!(LocalFileSystemScope::Rooted, rooted.scope());
    assert!(
        host.file_names()
            .validate(OsStr::new("native-name"))
            .is_ok()
    );
}

/// Verifies native Unix random-name affixes retain non-UTF-8 bytes.
#[cfg(unix)]
#[test]
fn test_native_unix_random_name_preserves_non_utf8_affixes() {
    use std::os::unix::ffi::OsStrExt;

    let prefix = OsStr::from_bytes(b"prefix-\x80-");
    let suffix = OsStr::from_bytes(b"-\x81.tmp");
    let name = LocalPaths::host()
        .file_names()
        .random_name_with(Some(prefix), Some(suffix))
        .expect("native non-UTF-8 affixes should be preserved");

    assert!(name.as_bytes().starts_with(prefix.as_bytes()));
    assert!(name.as_bytes().ends_with(suffix.as_bytes()));
}

/// Verifies rooted path conversion preserves Unix non-UTF-8 components.
#[cfg(unix)]
#[test]
fn test_rooted_paths_round_trip_unix_non_utf8_component() {
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::ffi::OsStringExt;

    let paths = LocalPaths::rooted();
    let native = PathBuf::from(OsString::from_vec(vec![b'a', 0x80, b'b']));
    let canonical = paths
        .to_canonical_components(&native)
        .expect("Unix non-UTF-8 component should encode");
    let decoded = paths
        .from_canonical_components(canonical.iter().map(String::as_str))
        .expect("canonical Unix component should decode");

    assert_eq!(
        native.as_os_str().as_bytes(),
        decoded.as_os_str().as_bytes()
    );
}

/// Verifies rooted path conversion preserves Windows unpaired surrogates
/// through canonical WTF-8.
#[cfg(windows)]
#[test]
fn test_rooted_paths_round_trip_windows_unpaired_surrogate() {
    use std::ffi::OsString;
    use std::os::windows::ffi::OsStrExt;
    use std::os::windows::ffi::OsStringExt;

    let paths = LocalPaths::rooted();
    let native = PathBuf::from(OsString::from_wide(&[0x0061, 0xD800, 0x0062]));
    let canonical = paths
        .to_canonical_components(&native)
        .expect("Windows unpaired surrogate component should encode");
    let decoded = paths
        .from_canonical_components(canonical.iter().map(String::as_str))
        .expect("canonical WTF-8 component should decode");

    assert_eq!(
        native.as_os_str().encode_wide().collect::<Vec<_>>(),
        decoded.as_os_str().encode_wide().collect::<Vec<_>>(),
    );
}

/// Verifies rooted path conversion rejects Windows drive-relative prefixes.
#[cfg(windows)]
#[test]
fn test_rooted_paths_reject_windows_drive_relative_prefix() {
    let paths = LocalPaths::rooted();

    assert!(
        paths
            .to_canonical_components(Path::new(r"C:escape"))
            .is_err()
    );
    assert!(paths.from_canonical_components(["C:escape"]).is_err());
}
