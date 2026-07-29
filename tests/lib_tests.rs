// =============================================================================

//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

#[cfg(coverage)]
mod atomic_tests;
#[cfg(coverage)]
mod copy_tests;
#[cfg(coverage)]
mod directory_tests;
#[cfg(coverage)]
mod local;
#[cfg(coverage)]
mod metadata_tests;
#[cfg(coverage)]
mod native_module_tests;
#[cfg(coverage)]
mod path;
#[cfg(coverage)]
mod read;
#[cfg(coverage)]
mod remove_tests;
#[cfg(coverage)]
mod rename_tests;
#[cfg(coverage)]
mod rooted;
#[cfg(coverage)]
mod write;

/// Verifies that project wrappers do not hide host-compiled source files from
/// per-source coverage thresholds.
#[cfg(coverage)]
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

/// Verifies crate-root exports for the unified public API.
#[test]
fn test_crate_root_exports_unified_api_types() {
    use std::{borrow::Cow, ffi::OsStr};

    use qubit_local_files::{
        LocalCopyFailureState, LocalPathCodec, LocalPathCodecError, LocalRenameFailureState,
    };

    let _: for<'a> fn(&'a str) -> Result<Cow<'a, OsStr>, LocalPathCodecError> =
        LocalPathCodec::encode;
    let _: Option<LocalCopyFailureState> = None;
    let _: Option<LocalRenameFailureState> = None;
}
