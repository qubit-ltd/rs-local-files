// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use qubit_local_files::LocalCopyDirStats;

#[test]
fn test_copy_dir_stats_default_and_fields_are_explicit() {
    let default_stats = LocalCopyDirStats::default();
    assert_eq!(0, default_stats.files);
    assert_eq!(0, default_stats.directories);
    assert_eq!(0, default_stats.bytes);
    assert_eq!(0, default_stats.skipped);

    let mut stats = LocalCopyDirStats::default();
    stats.files = 2;
    stats.directories = 3;
    stats.bytes = 5;
    stats.skipped = 7;
    assert_eq!(2, stats.files());
    assert_eq!(3, stats.directories());
    assert_eq!(5, stats.bytes());
    assert_eq!(7, stats.skipped());
}
