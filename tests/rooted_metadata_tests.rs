// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::path::Path;

use qubit_local_files::LocalFileKind;
use qubit_local_files::LocalFileSystem;

#[test]
fn observes_root_limits_space_and_metadata() {
    let filesystem = LocalFileSystem::rooted(Path::new("."))
        .expect("current directory can be opened");
    let limits = filesystem
        .limits_at(Path::new("Cargo.toml"))
        .expect("limits are queryable");
    let _ = (limits.max_file_name_bytes(), limits.max_path_bytes());
    let space = filesystem
        .space_at(Path::new("Cargo.toml"))
        .expect("space is queryable");
    let _ = (
        space.available_bytes(),
        space.capacity_bytes(),
        space.free_bytes(),
    );
    assert_eq!(
        filesystem
            .metadata(Path::new(""))
            .expect("root metadata")
            .kind(),
        LocalFileKind::Directory
    );
    assert_eq!(
        filesystem
            .metadata(Path::new("Cargo.toml"))
            .expect("file metadata")
            .kind(),
        LocalFileKind::File
    );
    let _ = filesystem
        .limits_at(Path::new("missing/entry"))
        .expect("nearest existing ancestor provides limits");
    let _ = filesystem
        .space_at(Path::new("missing/entry"))
        .expect("nearest existing ancestor provides space");
    assert!(filesystem.metadata(Path::new("missing/entry")).is_err());
}
