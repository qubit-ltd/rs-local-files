// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Crate-local coverage tests for public filesystem facade delegation.

use std::fs;
use std::io::Read;
use std::io::Write;
use std::path::Path;
use std::time::Duration;

use qubit_local_files::LocalCopyConflictPolicy;
use qubit_local_files::LocalCopyOptions;
use qubit_local_files::LocalCreateDirectoryOptions;
use qubit_local_files::LocalDeleteOptions;
use qubit_local_files::LocalFileErrorKind;
use qubit_local_files::LocalFileSystem;
use qubit_local_files::LocalFileSystemLimits;
use qubit_local_files::LocalFileSystemProtocols;
use qubit_local_files::LocalFileSystemScope;
use qubit_local_files::LocalListOptions;
use qubit_local_files::LocalPaths;
use qubit_local_files::LocalReadOptions;
use qubit_local_files::LocalRenameOptions;
use qubit_local_files::LocalResourceKind;
use qubit_local_files::LocalResourceLimitError;
use qubit_local_files::LocalSymlinkPolicy;
use qubit_local_files::LocalTempDirectoryOptions;
use qubit_local_files::LocalTempFileOptions;
use qubit_local_files::LocalWriteMode;
use qubit_local_files::LocalWriteOptions;
use tempfile::tempdir;

/// Verifies per-instance defaults are observable, replaceable, and used by
/// every convenience operation that omits an explicit options value.
#[test]
fn test_public_facade_uses_complete_instance_defaults() {
    let directory = tempdir().expect("temporary directory should be created");
    let mut filesystem = LocalFileSystem::host().expect("Host filesystem should open");

    filesystem
        .set_symlink_policy(LocalSymlinkPolicy::FollowAcrossScope)
        .expect("Host symlink policy should be configurable");
    let symlink_policy =
        std::hint::black_box(LocalFileSystem::symlink_policy as fn(&LocalFileSystem) -> LocalSymlinkPolicy);
    assert_eq!(LocalSymlinkPolicy::FollowAcrossScope, symlink_policy(&filesystem));
    let diagnostic_root =
        std::hint::black_box(LocalFileSystem::diagnostic_root as fn(&LocalFileSystem) -> Option<&Path>);
    assert!(diagnostic_root(&filesystem).is_none());
    let protocols =
        std::hint::black_box(LocalFileSystem::protocols as fn(&LocalFileSystem) -> LocalFileSystemProtocols);
    let limits = std::hint::black_box(LocalFileSystem::limits as fn(&LocalFileSystem) -> LocalFileSystemLimits);
    assert!(protocols(&filesystem).supports_rooted_operations());
    assert_eq!(limits(&filesystem), filesystem.limits());
    let rooted_paths = LocalPaths::rooted();
    assert_eq!(LocalFileSystemScope::Rooted, rooted_paths.scope());
    let generated_name = rooted_paths
        .file_names()
        .random_name()
        .expect("native random filename generation should succeed");
    assert!(!generated_name.is_empty());

    let read_options = LocalReadOptions::new().with_open_retry_timeout(Duration::ZERO);
    filesystem
        .set_default_read_options(read_options)
        .expect("reader defaults should be configurable");
    assert_eq!(
        Some(Duration::ZERO),
        std::hint::black_box(LocalFileSystem::default_read_options as fn(&LocalFileSystem) -> &LocalReadOptions)(
            &filesystem,
        )
        .open_retry_timeout()
    );

    let write_options = LocalWriteOptions::new(LocalWriteMode::CreateOrReplace);
    filesystem
        .set_default_write_options(write_options)
        .expect("writer defaults should be configurable");
    assert_eq!(
        LocalWriteMode::CreateOrReplace,
        std::hint::black_box(LocalFileSystem::default_write_options as fn(&LocalFileSystem) -> &LocalWriteOptions)(
            &filesystem,
        )
        .mode()
    );

    assert!(
        filesystem
            .set_default_list_options(LocalListOptions::new().with_max_open_directories(0))
            .is_err()
    );
    filesystem
        .set_default_list_options(LocalListOptions::new().with_recursive())
        .expect("listing defaults should be configurable");
    assert!(
        std::hint::black_box(LocalFileSystem::default_list_options as fn(&LocalFileSystem) -> &LocalListOptions)(
            &filesystem,
        )
        .recursive()
    );

    assert!(
        filesystem
            .set_default_copy_options(LocalCopyOptions::new().with_deadline(Duration::MAX))
            .is_err()
    );
    filesystem
        .set_default_copy_options(LocalCopyOptions::new().with_conflict(LocalCopyConflictPolicy::Overwrite))
        .expect("copy defaults should be configurable");
    assert_eq!(
        LocalCopyConflictPolicy::Overwrite,
        std::hint::black_box(LocalFileSystem::default_copy_options as fn(&LocalFileSystem) -> &LocalCopyOptions)(
            &filesystem,
        )
        .conflict()
    );

    filesystem
        .set_default_create_directory_options(LocalCreateDirectoryOptions::new().with_recursive())
        .expect("directory-creation defaults should be configurable");
    assert!(
        std::hint::black_box(
            LocalFileSystem::default_create_directory_options as fn(&LocalFileSystem) -> &LocalCreateDirectoryOptions,
        )(&filesystem)
        .recursive()
    );

    filesystem
        .set_default_delete_options(LocalDeleteOptions::new().with_recursive())
        .expect("deletion defaults should be configurable");
    assert!(
        std::hint::black_box(LocalFileSystem::default_delete_options as fn(&LocalFileSystem) -> &LocalDeleteOptions)(
            &filesystem,
        )
        .recursive()
    );

    filesystem
        .set_default_rename_options(LocalRenameOptions::new().with_overwrite())
        .expect("rename defaults should be configurable");
    assert!(
        std::hint::black_box(LocalFileSystem::default_rename_options as fn(&LocalFileSystem) -> &LocalRenameOptions)(
            &filesystem,
        )
        .overwrite()
    );

    assert!(
        filesystem
            .set_default_temp_file_options(LocalTempFileOptions::new().with_max_attempts(0))
            .is_err()
    );
    filesystem
        .set_default_temp_file_options(
            LocalTempFileOptions::new()
                .with_parent(directory.path())
                .with_max_attempts(8),
        )
        .expect("temporary-file defaults should be configurable");
    assert_eq!(
        Some(8),
        std::hint::black_box(
            LocalFileSystem::default_temp_file_options as fn(&LocalFileSystem) -> &LocalTempFileOptions,
        )(&filesystem)
        .max_attempts(),
    );

    assert!(
        filesystem
            .set_default_temp_directory_options(LocalTempDirectoryOptions::new().with_max_attempts(0))
            .is_err()
    );
    filesystem
        .set_default_temp_directory_options(
            LocalTempDirectoryOptions::new()
                .with_parent(directory.path())
                .with_max_attempts(8),
        )
        .expect("temporary-directory defaults should be configurable");
    assert_eq!(
        Some(8),
        std::hint::black_box(
            LocalFileSystem::default_temp_directory_options as fn(&LocalFileSystem) -> &LocalTempDirectoryOptions,
        )(&filesystem)
        .max_attempts(),
    );

    let tree = directory.path().join("tree/nested");
    let _ = filesystem
        .create_directory(&tree)
        .expect("configured recursive directory creation should succeed");

    let source = directory.path().join("source");
    let copied = directory.path().join("copied");
    let renamed = directory.path().join("renamed");
    let mut writer = filesystem.open_writer(&source).expect("default writer should open");
    writer
        .write_all(b"payload")
        .expect("default writer should accept bytes");
    let _ = writer.commit().expect("default writer should commit");

    let mut reader = filesystem.open_reader(&source).expect("default reader should open");
    assert_eq!(7, reader.metadata().len());
    let permissions = reader.metadata().permissions();
    let _ = permissions.is_read_only();
    let _ = permissions.unix_mode();
    let mut content = String::new();
    reader.read_to_string(&mut content).expect("default reader should read");
    assert_eq!("payload", content);
    assert_eq!(b"pay", filesystem.read_prefix(&source, 3).unwrap().as_slice());

    let _ = filesystem.copy(&source, &copied).expect("default copy should succeed");
    let _ = filesystem
        .rename(&copied, &renamed)
        .expect("default rename should succeed");
    filesystem
        .list(directory.path())
        .expect("default listing should open")
        .collect::<Result<Vec<_>, _>>()
        .expect("default listing should complete");
    let _ = filesystem
        .delete_file(&renamed)
        .expect("default file deletion should succeed");
    let _ = filesystem
        .delete_directory(&directory.path().join("tree"))
        .expect("default recursive directory deletion should succeed");

    let mut temporary_file = filesystem
        .create_temp_file()
        .expect("default temporary file should be created");
    temporary_file.close();
    temporary_file
        .cleanup()
        .expect("default temporary file should clean up");
    let mut temporary_directory = filesystem
        .create_temp_directory()
        .expect("default temporary directory should be created");
    temporary_directory
        .cleanup()
        .expect("default temporary directory should clean up");
}

/// Verifies public failures from capability probes and protected Rooted
/// operands retain their operation-level structured errors.
#[cfg(unix)]
#[test]
fn test_public_facade_contextualizes_capability_and_root_operand_failures() {
    use std::os::unix::fs::symlink;

    let directory = tempdir().expect("temporary directory should be created");
    let target = directory.path().join("target");
    fs::create_dir(&target).expect("symlink target should be created");
    let link = directory.path().join("link");
    symlink(&target, &link).expect("host symlink should be created");

    let mut host = LocalFileSystem::host().expect("Host filesystem should open");
    host.set_symlink_policy(LocalSymlinkPolicy::Reject)
        .expect("Host reject policy should be accepted");
    assert_eq!(
        LocalFileErrorKind::Unsupported,
        host.limits_at(&link).unwrap_err().kind()
    );
    assert_eq!(
        LocalFileErrorKind::Unsupported,
        host.space_at(&link).unwrap_err().kind()
    );

    fs::write(directory.path().join("source"), b"payload").expect("rooted source should be created");
    symlink("../../outside", directory.path().join("escape")).expect("escaping symlink should be created");
    let rooted = LocalFileSystem::rooted(directory.path()).expect("Rooted filesystem should open");
    assert_eq!(
        LocalFileErrorKind::InvalidPath,
        rooted.limits_at(Path::new("/escape")).unwrap_err().kind()
    );
    assert_eq!(
        LocalFileErrorKind::InvalidPath,
        rooted.space_at(Path::new("/escape")).unwrap_err().kind()
    );

    assert_eq!(
        LocalFileErrorKind::InvalidPath,
        rooted
            .copy(Path::new(".."), Path::new("/target"))
            .unwrap_err()
            .error()
            .kind()
    );
    assert_eq!(
        LocalFileErrorKind::InvalidPath,
        rooted
            .copy(Path::new("/"), Path::new("/target"))
            .unwrap_err()
            .error()
            .kind()
    );
    assert_eq!(
        LocalFileErrorKind::InvalidPath,
        rooted
            .copy(Path::new("/source"), Path::new("/"))
            .unwrap_err()
            .error()
            .kind()
    );
    assert_eq!(
        LocalFileErrorKind::InvalidPath,
        rooted
            .rename(Path::new("/"), Path::new("/target"))
            .unwrap_err()
            .error()
            .kind()
    );
    assert_eq!(
        LocalFileErrorKind::InvalidPath,
        rooted
            .rename(Path::new("/source"), Path::new("/"))
            .unwrap_err()
            .error()
            .kind()
    );
}

/// Verifies the public facade delegates every ordinary host operation through
/// the library crate, retaining the expected filesystem effects.
#[test]
fn test_public_host_facade_delegates_ordinary_operations() {
    let directory = tempdir().expect("temporary directory should be created");
    let filesystem = LocalFileSystem::host().expect("Host filesystem should open");
    assert_eq!(LocalFileSystemScope::Host, filesystem.scope());
    assert!(filesystem.diagnostic_root().is_none());
    let _ = filesystem.protocols();
    let _ = filesystem.limits();
    let _ = filesystem
        .limits_at(directory.path())
        .expect("host limits should be available");
    let _ = filesystem
        .space_at(directory.path())
        .expect("host space should be available");

    let source = directory.path().join("source");
    let copied = directory.path().join("copied");
    let renamed = directory.path().join("renamed");
    fs::write(&source, b"payload").expect("source fixture should be written");
    assert_eq!(
        7,
        filesystem
            .metadata(&source)
            .expect("source metadata should be available")
            .len()
    );
    let mut reader = filesystem
        .open_reader_with_options(&source, &LocalReadOptions::new())
        .expect("source reader should open");
    let mut content = String::new();
    reader.read_to_string(&mut content).expect("source reader should read");
    assert_eq!("payload", content);
    assert_eq!(
        b"pay",
        filesystem
            .read_prefix_with_options(&source, 3, &LocalReadOptions::new())
            .expect("prefix should be readable")
            .as_slice()
    );

    let _ = filesystem
        .copy_with_options(&source, &copied, &LocalCopyOptions::new())
        .expect("file should copy");
    let _ = filesystem
        .rename_with_options(&copied, &renamed, &LocalRenameOptions::new())
        .expect("file should rename");
    let mut writer = filesystem
        .open_writer_with_options(&renamed, &LocalWriteOptions::new(LocalWriteMode::Append))
        .expect("append writer should open");
    writer.write_all(b"!").expect("append writer should write");
    let _ = writer.commit().expect("append writer should commit");

    let created = directory.path().join("created");
    let _ = filesystem
        .create_directory_with_options(&created, &LocalCreateDirectoryOptions::new())
        .expect("directory should be created");
    assert_eq!(
        3,
        filesystem
            .list_with_options(directory.path(), &LocalListOptions::new())
            .expect("directory listing should open")
            .collect::<Result<Vec<_>, _>>()
            .expect("directory listing should complete")
            .len()
    );
    let _ = filesystem
        .delete_directory_with_options(&created, &LocalDeleteOptions::new())
        .expect("directory should be deleted");
    let _ = filesystem
        .delete_file_with_options(&renamed, &LocalDeleteOptions::new())
        .expect("file should be deleted");

    let mut temporary_file = filesystem
        .create_temp_file_with_options(&LocalTempFileOptions::new().with_parent(directory.path()))
        .expect("temporary file should be created");
    temporary_file.close();
    temporary_file.cleanup().expect("temporary file should clean up");
    let mut temporary_directory = filesystem
        .create_temp_directory_with_options(&LocalTempDirectoryOptions::new().with_parent(directory.path()))
        .expect("temporary directory should be created");
    temporary_directory
        .cleanup()
        .expect("temporary directory should clean up");
}

/// Verifies Rooted facade entry points accept PWD-relative paths while
/// preserving their opened authority and diagnostic root.
#[test]
fn test_public_rooted_facade_delegates_relative_operations() {
    let directory = tempdir().expect("temporary root should be created");
    let filesystem = LocalFileSystem::rooted(directory.path()).expect("rooted filesystem should open");
    assert_eq!(LocalFileSystemScope::Rooted, filesystem.scope());
    assert_eq!(Some(directory.path()), filesystem.diagnostic_root());
    let _ = filesystem.protocols();
    let _ = filesystem.limits();
    let _ = filesystem
        .limits_at(Path::new("missing/child"))
        .expect("rooted limits should be available");
    let _ = filesystem
        .space_at(Path::new("missing/child"))
        .expect("rooted space should be available");

    let _ = filesystem
        .create_directory_with_options(Path::new("nested"), &LocalCreateDirectoryOptions::new())
        .expect("rooted directory should be created");
    let mut writer = filesystem
        .open_writer_with_options(
            Path::new("nested/source"),
            &LocalWriteOptions::new(LocalWriteMode::CreateOrReplace),
        )
        .expect("rooted writer should open");
    writer.write_all(b"payload").expect("rooted writer should write");
    let _ = writer.commit().expect("rooted writer should commit");
    let _ = filesystem
        .copy_with_options(
            Path::new("nested/source"),
            Path::new("nested/copied"),
            &LocalCopyOptions::new(),
        )
        .expect("rooted file should copy");
    let _ = filesystem
        .rename_with_options(
            Path::new("nested/copied"),
            Path::new("nested/renamed"),
            &LocalRenameOptions::new(),
        )
        .expect("rooted file should rename");
    let _ = filesystem
        .delete_file_with_options(Path::new("nested/renamed"), &LocalDeleteOptions::new())
        .expect("rooted file should be deleted");
}

/// Verifies resource-limit diagnostics retain every supplied budget fact.
#[test]
fn test_resource_limit_error_retains_budget_facts() {
    let error = LocalResourceLimitError::new(LocalResourceKind::OpenDirectory, 8, 2, 3);

    assert_eq!(LocalResourceKind::OpenDirectory, error.resource());
    assert_eq!(8, error.limit());
    assert_eq!(2, error.remaining());
    assert_eq!(3, error.requested());
}
