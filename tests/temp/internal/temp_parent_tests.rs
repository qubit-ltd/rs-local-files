// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Public behavior coverage for shared temporary parent preparation.

use qubit_local_files::{
    LocalFileSystem,
    LocalPersistOptions,
    LocalTempFileOptions,
};

/// Verifies persistence creates a missing parent only when explicitly enabled.
#[test]
fn test_temp_parent_creates_persist_destination_parent_when_enabled() {
    let root = tempfile::tempdir().expect("temporary root should be created");
    let target = root.path().join("missing/target");
    let temporary = LocalFileSystem::host()
        .create_temp_file(&LocalTempFileOptions::new().with_parent(root.path()))
        .expect("temporary file should be created");

    let outcome = temporary
        .persist_with(&target, LocalPersistOptions::new().with_create_parent())
        .expect("explicit parent creation should publish the temporary file");
    assert!(outcome.path().ends_with("target"));
    assert!(target.is_file());
}
