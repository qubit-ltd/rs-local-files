// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

mod capability;
mod local;
mod options;
mod rooted;
mod rooted_local_file_system;
mod temp;

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
