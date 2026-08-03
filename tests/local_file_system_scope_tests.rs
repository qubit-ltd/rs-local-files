// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::fs;
use std::path::Path;

use qubit_local_files::{
    LocalFileKind,
    LocalFileSystem,
    LocalFileSystemScope,
    LocalSymlinkPolicy,
    metadata,
};
#[cfg(unix)]
use qubit_local_files::{
    LocalCopyOptions,
    LocalDeleteOptions,
    LocalPersistOptions,
    LocalReadOptions,
    LocalRenameOptions,
    LocalTempFileOptions,
    LocalWriteMode,
    LocalWriteOptions,
};
use tempfile::tempdir;

/// Verifies Host methods and convenience functions inspect the same native
/// namespace.
#[test]
fn test_local_file_system_host_matches_convenience_functions() {
    let directory = tempdir().expect("temporary directory should be created");
    let path = directory.path().join("payload");
    fs::write(&path, b"payload").expect("fixture should be written");
    let filesystem = LocalFileSystem::host();

    assert_eq!(LocalFileSystemScope::Host, filesystem.scope());
    assert_eq!(
        LocalSymlinkPolicy::FollowAcrossScope,
        filesystem.symlink_policy()
    );
    assert_eq!(
        LocalFileKind::File,
        filesystem
            .metadata(&path)
            .expect("Host instance should inspect the fixture",)
            .kind()
    );
    let convenience = metadata(&path)
        .expect("Host convenience function should inspect the fixture");
    let instance = filesystem
        .metadata(&path)
        .expect("Host instance should inspect the fixture");
    assert_eq!(convenience.kind(), instance.kind());
    assert_eq!(convenience.len(), instance.len());
}

/// Verifies Rooted scope and its separate diagnostic root accessor.
#[test]
fn test_local_file_system_rooted_reports_scope_and_reads_relative_path() {
    let directory = tempdir().expect("temporary directory should be created");
    fs::write(directory.path().join("payload"), b"payload")
        .expect("fixture should be written");
    let filesystem = LocalFileSystem::rooted(directory.path())
        .expect("Rooted filesystem should open");

    assert_eq!(LocalFileSystemScope::Rooted, filesystem.scope(),);
    assert_eq!(
        LocalSymlinkPolicy::FollowWithinScope,
        filesystem.symlink_policy()
    );
    assert_eq!(Some(directory.path()), filesystem.diagnostic_root());
    assert_eq!(
        LocalFileKind::File,
        filesystem
            .metadata(Path::new("payload"))
            .expect("Rooted instance should inspect a relative fixture")
            .kind(),
    );
}

/// Verifies that an explicit rooted across-scope policy applies to reads and
/// mutations through intermediate symbolic links.
#[cfg(unix)]
#[test]
fn test_rooted_follow_across_scope_applies_to_all_path_operations() {
    use std::io::{
        Read,
        Write,
    };
    use std::os::unix::fs::symlink;

    let parent = tempdir().expect("parent should be created");
    let root_path = parent.path().join("root");
    let outside = parent.path().join("outside");
    fs::create_dir(&root_path).expect("root should be created");
    fs::create_dir(&outside).expect("outside should be created");
    fs::write(outside.join("config"), b"old")
        .expect("outside file should be written");
    symlink(&outside, root_path.join("link"))
        .expect("cross-scope link should be created");

    let filesystem = LocalFileSystem::rooted(&root_path)
        .expect("rooted filesystem should open")
        .with_symlink_policy(LocalSymlinkPolicy::FollowAcrossScope);

    let mut reader = filesystem
        .open_reader(Path::new("link/config"), &LocalReadOptions::new())
        .expect("reader should follow the cross-scope link");
    let mut content = String::new();
    reader
        .read_to_string(&mut content)
        .expect("reader should read outside the root");
    assert_eq!("old", content);

    let mut writer = filesystem
        .open_writer(
            Path::new("link/config"),
            &LocalWriteOptions::new(LocalWriteMode::CreateOrReplace),
        )
        .expect("writer should follow the cross-scope link");
    writer
        .write_all(b"new")
        .expect("writer should stage outside-root content");
    let _ = writer
        .commit()
        .expect("writer should publish outside-root content");
    assert_eq!(b"new", fs::read(outside.join("config")).unwrap().as_slice());

    fs::write(outside.join("rename-source"), b"rename")
        .expect("rename source should be written");
    let _ = filesystem
        .rename(
            Path::new("link/rename-source"),
            Path::new("link/renamed"),
            &LocalRenameOptions::new(),
        )
        .expect("rename should follow the cross-scope link");
    assert!(outside.join("renamed").exists());

    fs::write(root_path.join("copy-source"), b"copy")
        .expect("copy source should be written");
    let _ = filesystem
        .copy(
            Path::new("copy-source"),
            Path::new("link/copied"),
            &LocalCopyOptions::new(),
        )
        .expect("copy target should follow the cross-scope link");
    assert_eq!(
        b"copy",
        fs::read(outside.join("copied")).unwrap().as_slice()
    );

    let _ = filesystem
        .delete_file(Path::new("link/copied"), &LocalDeleteOptions::new())
        .expect("delete should follow the cross-scope link");
    assert!(!outside.join("copied").exists());
}

/// Verifies that temporary-resource publication follows intermediate links
/// while replacing a final link entry itself.
#[cfg(unix)]
#[test]
fn test_rooted_temp_persist_uses_the_filesystem_symlink_policy() {
    use std::io::Write;
    use std::os::unix::fs::symlink;

    let parent = tempdir().expect("parent should be created");
    let root_path = parent.path().join("root");
    let outside = parent.path().join("outside");
    fs::create_dir(&root_path).expect("root should be created");
    fs::create_dir(&outside).expect("outside should be created");
    symlink(&outside, root_path.join("link"))
        .expect("cross-scope link should be created");

    let filesystem = LocalFileSystem::rooted(&root_path)
        .expect("rooted filesystem should open")
        .with_symlink_policy(LocalSymlinkPolicy::FollowAcrossScope);
    let mut temporary = filesystem
        .create_temp_file(&LocalTempFileOptions::new())
        .expect("temporary file should be created");
    temporary
        .write_all(b"outside")
        .expect("temporary file should accept content");
    let _ = temporary
        .persist_with(Path::new("link/published"), LocalPersistOptions::new())
        .expect("persist should follow the intermediate link");
    assert_eq!(
        b"outside".as_slice(),
        fs::read(outside.join("published")).unwrap().as_slice()
    );

    fs::write(outside.join("referent"), b"old")
        .expect("referent should be written");
    symlink(outside.join("referent"), root_path.join("entry"))
        .expect("final link should be created");
    let mut temporary = filesystem
        .create_temp_file(&LocalTempFileOptions::new())
        .expect("second temporary file should be created");
    temporary
        .write_all(b"new")
        .expect("second temporary file should accept content");
    let _ = temporary
        .persist_with(
            Path::new("entry"),
            LocalPersistOptions::new().with_overwrite(),
        )
        .expect("persist should replace the final link entry");
    assert_eq!(
        b"old".as_slice(),
        fs::read(outside.join("referent")).unwrap().as_slice()
    );
    assert!(!root_path.join("entry").is_symlink());
    assert_eq!(
        b"new".as_slice(),
        fs::read(root_path.join("entry")).unwrap().as_slice()
    );
}

/// Verifies rooted recursive copy follows in-scope directory links without
/// allowing a followed directory to escape the opened root.
#[cfg(unix)]
#[test]
fn test_rooted_recursive_copy_follows_in_scope_directory_link() {
    use std::os::unix::fs::symlink;

    let directory = tempdir().expect("temporary directory should be created");
    let root = directory.path().join("root");
    let source = root.join("source");
    let target = root.join("target");
    let linked = root.join("linked");
    fs::create_dir_all(source.join("nested"))
        .expect("source directory should be created");
    fs::create_dir(&linked).expect("linked directory should be created");
    fs::write(linked.join("entry"), b"entry")
        .expect("linked entry should be written");
    symlink(&linked, source.join("link"))
        .expect("in-scope directory link should be created");

    let filesystem =
        LocalFileSystem::rooted(&root).expect("rooted filesystem should open");
    let _ = filesystem
        .copy(
            Path::new("source"),
            Path::new("target"),
            &LocalCopyOptions::new().with_tree_source(),
        )
        .expect("in-scope directory link should be followed");
    assert_eq!(
        b"entry",
        fs::read(target.join("link/entry"))
            .expect("linked entry should be copied")
            .as_slice()
    );
}
