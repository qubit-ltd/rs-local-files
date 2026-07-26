// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

/// Verifies fallible existence checks report an existing path.
#[test]
fn test_metadata_exists_reports_existing_path() {
    let directory =
        tempfile::tempdir().expect("a temporary directory should exist");
    assert!(qubit_local_files::metadata::exists(directory.path()).unwrap());
}

/// Verifies followed and unfollowed metadata are both available.
#[test]
fn test_metadata_read_entry_points_inspect_files() {
    let file =
        tempfile::NamedTempFile::new().expect("a temporary file should exist");

    assert!(
        qubit_local_files::metadata::read(file.path())
            .expect("followed metadata should be available")
            .is_file()
    );
    assert!(
        qubit_local_files::metadata::read_link(file.path())
            .expect("unfollowed metadata should be available")
            .is_file()
    );
}
