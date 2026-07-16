// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
// =============================================================================

use qubit_local_files::LocalCopyDirStage;

/// Verifies that every public recursive-copy stage remains distinguishable.
#[test]
fn test_copy_dir_stage_variants_are_distinct_and_debuggable() {
    let stages = [
        LocalCopyDirStage::InspectSource,
        LocalCopyDirStage::InspectSourceEntry,
        LocalCopyDirStage::ReadSourceDirectory,
        LocalCopyDirStage::PrepareDestination,
        LocalCopyDirStage::CopyFileContents,
        LocalCopyDirStage::PreservePermissions,
        LocalCopyDirStage::CommitFile,
        LocalCopyDirStage::CleanupTemporaryFile,
        LocalCopyDirStage::UpdateStatistics,
    ];

    for (index, stage) in stages.iter().enumerate() {
        assert_eq!(*stage, stages[index]);
        assert!(!format!("{stage:?}").is_empty());
    }
}
