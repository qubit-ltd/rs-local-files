// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
// =============================================================================

use qubit_local_files::{
    LocalFileSystem,
    LocalTempFileOptions,
};
use tempfile::tempdir;

/// Verifies closing file I/O does not release the retained persistence
/// responsibility.
#[test]
fn test_local_temp_file_close_retains_path_and_persist_responsibility() {
    let parent = tempdir().expect("temporary parent should be created");
    let target = parent.path().join("persisted");
    let mut temporary = LocalFileSystem::create_temp_file(
        &LocalTempFileOptions::new().with_parent(parent.path()),
    )
    .expect("temporary file should be created");
    let path = temporary.path().to_path_buf();

    temporary.close();

    assert_eq!(path, temporary.path());
    assert_eq!(
        target,
        temporary
            .persist(&target)
            .expect("closed file should persist")
    );
    assert!(target.exists());
}
