// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::io::Write;

use qubit_local_files::write::{Mode, OpenOptions, open};
use tempfile::tempdir;

#[test]
fn creates_parent_directories_and_writes_a_regular_file() {
    let directory = tempdir().expect("temporary directory should be created");
    let path = directory.path().join("nested/payload.txt");
    let options = OpenOptions::new(Mode::CreateOrTruncate).with_parents();

    let mut file = open(&path, &options).expect("file should open");
    file.write_all(b"payload")
        .expect("content should be writable");
    drop(file);

    assert_eq!(
        b"payload",
        std::fs::read(path)
            .expect("written file should be readable")
            .as_slice(),
    );
}
