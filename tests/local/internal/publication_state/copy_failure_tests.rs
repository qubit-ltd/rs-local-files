// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use qubit_local_files::LocalFileSystem;
use qubit_local_files::error::LocalFileErrorKind;
use qubit_local_files::options::LocalCopyOptions;
use qubit_local_files::outcome::LocalCopyFailureState;
#[cfg(windows)]
use qubit_local_files::outcome::LocalCopyStats;
#[cfg(windows)]
use qubit_local_files::test_support::internal_contract::LocalCopyDirStage;
#[cfg(windows)]
use qubit_local_files::test_support::internal_contract::copy_failure_state;
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

/// Verifies symbolic-link publication stages retain exact recovery states.
#[cfg(windows)]
#[test]
fn test_symbolic_link_publication_stages_map_exact_states() {
    let stats = LocalCopyStats::default();

    assert_eq!(
        LocalCopyFailureState::Unchanged,
        copy_failure_state(LocalCopyDirStage::PublishSymlinkUnchanged, stats),
    );
    assert_eq!(
        LocalCopyFailureState::PartiallyPublished,
        copy_failure_state(LocalCopyDirStage::PublishSymlinkPartially, stats),
    );
    assert_eq!(
        LocalCopyFailureState::Indeterminate,
        copy_failure_state(LocalCopyDirStage::PublishSymlinkIndeterminate, stats),
    );
}
