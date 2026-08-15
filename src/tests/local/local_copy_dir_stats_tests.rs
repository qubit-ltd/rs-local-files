// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Crate-private contract tests for `LocalCopyDirStats`.

use crate::LocalCopyDirStats;

#[test]
fn test_local_copy_dir_stats_exposes_counts_and_publication_flags() {
    let stats = LocalCopyDirStats {
        files: 1,
        directories: 2,
        bytes: 3,
        skipped: 4,
        overwritten: 5,
        non_atomic_publication: true,
        files_durable: false,
    };
    assert_eq!(stats.files(), 1);
    assert_eq!(stats.directories(), 2);
    assert_eq!(stats.bytes(), 3);
    assert_eq!(stats.skipped(), 4);
    assert_eq!(stats.overwritten(), 5);
    assert!(!stats.atomic_publication());
    assert!(!stats.files_durable());
    assert!(LocalCopyDirStats::default().files_durable());
}
