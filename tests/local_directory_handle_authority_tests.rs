// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::path::Path;

use qubit_local_files::LocalFileSystem;
use qubit_local_files::options::LocalDirectoryOpenOptions;
use qubit_local_files::options::LocalWriteMode;
use qubit_local_files::options::LocalWriteOptions;

#[test]
fn test_cloned_directory_handle_keeps_authority_after_its_path_is_renamed() {
    let fixture = tempfile::tempdir().expect("fixture should be created");
    let original_path = fixture.path().join("directory");
    std::fs::create_dir(&original_path).expect("directory should be created");
    std::fs::write(original_path.join("original"), b"retained").expect("original entry should be written");

    let filesystem = LocalFileSystem::rooted(fixture.path()).expect("rooted filesystem should open");
    let handle = filesystem
        .open_directory_with_options(Path::new("directory"), &LocalDirectoryOpenOptions::new())
        .expect("directory authority should open");
    let cloned = handle.clone();

    let retained_path = fixture.path().join("retained-directory");
    std::fs::rename(&original_path, &retained_path).expect("opened directory should be renamed");
    std::fs::create_dir(&original_path).expect("replacement directory should be created");
    std::fs::write(original_path.join("replacement"), b"different").expect("replacement entry should be written");

    assert_eq!(
        cloned
            .entry_metadata(Path::new("original"))
            .expect("retained authority should query its original entry")
            .expect("original entry should remain beneath the retained authority")
            .len(),
        8
    );
    assert!(
        cloned
            .entry_metadata(Path::new("replacement"))
            .expect("replacement path should be absent from the retained authority")
            .is_none()
    );

    let mut writer = cloned
        .open_writer_with_options(
            Path::new("created-through-handle"),
            &LocalWriteOptions::new(LocalWriteMode::CreateNew),
        )
        .expect("cloned handle should write through its retained authority");
    qubit_io::Output::write_fully(&mut writer, b"bound").expect("write through retained authority should succeed");
    let committed = writer.commit().expect("writer should commit");
    assert_eq!(committed.bytes_written(), 5);

    assert_eq!(
        std::fs::read(retained_path.join("created-through-handle")).expect("write should reach renamed directory"),
        b"bound"
    );
    assert!(
        !original_path.join("created-through-handle").exists(),
        "operations through a retained handle must not follow its diagnostic path"
    );
}
