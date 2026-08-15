// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::path::Path;

use super::RelativePath;

/// Verifies relative path values preserve normal authority-relative paths.
#[test]
fn test_relative_path_parse_preserves_normal_components() {
    let relative = RelativePath::parse(Path::new("reports/2026/a.txt"))
        .expect("normal rooted path should parse");

    assert_eq!(Path::new("reports/2026/a.txt"), relative.as_path());
}

/// Verifies relative path values reject authority-changing components.
#[test]
fn test_relative_path_parse_rejects_authority_changes() {
    for path in ["/escape", "../escape", "safe/./file"] {
        assert!(
            RelativePath::parse(Path::new(path)).is_err(),
            "authority-changing path should be rejected: {path:?}",
        );
    }
}

/// Verifies Windows drive-relative prefixes cannot escape rooted authority.
#[cfg(windows)]
#[test]
fn test_relative_path_parse_rejects_windows_drive_relative_prefix() {
    assert!(RelativePath::parse(Path::new(r"C:escape")).is_err());
}
