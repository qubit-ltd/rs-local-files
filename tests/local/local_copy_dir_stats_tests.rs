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
    assert_eq!(
        LocalCopyDirStats {
            files: 0,
            directories: 0,
            bytes: 0,
            skipped: 0,
        },
        LocalCopyDirStats::default()
    );

    let stats = LocalCopyDirStats {
        files: 2,
        directories: 3,
        bytes: 5,
        skipped: 7,
    };
    assert_eq!(2, stats.files);
    assert_eq!(3, stats.directories);
    assert_eq!(5, stats.bytes);
    assert_eq!(7, stats.skipped);
}
