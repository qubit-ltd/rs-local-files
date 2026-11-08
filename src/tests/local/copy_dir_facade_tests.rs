// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Crate-private contract tests for the recursive directory-copy facade.

use std::fs;
use std::os::unix::fs::symlink;

use tempfile::tempdir;

use crate::LocalCopyDirOptions;
use crate::LocalSymlinkPolicy;
use crate::local::copy_dir_all_with_paths_scoped;

#[test]
fn test_scoped_copy_follows_in_scope_directory_link() {
    let directory = tempdir().expect("temporary directory should be created");
    let source = directory.path().join("source");
    let linked = directory.path().join("linked");
    let destination = directory.path().join("destination");
    fs::create_dir_all(&source).expect("source directory should be created");
    fs::create_dir(&linked).expect("link target should be created");
    fs::write(linked.join("entry"), b"payload").expect("link target entry should be written");
    symlink("../linked", source.join("link")).expect("in-scope directory link should be created");

    let stats = copy_dir_all_with_paths_scoped(
        &source,
        &destination,
        LocalCopyDirOptions::new().with_symlink_policy(LocalSymlinkPolicy::FollowWithinScope),
        directory.path(),
    )
    .expect("copy should follow a directory link inside its scope");

    assert_eq!(1, stats.files);
    assert_eq!(b"payload", fs::read(destination.join("link/entry")).unwrap().as_slice());
}
