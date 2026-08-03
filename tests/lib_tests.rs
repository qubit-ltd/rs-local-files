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

/// Verifies crate-root exports for the unified public API.
#[test]
fn test_crate_root_exports_unified_api_types() {
    use std::{
        borrow::Cow,
        ffi::OsStr,
    };

    use qubit_local_files::{
        LocalCopyFailureState,
        LocalPathCodec,
        LocalPathCodecError,
        LocalRenameFailureState,
    };

    let _: for<'a> fn(&'a str) -> Result<Cow<'a, OsStr>, LocalPathCodecError> =
        LocalPathCodec::from_canonical_text;
    let _: Option<LocalCopyFailureState> = None;
    let _: Option<LocalRenameFailureState> = None;
}
