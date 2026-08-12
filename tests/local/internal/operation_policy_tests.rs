// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Regression coverage for shared operation preflight policy.

#[cfg(not(windows))]
use qubit_local_files::LocalDurabilityRequirement;
#[cfg(not(windows))]
use qubit_local_files::LocalFileSystem;
#[cfg(not(windows))]
use qubit_local_files::LocalRenameOptions;

/// Verifies required directory durability is honored by the host rename path.
#[cfg(not(windows))]
#[test]
fn test_operation_policy_honors_required_durability() {
    let root = tempfile::tempdir().expect("temporary root should be created");
    let source = root.path().join("source");
    let target = root.path().join("target");
    std::fs::write(&source, b"source").expect("source should be written");

    let _error = LocalFileSystem::host()
        .rename(
            &source,
            &target,
            &LocalRenameOptions::new()
                .with_durability(LocalDurabilityRequirement::Required),
        )
        .expect("host rename should meet its required durability policy");

    assert!(!source.exists());
    assert!(target.is_file());
}
