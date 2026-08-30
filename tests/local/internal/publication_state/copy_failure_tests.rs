// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use qubit_local_files::LocalCopyFailureState;
use qubit_local_files::LocalCopyOptions;
use qubit_local_files::LocalFileErrorKind;
use qubit_local_files::LocalFileSystem;
use tempfile::tempdir;

/// Verifies a copy failure before publication reports an unchanged target.
#[test]
fn test_copy_failure_before_publication_is_unchanged() {
    let directory = tempdir().expect("copy fixture directory must be created");
    let failure = LocalFileSystem::host()
        .expect("Host filesystem should open")
        .copy_with_options(
            &directory.path().join("missing"),
            &directory.path().join("target"),
            &LocalCopyOptions::new(),
        )
        .expect_err("missing copy source must fail");

    assert_eq!(LocalFileErrorKind::NotFound, failure.error().kind());
    assert_eq!(LocalCopyFailureState::Unchanged, failure.state());
}
