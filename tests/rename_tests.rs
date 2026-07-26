// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

/// Verifies local rename moves a file.
#[test]
fn test_rename_moves_file() {
    let directory =
        tempfile::tempdir().expect("a temporary directory should exist");
    let source = directory.path().join("source");
    let target = directory.path().join("target");
    std::fs::write(&source, b"value").expect("the source should be written");
    qubit_local_files::rename::move_path(&source, &target)
        .expect("the file should move");
    assert!(target.exists());
}
