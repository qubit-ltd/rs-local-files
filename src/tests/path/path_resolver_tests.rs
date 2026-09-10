// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Namespace path normalization contract tests.

use std::path::Path;

use crate::error::LocalFileErrorKind;
use crate::path::LocalFileSystemScope;
use crate::path::LocalPathResolver;

#[test]
fn test_rooted_paths_use_virtual_absolute_and_pwd_semantics() {
    let resolver = LocalPathResolver::new(LocalFileSystemScope::Rooted, Path::new("/work/project"))
        .expect("rooted resolver should accept virtual absolute PWD");

    assert_resolution(&resolver, Path::new(""), "/work/project", "work/project");
    assert_resolution(&resolver, Path::new("."), "/work/project", "work/project");
    assert_resolution(
        &resolver,
        Path::new("data.db"),
        "/work/project/data.db",
        "work/project/data.db",
    );
    assert_resolution(&resolver, Path::new("a/./b"), "/work/project/a/b", "work/project/a/b");
    assert_resolution(&resolver, Path::new("a/../b"), "/work/project/b", "work/project/b");
    assert_resolution(&resolver, Path::new("../../tmp"), "/tmp", "tmp");
    assert_resolution(&resolver, Path::new("/etc/hosts"), "/etc/hosts", "etc/hosts");
}

#[test]
fn test_rooted_paths_reject_virtual_root_escape() {
    let root = LocalPathResolver::new(LocalFileSystemScope::Rooted, Path::new("/"))
        .expect("rooted resolver should accept virtual root PWD");
    for path in ["..", "a/./.././../b"] {
        let error = root
            .resolve(Path::new(path))
            .expect_err("parent traversal beyond virtual root must fail");
        assert_eq!(error.kind(), LocalFileErrorKind::InvalidPath);
    }

    let nested = LocalPathResolver::new(LocalFileSystemScope::Rooted, Path::new("/work/project"))
        .expect("rooted resolver should accept nested PWD");
    let error = nested
        .resolve(Path::new("../../../tmp"))
        .expect_err("relative traversal beyond virtual root must fail");
    assert_eq!(error.kind(), LocalFileErrorKind::InvalidPath);
}

#[test]
fn test_rooted_path_resolver_rejects_relative_current_directory() {
    let error = LocalPathResolver::new(LocalFileSystemScope::Rooted, Path::new("relative"))
        .expect_err("Rooted current directories must be virtual absolute paths");

    assert_eq!(LocalFileErrorKind::InvalidPath, error.kind());
}

#[test]
fn test_rooted_resolver_preserves_directory_intent() {
    let resolver =
        LocalPathResolver::new(LocalFileSystemScope::Rooted, Path::new("/")).expect("rooted resolver should open");
    assert!(resolver.resolve(Path::new("missing/")).unwrap().directory_required());
    assert!(resolver.resolve(Path::new("a/.")).unwrap().directory_required());
    assert!(resolver.resolve(Path::new("a/..")).unwrap().directory_required());
    assert!(!resolver.resolve(Path::new("missing")).unwrap().directory_required());
}

#[cfg(unix)]
#[test]
fn test_resolver_preserves_non_utf8_normal_components() {
    use std::ffi::OsString;
    use std::os::unix::ffi::OsStringExt;
    use std::path::PathBuf;

    let resolver =
        LocalPathResolver::new(LocalFileSystemScope::Rooted, Path::new("/")).expect("rooted resolver should open");
    let component = OsString::from_vec(vec![0x66, 0x80]);
    let input = PathBuf::from(&component);
    let resolved = resolver
        .resolve(&input)
        .expect("non-UTF-8 component should remain valid");
    assert_eq!(resolved.authority_relative(), input);
    assert_eq!(resolved.namespace_absolute().file_name(), Some(component.as_os_str()));
}

#[cfg(unix)]
#[test]
fn test_host_relative_paths_bind_to_instance_pwd() {
    let resolver = LocalPathResolver::new(LocalFileSystemScope::Host, Path::new("/srv/app"))
        .expect("host resolver should accept absolute PWD");
    assert_resolution(
        &resolver,
        Path::new("etc/hosts"),
        "/srv/app/etc/hosts",
        "/srv/app/etc/hosts",
    );
    assert_resolution(&resolver, Path::new("/etc/hosts"), "/etc/hosts", "/etc/hosts");
}

#[cfg(unix)]
#[test]
fn test_host_binding_preserves_raw_spelling_and_non_utf8() {
    use std::ffi::OsStr;
    use std::os::unix::ffi::OsStrExt;

    let resolver = LocalPathResolver::new(LocalFileSystemScope::Host, Path::new("/base")).expect("host resolver");
    let input = Path::new(OsStr::from_bytes(b"raw/./\xff/../leaf/"));
    let bound = resolver.resolve(input).expect("raw relative binding");
    assert_eq!(
        bound.authority_relative().as_os_str().as_bytes(),
        b"/base/raw/./\xff/../leaf/"
    );
    let absolute = Path::new(OsStr::from_bytes(b"/raw/./\xff/../leaf/"));
    assert_eq!(
        resolver
            .resolve(absolute)
            .expect("absolute binding")
            .authority_relative()
            .as_os_str(),
        absolute.as_os_str()
    );
}

#[cfg(windows)]
#[test]
fn test_host_windows_drive_binding() {
    let resolver = LocalPathResolver::new(LocalFileSystemScope::Host, Path::new(r"C:\base")).expect("drive resolver");
    assert_eq!(
        resolver
            .resolve(Path::new(r"\dir\..\file"))
            .expect("root relative")
            .authority_relative()
            .as_os_str(),
        Path::new(r"C:\dir\..\file").as_os_str()
    );
    assert!(resolver.resolve(Path::new("C:file")).is_err());
    let verbatim =
        LocalPathResolver::new(LocalFileSystemScope::Host, Path::new(r"\\?\C:\base")).expect("verbatim resolver");
    assert_eq!(
        verbatim
            .resolve(Path::new(r"dir\..\file"))
            .expect("verbatim relative")
            .authority_relative()
            .as_os_str(),
        Path::new(r"\\?\C:\base\dir\..\file").as_os_str()
    );
}

fn assert_resolution(resolver: &LocalPathResolver, input: &Path, logical: &str, backend: &str) {
    let resolved = resolver.resolve(input).expect("path should normalize");
    assert_eq!(resolved.namespace_absolute(), Path::new(logical));
    assert_eq!(resolved.authority_relative(), Path::new(backend));
}
