// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::io::Read;

use qubit_local_files::read::{
    OpenOptions,
    open,
};
use tempfile::tempdir;

#[test]
fn opens_a_regular_file_for_reading() {
    let directory = tempdir().expect("temporary directory should be created");
    let path = directory.path().join("payload.txt");
    std::fs::write(&path, b"payload").expect("fixture should be written");

    let mut file =
        open(&path, &OpenOptions::default()).expect("regular file should open");
    let mut content = String::new();
    file.read_to_string(&mut content)
        .expect("content should be readable");

    assert_eq!("payload", content);
}
