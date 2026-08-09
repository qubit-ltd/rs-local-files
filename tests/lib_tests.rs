// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

mod local;
mod options;
mod rooted;
mod rooted_local_file_system;

#[path = "capability/filesystem_probe_tests.rs"]
mod filesystem_probe_tests;
#[path = "options/local_directory_reopen_policy_tests.rs"]
mod local_directory_reopen_policy_tests;
#[path = "capability/local_file_system_limits_tests.rs"]
mod local_file_system_limits_tests;
#[path = "capability/local_file_system_protocols_tests.rs"]
mod local_file_system_protocols_tests;
#[path = "capability/local_file_system_space_tests.rs"]
mod local_file_system_space_tests;
#[path = "local/internal/operation_policy_tests.rs"]
mod operation_policy_tests;
#[path = "capability/size_limit_tests.rs"]
mod size_limit_tests;
#[path = "temp/internal/temp_parent_tests.rs"]
mod temp_parent_tests;

/// Verifies crate-root exports for the unified public API.
#[test]
fn test_crate_root_exports_unified_api_types() {
    use std::borrow::Cow;
    use std::ffi::OsStr;

    use qubit_local_files::LocalCopyFailureState;
    use qubit_local_files::LocalPathCodec;
    use qubit_local_files::LocalPathCodecError;
    use qubit_local_files::LocalRenameFailureState;

    let _: for<'a> fn(&'a str) -> Result<Cow<'a, OsStr>, LocalPathCodecError> =
        LocalPathCodec::from_canonical_text;
    let _: Option<LocalCopyFailureState> = None;
    let _: Option<LocalRenameFailureState> = None;
}
