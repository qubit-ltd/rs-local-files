// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

mod local;

/// Verifies that project wrappers do not hide host-compiled source files from
/// per-source coverage thresholds.
#[test]
fn test_coverage_wrappers_do_not_define_project_source_exclusions() {
    for (name, script) in [
        ("coverage.sh", include_str!("../coverage.sh")),
        ("ci-check.sh", include_str!("../ci-check.sh")),
        ("align-ci.sh", include_str!("../align-ci.sh")),
    ] {
        assert!(
            !script.contains("EXCEPTIONAL_COVERAGE_REGEX"),
            "{name} must not exempt host-compiled source files"
        );
        assert!(
            !script.contains("COVERAGE_EXTRA_EXCLUDE_REGEX="),
            "{name} must preserve caller-provided coverage exclusions"
        );
    }
}
