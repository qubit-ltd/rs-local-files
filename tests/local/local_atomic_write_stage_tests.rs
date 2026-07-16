// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use qubit_local_files::LocalAtomicWriteStage;

#[test]
fn test_atomic_write_stage_variants_are_distinct_and_debuggable() {
    let stages = [
        LocalAtomicWriteStage::PrepareParent,
        LocalAtomicWriteStage::InspectDestination,
        LocalAtomicWriteStage::CreateTemporaryFile,
        LocalAtomicWriteStage::WriteTemporaryFile,
        LocalAtomicWriteStage::PreservePermissions,
        LocalAtomicWriteStage::SyncTemporaryFile,
        LocalAtomicWriteStage::ReplaceDestination,
        LocalAtomicWriteStage::CleanupTemporaryFile,
        LocalAtomicWriteStage::SyncParentDirectory,
    ];

    for (index, stage) in stages.iter().enumerate() {
        assert_eq!(*stage, stages[index]);
        assert!(!format!("{stage:?}").is_empty());
    }
}
