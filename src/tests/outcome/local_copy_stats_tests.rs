// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Crate-private contract tests for `LocalCopyStats`.

use crate::LocalCopyStats;
use crate::local::LocalCopyDirStats;

#[test]
fn test_local_copy_stats_exposes_skipped_and_internal_counts() {
    let skipped = LocalCopyStats::skipped_one();
    assert_eq!(skipped.skipped(), 1);
    let stats = LocalCopyStats::from_internal(LocalCopyDirStats {
        files: 1,
        directories: 2,
        bytes: 3,
        skipped: 4,
        overwritten: 5,
        non_atomic_publication: false,
        files_durable: true,
    });
    assert_eq!(
        (stats.files(), stats.directories(), stats.bytes()),
        (1, 2, 3)
    );
    assert_eq!((stats.skipped(), stats.overwritten()), (4, 5));
}
