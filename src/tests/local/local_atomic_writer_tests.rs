// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
use std::fs;

use qubit_io::Output;
use tempfile::tempdir;

use crate::LocalAtomicWriteOptions;
use crate::LocalDurabilityRequirement;
use crate::local::LocalAtomicWriter;

#[test]
fn test_output_range_preserves_sentinels() {
    let directory = tempdir().expect("temporary directory should be created");
    let destination = directory.path().join("target");
    let options = LocalAtomicWriteOptions::new().with_durability(LocalDurabilityRequirement::NotRequired);
    let mut writer = LocalAtomicWriter::new(&destination, options).expect("atomic writer should open");
    // SAFETY: 2..5 is a valid input range.
    assert_eq!(
        3,
        unsafe { Output::write_unchecked(&mut writer, b"!!xyz!!", 2, 3) }.unwrap()
    );
    // SAFETY: 7..7 is a valid empty input range.
    assert_eq!(
        0,
        unsafe { Output::write_unchecked(&mut writer, b"1234567", 7, 0) }.unwrap()
    );
    writer
        .commit_recoverable_with_durability()
        .expect("atomic writer should commit");
    assert_eq!(b"xyz", fs::read(destination).unwrap().as_slice());
}
