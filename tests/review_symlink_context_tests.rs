// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

#[cfg(unix)]
use std::fs;
#[cfg(unix)]
use std::os::unix::fs::symlink;
#[cfg(unix)]
use std::path::Path;

#[cfg(unix)]
use qubit_io::Streams;
#[cfg(unix)]
use qubit_local_files::LocalFileSystem;
#[cfg(unix)]
use qubit_local_files::error::LocalFileErrorKind;
#[cfg(unix)]
use qubit_local_files::options::LocalObservedReadOptions;
#[cfg(unix)]
use qubit_local_files::options::LocalResolutionLimits;
#[cfg(unix)]
use tempfile::tempdir;

#[cfg(unix)]
#[test]
fn test_hardlinked_relative_symlink_is_not_a_cycle() {
    let fixture = tempdir().expect("fixture");
    fs::create_dir_all(fixture.path().join("d/d")).expect("nested directories");
    fs::write(fixture.path().join("d/d/l"), b"payload").expect("payload");
    symlink("d/l", fixture.path().join("l")).expect("first symlink");
    fs::hard_link(fixture.path().join("l"), fixture.path().join("d/l")).expect("hard link symlink");
    assert_eq!(
        fs::read(fixture.path().join("l")).expect("native traversal"),
        b"payload"
    );

    let filesystem = LocalFileSystem::rooted(fixture.path()).expect("rooted filesystem");
    let mut reader = filesystem.open_reader(Path::new("l")).expect("contextual traversal");
    let mut bytes = Vec::new();
    Streams::copy_input_to_output(&mut reader, &mut bytes).expect("read payload");
    assert_eq!(bytes, b"payload");
}

#[cfg(unix)]
#[test]
fn test_active_self_and_double_link_cycles_are_rejected() {
    let fixture = tempdir().expect("fixture");
    symlink("self", fixture.path().join("self")).expect("self cycle");
    symlink("second", fixture.path().join("first")).expect("first link");
    symlink("first", fixture.path().join("second")).expect("second link");
    let filesystem = LocalFileSystem::rooted(fixture.path()).expect("rooted filesystem");

    assert_eq!(
        filesystem
            .open_reader(Path::new("self"))
            .expect_err("self cycle")
            .kind(),
        LocalFileErrorKind::InvalidPath
    );
    assert_eq!(
        filesystem
            .open_reader(Path::new("first"))
            .expect_err("double cycle")
            .kind(),
        LocalFileErrorKind::InvalidPath
    );
}

#[cfg(unix)]
#[test]
fn test_same_link_can_be_expanded_again_after_its_first_expansion_ends() {
    let fixture = tempdir().expect("fixture");
    fs::create_dir(fixture.path().join("directory")).expect("directory");
    fs::write(fixture.path().join("directory/leaf"), b"payload").expect("leaf");
    symlink("directory", fixture.path().join("link")).expect("link");
    let filesystem = LocalFileSystem::rooted(fixture.path()).expect("rooted filesystem");

    let mut reader = filesystem
        .open_reader(Path::new("link/../link/leaf"))
        .expect("the same link may be revisited after its active marker closes");
    let mut bytes = Vec::new();
    Streams::copy_input_to_output(&mut reader, &mut bytes).expect("read leaf");
    assert_eq!(bytes, b"payload");
}

#[cfg(unix)]
#[test]
fn test_explicit_symlink_hop_limits_remain_effective() {
    let fixture = tempdir().expect("fixture");
    fs::write(fixture.path().join("payload"), b"payload").expect("payload");
    symlink("payload", fixture.path().join("second")).expect("second link");
    symlink("second", fixture.path().join("first")).expect("first link");
    let filesystem = LocalFileSystem::rooted(fixture.path()).expect("rooted filesystem");

    let one_hop =
        LocalObservedReadOptions::new().with_resolution_limits(LocalResolutionLimits::new().with_max_symlink_hops(1));
    assert_eq!(
        filesystem
            .open_observed_reader(Path::new("first"), &one_hop)
            .expect_err("one hop must not allow two links")
            .kind(),
        LocalFileErrorKind::ResourceLimit
    );
    let two_hops =
        LocalObservedReadOptions::new().with_resolution_limits(LocalResolutionLimits::new().with_max_symlink_hops(2));
    assert!(filesystem.open_observed_reader(Path::new("first"), &two_hops).is_ok());
}
