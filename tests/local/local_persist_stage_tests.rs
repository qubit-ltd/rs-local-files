// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use super::api_tests::LocalPersistStage;

#[test]
fn test_persist_stage_variants_are_distinct_and_debuggable() {
    let stages = [
        LocalPersistStage::ResolveTarget,
        LocalPersistStage::PrepareParent,
        LocalPersistStage::InstallDestination,
    ];

    for (index, stage) in stages.iter().enumerate() {
        assert_eq!(*stage, stages[index]);
        assert!(!format!("{stage:?}").is_empty());
    }
}
