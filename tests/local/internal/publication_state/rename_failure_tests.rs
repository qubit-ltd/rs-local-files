// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use qubit_local_files::LocalFileErrorKind;
use qubit_local_files::LocalFileSystem;
use qubit_local_files::LocalRenameFailureState;
use qubit_local_files::LocalRenameOptions;
use tempfile::tempdir;

/// Verifies a native rename failure before publication reports unchanged state.
#[test]
fn test_rename_failure_before_publication_is_unchanged() {
    let directory = tempdir().expect("rename fixture directory must be created");
    let failure = LocalFileSystem::host()
        .expect("Host filesystem should open")
        .rename_with_options(
            &directory.path().join("missing"),
            &directory.path().join("target"),
            &LocalRenameOptions::new(),
        )
        .expect_err("missing rename source must fail");

    assert_eq!(LocalFileErrorKind::NotFound, failure.error().kind());
    assert_eq!(LocalRenameFailureState::Unchanged, failure.state());
}
