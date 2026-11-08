// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::path::Path;

use tempfile::tempdir;

use crate::LocalAtomicWriteOptions;
use crate::LocalDurabilityRequirement;
use crate::LocalRelativePath;
use crate::local::LocalRootAtomicWriter;

#[test]
fn test_rooted_atomic_writer_output_range_preserves_sentinels() {
    let directory = tempdir().expect("temporary directory should be created");
    let root = std::fs::File::open(directory.path()).expect("root directory should open");
    let path = LocalRelativePath::new(Path::new("target")).expect("relative path should be valid");
    let options = LocalAtomicWriteOptions::new().with_durability(LocalDurabilityRequirement::NotRequired);
    let mut writer =
        LocalRootAtomicWriter::new(&root, directory.path(), &path, options).expect("rooted atomic writer should open");
    // SAFETY: 2..5 is a valid input range.
    assert_eq!(
        3,
        unsafe { qubit_io::Output::write_unchecked(&mut writer, b"!!xyz!!", 2, 3) }.unwrap()
    );
    // SAFETY: 7..7 is a valid empty input range.
    assert_eq!(
        0,
        unsafe { qubit_io::Output::write_unchecked(&mut writer, b"1234567", 7, 0) }.unwrap()
    );
    writer.commit_with_durability().expect("rooted writer should commit");
    assert_eq!(
        b"xyz",
        std::fs::read(directory.path().join("target")).unwrap().as_slice()
    );
}
