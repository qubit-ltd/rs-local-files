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
#[path = "capability/local_file_system_capabilities_tests.rs"]
mod local_file_system_capabilities_tests;
#[path = "capability/local_file_system_limits_tests.rs"]
mod local_file_system_limits_tests;
#[path = "capability/local_file_system_space_tests.rs"]
mod local_file_system_space_tests;
#[path = "local/internal/operation_policy_tests.rs"]
mod operation_policy_tests;
#[path = "capability/size_limit_tests.rs"]
mod size_limit_tests;
#[path = "temp/internal/temp_parent_tests.rs"]
mod temp_parent_tests;

/// Verifies the small crate-root facade and stable domain modules.
#[test]
fn test_public_facade_and_domain_modules_are_available() {
    use std::ffi::OsStr;
    use std::ffi::OsString;

    use qubit_local_files::LocalResult;
    use qubit_local_files::outcome::LocalCopyFailureState;
    use qubit_local_files::outcome::LocalRenameFailureState;
    use qubit_local_files::path::LocalPathCodec;

    let _: fn(&OsStr) -> LocalResult<String> = LocalPathCodec::encode_component;
    let _: fn(&str) -> LocalResult<OsString> = LocalPathCodec::decode_component;
    let _: Option<LocalCopyFailureState> = None;
    let _: Option<LocalRenameFailureState> = None;
}
