// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::fs;
use std::path::Path;
use std::path::PathBuf;

use qubit_local_files::LocalFileSystem;
use qubit_local_files::options::LocalTempDirectoryOptions;
use qubit_local_files::options::LocalTempFileOptions;
use qubit_local_files::policy::LocalPrivacyRequirement;
use tempfile::tempdir;

fn rooted_host_path(root: &Path, virtual_path: &Path) -> PathBuf {
    root.join(
        virtual_path
            .strip_prefix(Path::new(std::path::MAIN_SEPARATOR_STR))
            .expect("Rooted public paths should be virtual absolute paths"),
    )
}

#[test]
fn test_creation_parent_tracks_host_and_rooted_creation_coordinates() {
    let host_parent = tempdir().expect("Host parent should be created");
    let host = LocalFileSystem::host().expect("Host filesystem should open");
    let mut host_file = host
        .create_temp_file_with_options(&LocalTempFileOptions::new().with_parent(host_parent.path()))
        .expect("Host temporary file should be created");
    let host_file_parent = host_file.creation_parent().to_path_buf();
    assert!(host_file_parent.is_absolute());
    assert_eq!(host_file_parent, host_parent.path());
    assert!(host_file.path().starts_with(&host_file_parent));
    host_file.cleanup().expect("Host temporary file should clean up");
    assert_eq!(host_file.creation_parent(), host_file_parent);

    let mut host_directory = host
        .create_temp_directory_with_options(&LocalTempDirectoryOptions::new().with_parent(host_parent.path()))
        .expect("Host temporary directory should be created");
    let host_directory_parent = host_directory.creation_parent().to_path_buf();
    assert_eq!(host_directory_parent, host_parent.path());
    assert!(host_directory.path().starts_with(&host_directory_parent));
    host_directory
        .cleanup()
        .expect("Host temporary directory should clean up");
    assert_eq!(host_directory.creation_parent(), host_directory_parent);

    let rooted_host = tempdir().expect("Rooted host directory should be created");
    fs::create_dir(rooted_host.path().join("parent")).expect("Rooted explicit parent should be created");
    fs::create_dir(rooted_host.path().join("parent/child")).expect("Rooted virtual-PWD child should be created");
    let mut rooted = LocalFileSystem::rooted(rooted_host.path()).expect("Rooted filesystem should open");
    let mut rooted_file = rooted
        .create_temp_file_with_options(&LocalTempFileOptions::new().with_parent(Path::new("/parent")))
        .expect("Rooted temporary file should be created");
    let rooted_file_parent = rooted_file.creation_parent().to_path_buf();
    assert_eq!(rooted_file_parent, Path::new("/parent"));
    assert!(rooted_file.path().starts_with(&rooted_file_parent));
    assert_eq!(
        rooted_host_path(rooted_host.path(), &rooted_file_parent),
        rooted_host.path().join("parent")
    );
    rooted_file.close();
    assert_eq!(rooted_file.creation_parent(), rooted_file_parent);
    rooted_file.cleanup().expect("Rooted temporary file should clean up");
    assert_eq!(rooted_file.creation_parent(), rooted_file_parent);

    let mut rooted_directory = rooted
        .create_temp_directory_with_options(&LocalTempDirectoryOptions::new().with_parent(Path::new("/parent")))
        .expect("Rooted temporary directory should be created");
    let rooted_directory_parent = rooted_directory.creation_parent().to_path_buf();
    assert_eq!(rooted_directory_parent, Path::new("/parent"));
    assert!(rooted_directory.path().starts_with(&rooted_directory_parent));
    rooted_directory
        .cleanup()
        .expect("Rooted temporary directory should clean up");
    assert_eq!(rooted_directory.creation_parent(), rooted_directory_parent);

    rooted
        .set_current_directory(Path::new("/parent"))
        .expect("Rooted virtual PWD should change");
    let mut rooted_pwd_file = rooted
        .create_temp_file_with_options(&LocalTempFileOptions::new().with_parent(Path::new("child")))
        .expect("Rooted temporary file below virtual PWD should be created");
    let rooted_pwd_parent = rooted_pwd_file.creation_parent().to_path_buf();
    assert_eq!(rooted_pwd_parent, Path::new("/parent/child"));
    assert!(rooted_pwd_file.path().starts_with(&rooted_pwd_parent));
    rooted_pwd_file
        .cleanup()
        .expect("Rooted virtual-PWD file should clean up");
    rooted
        .set_current_directory(Path::new("/"))
        .expect("Rooted virtual PWD should reset to its root");

    let mut rooted_root_file = rooted
        .create_temp_file_with_options(&LocalTempFileOptions::new())
        .expect("Rooted root-level temporary file should be created");
    let rooted_root_file_parent = rooted_root_file.creation_parent().to_path_buf();
    assert_eq!(rooted_root_file_parent, Path::new(std::path::MAIN_SEPARATOR_STR));
    assert!(rooted_root_file.path().starts_with(&rooted_root_file_parent));
    rooted_root_file
        .cleanup()
        .expect("Rooted root-level file should clean up");
    assert_eq!(rooted_root_file.creation_parent(), rooted_root_file_parent);

    let mut rooted_root_directory = rooted
        .create_temp_directory_with_options(&LocalTempDirectoryOptions::new())
        .expect("Rooted root-level temporary directory should be created");
    let rooted_root_directory_parent = rooted_root_directory.creation_parent().to_path_buf();
    assert_eq!(rooted_root_directory_parent, Path::new(std::path::MAIN_SEPARATOR_STR));
    rooted_root_directory
        .cleanup()
        .expect("Rooted root-level directory should clean up");
    assert_eq!(rooted_root_directory.creation_parent(), rooted_root_directory_parent);
}

#[test]
fn test_creation_parent_is_a_snapshot_after_close_and_keep() {
    let host_parent = tempdir().expect("Host parent should be created");
    let mut temporary = LocalFileSystem::host()
        .expect("Host filesystem should open")
        .create_temp_file_with_options(&LocalTempFileOptions::new().with_parent(host_parent.path()))
        .expect("Host temporary file should be created");
    let parent = temporary.creation_parent().to_path_buf();
    temporary.close();
    assert_eq!(temporary.creation_parent(), parent);

    let kept = temporary.keep().expect("Temporary file should be kept");
    assert!(kept.path().starts_with(parent));
}

#[test]
#[cfg(unix)]
fn test_creation_parent_uses_the_active_namespace_for_alias_parents() {
    use std::os::unix::fs::symlink;

    let directory = tempdir().expect("temporary root should be created");
    fs::create_dir(directory.path().join("real")).expect("real parent should be created");
    symlink("real", directory.path().join("alias")).expect("parent alias should be created");

    let host_parent = directory.path().join("alias");
    let host = LocalFileSystem::host().expect("Host filesystem should open");
    let mut host_file = host
        .create_temp_file_with_options(&LocalTempFileOptions::new().with_parent(&host_parent))
        .expect("Host temporary file should be created through the alias");
    let resolved_host_parent = fs::canonicalize(&host_parent).expect("Host alias should resolve");
    assert_eq!(host_file.creation_parent(), resolved_host_parent);
    assert!(host_file.path().starts_with(host_file.creation_parent()));
    host_file.cleanup().expect("Host temporary file should clean up");

    let rooted = LocalFileSystem::rooted(directory.path()).expect("Rooted filesystem should open");
    let mut rooted_file = rooted
        .create_temp_file_with_options(&LocalTempFileOptions::new().with_parent(Path::new("/alias")))
        .expect("Rooted temporary file should be created through the alias");
    assert!(rooted_file.creation_parent().is_absolute());
    assert!(rooted_file.path().starts_with(rooted_file.creation_parent()));
    assert!(!rooted_file.creation_parent().starts_with(directory.path()));
    rooted_file.cleanup().expect("Rooted temporary file should clean up");
}

#[test]
#[cfg(any(unix, windows))]
fn test_creation_parent_tracks_required_privacy_resources() {
    let host_parent = tempdir().expect("Host parent should be created");
    let host = LocalFileSystem::host().expect("Host filesystem should open");
    let mut host_file = host
        .create_temp_file_with_options(
            &LocalTempFileOptions::new()
                .with_parent(host_parent.path())
                .with_privacy(LocalPrivacyRequirement::Required),
        )
        .expect("Host Required temporary file should be created on supported platforms");
    assert_eq!(host_file.creation_parent(), host_parent.path());
    host_file.cleanup().expect("Host Required file should clean up");

    let mut host_directory = host
        .create_temp_directory_with_options(
            &LocalTempDirectoryOptions::new()
                .with_parent(host_parent.path())
                .with_privacy(LocalPrivacyRequirement::Required),
        )
        .expect("Host Required temporary directory should be created on supported platforms");
    assert_eq!(host_directory.creation_parent(), host_parent.path());
    host_directory
        .cleanup()
        .expect("Host Required directory should clean up");

    let rooted_host = tempdir().expect("Rooted host directory should be created");
    let rooted = LocalFileSystem::rooted(rooted_host.path()).expect("Rooted filesystem should open");
    let mut rooted_file = rooted
        .create_temp_file_with_options(&LocalTempFileOptions::new().with_privacy(LocalPrivacyRequirement::Required))
        .expect("Rooted Required temporary file should be created on supported platforms");
    assert_eq!(rooted_file.creation_parent(), Path::new(std::path::MAIN_SEPARATOR_STR));
    rooted_file.cleanup().expect("Rooted Required file should clean up");

    let mut rooted_directory = rooted
        .create_temp_directory_with_options(
            &LocalTempDirectoryOptions::new().with_privacy(LocalPrivacyRequirement::Required),
        )
        .expect("Rooted Required temporary directory should be created on supported platforms");
    assert_eq!(
        rooted_directory.creation_parent(),
        Path::new(std::path::MAIN_SEPARATOR_STR)
    );
    rooted_directory
        .cleanup()
        .expect("Rooted Required directory should clean up");
}
