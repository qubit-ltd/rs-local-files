// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

#[cfg(unix)]
use qubit_local_files::LocalFileSystem;
#[cfg(unix)]
use qubit_local_files::options::LocalCopyOptions;
#[cfg(unix)]
use qubit_local_files::outcome::LocalCopyFailureState;
#[cfg(unix)]
use tempfile::tempdir;

#[cfg(unix)]
#[test]
fn test_tree_copy_rejects_missing_parent_before_dotdot_without_panicking() {
    let work = tempdir().expect("isolated fixture");
    let source = work.path().join("source");
    std::fs::create_dir(&source).expect("source");
    std::fs::write(source.join("payload"), b"A").expect("payload");
    let destination = work.path().join("missing/../output");
    let filesystem = LocalFileSystem::host().expect("host");
    let failure = filesystem
        .copy_with_options(&source, &destination, &LocalCopyOptions::new().with_tree_source())
        .expect_err("native traversal through a missing parent must fail");
    assert_eq!(failure.state(), LocalCopyFailureState::Unchanged);
    assert_eq!(failure.request_target_path(), Some(destination.as_path()));
    assert!(!work.path().join("missing").exists());
    assert!(!work.path().join("output").exists());
    assert_eq!(std::fs::read(source.join("payload")).unwrap(), b"A");
}

#[cfg(unix)]
#[test]
fn test_tree_copy_preserves_native_dotdot_and_symlink_path_semantics() {
    use std::os::unix::fs::symlink;

    let work = tempdir().expect("isolated fixture");
    let source = work.path().join("source");
    std::fs::create_dir(&source).expect("source");
    std::fs::write(source.join("payload"), b"A").expect("payload");
    std::fs::create_dir(work.path().join("existing")).expect("existing parent");
    std::fs::create_dir_all(work.path().join("real/sub")).expect("symlink target parent");
    symlink(work.path().join("real/sub"), work.path().join("link")).expect("directory link");
    let filesystem = LocalFileSystem::host().expect("host");
    let options = LocalCopyOptions::new().with_tree_source();

    let ordinary = filesystem
        .copy_with_options(&source, &work.path().join("existing/../ordinary"), &options)
        .expect("existing parent followed by dotdot should retain native semantics");
    assert_eq!(ordinary.stats().files(), 1);
    assert_eq!(std::fs::read(work.path().join("ordinary/payload")).unwrap(), b"A");

    let through_link = filesystem
        .copy_with_options(&source, &work.path().join("link/../through-link"), &options)
        .expect("dotdot after a symlink should follow the native target");
    assert_eq!(through_link.stats().files(), 1);
    assert_eq!(
        std::fs::read(work.path().join("real/through-link/payload")).unwrap(),
        b"A"
    );
    assert!(!work.path().join("through-link").exists());

    std::fs::create_dir(work.path().join("new-parent")).expect("new parent should be created");
    let missing_tail = filesystem
        .copy_with_options(&source, &work.path().join("new-parent/child"), &options)
        .expect("a normal missing tail should be creatable");
    assert_eq!(missing_tail.stats().files(), 1);
    assert_eq!(
        std::fs::read(work.path().join("new-parent/child/payload")).unwrap(),
        b"A"
    );
}
