// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Crate-private contract tests for `LocalCopyDirStats`.

use qubit_local_files::internal_test_support::LocalCopyDirStats;

#[test]
fn test_local_copy_dir_stats_exposes_counts_and_publication_flags() {
    let stats = LocalCopyDirStats::default();
    assert_eq!(stats.files(), 0);
    assert_eq!(stats.directories(), 0);
    assert_eq!(stats.bytes(), 0);
    assert_eq!(stats.skipped(), 0);
    assert_eq!(stats.overwritten(), 0);
    assert!(stats.atomic_publication());
    assert!(stats.files_durable());
}
