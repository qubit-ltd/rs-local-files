// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

#[cfg(unix)]
#[test]
fn hardlinked_relative_symlink_is_not_a_cycle() {
    use std::path::Path;

    let fixture = tempfile::tempdir().expect("fixture");
    std::fs::create_dir_all(fixture.path().join("d/d")).expect("nested directories");
    std::fs::write(fixture.path().join("d/d/l"), b"payload").expect("payload");
    std::os::unix::fs::symlink("d/l", fixture.path().join("l")).expect("first symlink");
    std::fs::hard_link(fixture.path().join("l"), fixture.path().join("d/l")).expect("hard link symlink");
    assert_eq!(
        std::fs::read(fixture.path().join("l")).expect("native traversal"),
        b"payload"
    );

    let filesystem = qubit_local_files::LocalFileSystem::rooted(fixture.path()).expect("rooted filesystem");
    let mut reader = filesystem.open_reader(Path::new("l")).expect("contextual traversal");
    let mut bytes = Vec::new();
    qubit_io::Streams::copy_input_to_output(&mut reader, &mut bytes).expect("read payload");
    assert_eq!(bytes, b"payload");
}

#[cfg(unix)]
#[test]
fn active_self_and_double_link_cycles_are_rejected() {
    use std::path::Path;

    let fixture = tempfile::tempdir().expect("fixture");
    std::os::unix::fs::symlink("self", fixture.path().join("self")).expect("self cycle");
    std::os::unix::fs::symlink("second", fixture.path().join("first")).expect("first link");
    std::os::unix::fs::symlink("first", fixture.path().join("second")).expect("second link");
    let filesystem = qubit_local_files::LocalFileSystem::rooted(fixture.path()).expect("rooted filesystem");

    assert_eq!(
        filesystem
            .open_reader(Path::new("self"))
            .expect_err("self cycle")
            .kind(),
        qubit_local_files::error::LocalFileErrorKind::InvalidPath
    );
    assert_eq!(
        filesystem
            .open_reader(Path::new("first"))
            .expect_err("double cycle")
            .kind(),
        qubit_local_files::error::LocalFileErrorKind::InvalidPath
    );
}

#[cfg(unix)]
#[test]
fn same_link_can_be_expanded_again_after_its_first_expansion_ends() {
    use std::path::Path;

    let fixture = tempfile::tempdir().expect("fixture");
    std::fs::create_dir(fixture.path().join("directory")).expect("directory");
    std::fs::write(fixture.path().join("directory/leaf"), b"payload").expect("leaf");
    std::os::unix::fs::symlink("directory", fixture.path().join("link")).expect("link");
    let filesystem = qubit_local_files::LocalFileSystem::rooted(fixture.path()).expect("rooted filesystem");

    let mut reader = filesystem
        .open_reader(Path::new("link/../link/leaf"))
        .expect("the same link may be revisited after its active marker closes");
    let mut bytes = Vec::new();
    qubit_io::Streams::copy_input_to_output(&mut reader, &mut bytes).expect("read leaf");
    assert_eq!(bytes, b"payload");
}

#[cfg(unix)]
#[test]
fn explicit_symlink_hop_limits_remain_effective() {
    use std::path::Path;

    use qubit_local_files::options::LocalObservedReadOptions;
    use qubit_local_files::options::LocalResolutionLimits;

    let fixture = tempfile::tempdir().expect("fixture");
    std::fs::write(fixture.path().join("payload"), b"payload").expect("payload");
    std::os::unix::fs::symlink("payload", fixture.path().join("second")).expect("second link");
    std::os::unix::fs::symlink("second", fixture.path().join("first")).expect("first link");
    let filesystem = qubit_local_files::LocalFileSystem::rooted(fixture.path()).expect("rooted filesystem");

    let one_hop =
        LocalObservedReadOptions::new().with_resolution_limits(LocalResolutionLimits::new().with_max_symlink_hops(1));
    assert_eq!(
        filesystem
            .open_observed_reader(Path::new("first"), &one_hop)
            .expect_err("one hop must not allow two links")
            .kind(),
        qubit_local_files::error::LocalFileErrorKind::ResourceLimit
    );
    let two_hops =
        LocalObservedReadOptions::new().with_resolution_limits(LocalResolutionLimits::new().with_max_symlink_hops(2));
    assert!(filesystem.open_observed_reader(Path::new("first"), &two_hops).is_ok());
}
