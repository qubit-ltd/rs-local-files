// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

#[cfg(unix)]
use std::os::unix::fs::symlink;
use std::path::Path;

use qubit_io::Streams;
use qubit_local_files::LocalFileSystem;
use qubit_local_files::error::LocalFileErrorKind;
use qubit_local_files::options::LocalDirectoryOpenOptions;
use qubit_local_files::options::LocalObservedReadOptions;
use qubit_local_files::options::LocalPathBindingOptions;

#[cfg(unix)]
#[test]
fn test_trailing_separator_requires_a_directory_for_rooted_and_fixed_readers() {
    let fixture = tempfile::tempdir().expect("fixture");
    std::fs::write(fixture.path().join("payload"), b"file").expect("payload");
    std::fs::create_dir(fixture.path().join("directory")).expect("directory");
    symlink("payload/", fixture.path().join("alias")).expect("file alias");
    symlink("directory/", fixture.path().join("dir-alias")).expect("directory alias");
    let filesystem = LocalFileSystem::rooted(fixture.path()).expect("rooted filesystem");

    let mut reader = filesystem.open_reader(Path::new("payload")).expect("regular file");
    let mut bytes = Vec::new();
    Streams::copy_input_to_output(&mut reader, &mut bytes).expect("read regular file");
    assert_eq!(bytes, b"file");
    assert!(filesystem.open_reader(Path::new("alias")).is_err());
    let observed = LocalObservedReadOptions::new();
    assert!(filesystem.open_observed_reader(Path::new("alias"), &observed).is_err());

    let root = filesystem
        .open_directory_with_options(Path::new("/"), &LocalDirectoryOpenOptions::new())
        .expect("root directory");
    let error = root.open_observed_reader(Path::new("payload/"), &observed).unwrap_err();
    assert_eq!(error.kind(), LocalFileErrorKind::NotDirectory);
    assert!(root.open_observed_reader(Path::new("payload/."), &observed).is_err());
    let mut bound = observed;
    bound = bound.with_path_binding(LocalPathBindingOptions::new());
    assert!(root.open_observed_reader(Path::new("payload/"), &bound).is_err());
    assert!(
        root.list_with_options(Path::new("directory/"), &Default::default())
            .is_ok()
    );
    assert!(
        root.list_with_options(Path::new("dir-alias"), &Default::default())
            .is_ok()
    );
}
