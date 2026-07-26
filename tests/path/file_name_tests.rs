// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::path::Path;

use qubit_local_files::path;

/// Verifies focused path helpers expose lexical file-name components.
#[test]
fn test_file_name_helpers_return_expected_components() {
    let value = Path::new("/tmp/archive.tar.gz");

    assert_eq!(Some("archive.tar.gz"), path::file_name(value));
    assert_eq!(Some("archive.tar"), path::file_stem(value));
    assert_eq!(Some("archive"), path::file_prefix(value));
    assert_eq!(Some("gz"), path::extension(value));
    assert_eq!(Some(String::from(".gz")), path::dot_extension(value));
    assert!(path::has_extension(value, ".gz"));
    assert!(path::has_extension_ignore_ascii_case(value, "GZ"));
}

/// Verifies string-oriented helpers preserve their lexical safety behavior.
#[test]
fn test_file_name_helpers_extract_path_and_url_segments() {
    assert_eq!(
        "report.txt",
        path::file_name_from_path(r"C:\tmp\report.txt")
    );
    assert_eq!(
        "report final.txt",
        path::file_name_from_url("https://example.test/a/report%20final.txt")
    );
}

/// Verifies fallible random generation produces one safe path component.
#[test]
fn test_random_file_name_with_uses_requested_affixes() {
    let name = path::random_file_name_with(Some("prefix-"), Some(".tmp"))
        .expect("a random file name should be generated");

    assert!(name.starts_with("prefix-"));
    assert!(name.ends_with(".tmp"));
    assert_eq!(
        Path::new(&name).file_name(),
        Some(Path::new(&name).as_os_str())
    );
}
