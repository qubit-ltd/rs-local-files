// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

#[cfg(unix)]
use qubit_local_files::LocalDurabilityRequirement;
#[cfg(unix)]
use qubit_local_files::LocalFileSystem;
#[cfg(unix)]
use qubit_local_files::LocalRenameOptions;
#[cfg(unix)]
use tempfile::tempdir;

/// Verifies required rename durability is reported after publication.
#[cfg(unix)]
#[test]
fn test_required_rename_durability_is_reported() {
    let directory =
        tempdir().expect("rename fixture directory must be created");
    let source = directory.path().join("source");
    let target = directory.path().join("target");
    std::fs::write(&source, b"payload").expect("rename source must be written");

    let outcome = LocalFileSystem::host()
        .rename(
            &source,
            &target,
            &LocalRenameOptions::new()
                .with_durability(LocalDurabilityRequirement::Required),
        )
        .expect("required durable rename must succeed");

    assert!(outcome.durable());
}
