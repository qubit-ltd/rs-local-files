// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::{
    fs,
    path::PathBuf,
};

use qubit_local_files::{
    LocalFileKind,
    LocalFileSystem,
    LocalListOptions,
};
use tempfile::tempdir;

/// Verifies lazy recursive traversal with stable root-relative paths.
#[test]
fn test_local_directory_walker_recurses_lazily() {
    let directory = tempdir().expect("temporary directory should be created");
    fs::create_dir(directory.path().join("nested"))
        .expect("nested directory should be created");
    fs::write(directory.path().join("root.txt"), b"root")
        .expect("root file should be written");
    fs::write(directory.path().join("nested/child.txt"), b"child")
        .expect("nested file should be written");

    let walker = LocalFileSystem::list(
        directory.path(),
        &LocalListOptions::new().with_recursive(),
    )
    .expect("walker should be created");
    let mut entries = walker
        .map(|entry| entry.expect("fixture traversal should succeed"))
        .collect::<Vec<_>>();
    entries
        .sort_by(|left, right| left.relative_path().cmp(right.relative_path()));

    assert_eq!(
        vec![
            PathBuf::from("nested"),
            PathBuf::from("nested/child.txt"),
            PathBuf::from("root.txt"),
        ],
        entries
            .iter()
            .map(|entry| entry.relative_path().to_path_buf())
            .collect::<Vec<_>>(),
    );
    assert_eq!(LocalFileKind::Directory, entries[0].metadata().kind());
}

/// Verifies that the maximum depth is fixed when the walker is created.
#[test]
fn test_local_directory_walker_honors_max_depth() {
    let directory = tempdir().expect("temporary directory should be created");
    fs::create_dir(directory.path().join("nested"))
        .expect("nested directory should be created");
    fs::write(directory.path().join("nested/child"), b"x")
        .expect("child should be written");

    let entries = LocalFileSystem::list(
        directory.path(),
        &LocalListOptions::new().with_recursive().with_max_depth(1),
    )
    .expect("walker should be created")
    .collect::<Result<Vec<_>, _>>()
    .expect("fixture traversal should succeed");

    assert_eq!(1, entries.len());
    assert_eq!(PathBuf::from("nested"), entries[0].relative_path());
}

/// Verifies that default traversal observes but does not follow symbolic links.
#[cfg(unix)]
#[test]
fn test_local_directory_walker_does_not_follow_symlinks_by_default() {
    use std::os::unix::fs::symlink;

    let directory = tempdir().expect("temporary directory should be created");
    let outside = tempdir().expect("outside directory should be created");
    fs::write(outside.path().join("secret"), b"x")
        .expect("outside file should be written");
    symlink(outside.path(), directory.path().join("link"))
        .expect("link should be created");

    let entries =
        LocalFileSystem::list(directory.path(), &LocalListOptions::new())
            .expect("walker should be created")
            .collect::<Result<Vec<_>, _>>()
            .expect("link entry should be observable");

    assert_eq!(1, entries.len());
    assert_eq!(LocalFileKind::Symlink, entries[0].metadata().kind());
}
