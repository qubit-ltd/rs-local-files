// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Crate-private contract tests for unified native copy statistics.

use crate::local::LocalCopyDirStats;
use crate::outcome::LocalCopyStats;

#[test]
fn test_internal_constructors_preserve_all_copy_statistics() {
    let internal = LocalCopyDirStats {
        files: 2,
        directories: 3,
        bytes: 5,
        skipped: 7,
        overwritten: 11,
        ..LocalCopyDirStats::default()
    };

    let converted = LocalCopyStats::from_internal(internal);
    assert_eq!(2, converted.files());
    assert_eq!(3, converted.directories());
    assert_eq!(5, converted.bytes());
    assert_eq!(7, converted.skipped());
    assert_eq!(11, converted.overwritten());
}

#[test]
fn test_skipped_one_records_only_the_skipped_entry() {
    let stats = LocalCopyStats::skipped_one();

    assert_eq!(0, stats.files());
    assert_eq!(0, stats.directories());
    assert_eq!(0, stats.bytes());
    assert_eq!(1, stats.skipped());
    assert_eq!(0, stats.overwritten());
}
