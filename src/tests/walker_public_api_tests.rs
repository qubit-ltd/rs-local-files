// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::fs;
use std::path::Path;

use crate::LocalFileSystem;

#[test]
fn test_public_walker_root_and_subtree_restriction() {
    let fixture = tempfile::tempdir().expect("listing fixture should be created");
    fs::create_dir_all(fixture.path().join("wanted/nested")).expect("wanted tree should be created");
    fs::write(fixture.path().join("wanted/nested/item"), b"item").expect("wanted file should be written");
    fs::create_dir_all(fixture.path().join("other")).expect("other directory should be created");
    fs::write(fixture.path().join("other/item"), b"item").expect("other file should be written");

    let filesystem = LocalFileSystem::rooted(fixture.path()).expect("rooted filesystem should open");
    let walker = filesystem
        .list_with_options(Path::new("/"), &crate::LocalListOptions::new().with_recursive())
        .expect("listing root should open");
    assert_eq!(Path::new("/"), walker.root());

    let entries: Vec<_> = walker
        .with_descend_subtree(Path::new("wanted"))
        .expect("relative subtree should be accepted")
        .map(|result| {
            result
                .expect("directory entries should be readable")
                .relative_path()
                .to_path_buf()
        })
        .collect();

    assert!(entries.contains(&Path::new("wanted").to_path_buf()));
    assert!(entries.contains(&Path::new("other").to_path_buf()));
    assert!(entries.contains(&Path::new("wanted/nested/item").to_path_buf()));
    assert!(!entries.contains(&Path::new("other/item").to_path_buf()));
}
