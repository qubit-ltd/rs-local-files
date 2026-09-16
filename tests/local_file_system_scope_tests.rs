// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::fs;
use std::path::Path;

use qubit_local_files::LocalFileSystem;
use qubit_local_files::error::LocalFileErrorKind;
use qubit_local_files::error::LocalFileOperation;
use qubit_local_files::options::LocalCopyOptions;
use qubit_local_files::options::LocalListOptions;
use qubit_local_files::options::LocalWriteMode;
use qubit_local_files::options::LocalWriteOptions;
use qubit_local_files::outcome::LocalFileKind;
use qubit_local_files::outcome::LocalWriterState;
use qubit_local_files::path::LocalFileSystemScope;
use qubit_local_files::policy::LocalSymlinkPolicy;
use tempfile::tempdir;

/// Verifies Host methods inspect the process-visible native namespace.
#[test]
fn test_local_file_system_host_inspects_native_namespace() {
    let directory = tempdir().expect("temporary directory should be created");
    let path = directory.path().join("payload");
    fs::write(&path, b"payload").expect("fixture should be written");
    let filesystem = LocalFileSystem::host().expect("Host filesystem should open");

    assert_eq!(LocalFileSystemScope::Host, filesystem.scope());
    assert_eq!(LocalSymlinkPolicy::FollowAcrossScope, filesystem.symlink_policy());
    assert_eq!(
        LocalFileKind::File,
        filesystem
            .metadata(&path)
            .expect("Host instance should inspect the fixture",)
            .kind()
    );
    let instance = filesystem
        .metadata(&path)
        .expect("Host instance should inspect the fixture");
    assert_eq!(LocalFileKind::File, instance.kind());
    assert_eq!(instance.len(), b"payload".len() as u64);
}

/// Verifies cloning a Host handle preserves its cheap, stateless semantics.
#[test]
fn test_local_file_system_clone_preserves_host_configuration() {
    let filesystem = LocalFileSystem::host().expect("Host filesystem should open");
    let clone = filesystem.clone();

    assert_eq!(filesystem.scope(), clone.scope());
    assert_eq!(filesystem.capabilities(), clone.capabilities());
    assert_eq!(filesystem.symlink_policy(), clone.symlink_policy());
}

/// Verifies the Host facade publishes a file through the configured instance.
#[test]
fn test_local_file_system_host_writer_workflow() {
    use std::io::Write;

    let directory = tempdir().expect("temporary directory should be created");
    let path = directory.path().join("payload");
    let filesystem = LocalFileSystem::host().expect("Host filesystem should open");
    let mut writer = filesystem
        .open_writer_with_options(&path, &LocalWriteOptions::new(LocalWriteMode::CreateOrReplace))
        .expect("Host writer should open");
    writer.write_all(b"payload").expect("Host writer should accept payload");
    let outcome = writer.commit().expect("Host writer should commit");
    assert_eq!(LocalWriterState::Committed, outcome.state());
    assert_eq!(b"payload", fs::read(path).expect("payload should exist").as_slice());
}

/// Verifies Rooted scope and its separate diagnostic root accessor.
#[test]
fn test_local_file_system_rooted_reports_scope_and_reads_relative_path() {
    let directory = tempdir().expect("temporary directory should be created");
    fs::write(directory.path().join("payload"), b"payload").expect("fixture should be written");
    let filesystem = LocalFileSystem::rooted(directory.path()).expect("Rooted filesystem should open");

    assert_eq!(LocalFileSystemScope::Rooted, filesystem.scope(),);
    assert_eq!(LocalSymlinkPolicy::FollowWithinScope, filesystem.symlink_policy());
    assert_eq!(Some(directory.path()), filesystem.diagnostic_root());
    assert_eq!(
        LocalFileKind::File,
        filesystem
            .metadata(Path::new("payload"))
            .expect("Rooted instance should inspect a relative fixture")
            .kind(),
    );
}

/// Verifies cloning a Rooted handle shares its opened authority safely.
#[test]
fn test_local_file_system_clone_preserves_rooted_authority() {
    let directory = tempdir().expect("temporary directory should be created");
    let filesystem = LocalFileSystem::rooted(directory.path()).expect("Rooted filesystem should open");
    let clone = filesystem.clone();

    assert_eq!(filesystem.scope(), clone.scope());
    assert_eq!(filesystem.capabilities(), clone.capabilities());
    assert_eq!(filesystem.symlink_policy(), clone.symlink_policy());
    assert_eq!(filesystem.diagnostic_root(), clone.diagnostic_root());
}

/// Verifies Rooted configuration rejects an across-scope policy precisely.
#[test]
fn test_rooted_constructor_rejects_follow_across_scope() {
    let directory = tempdir().expect("temporary directory should be created");
    let mut filesystem = LocalFileSystem::rooted(directory.path()).expect("Rooted filesystem should open");
    let error = filesystem
        .set_symlink_policy(LocalSymlinkPolicy::FollowAcrossScope)
        .expect_err("Rooted must reject FollowAcrossScope");

    assert_eq!(LocalFileErrorKind::InvalidOptions, error.kind());
    assert_eq!(LocalFileOperation::Configure, error.operation());
    assert_eq!(None, error.path());
    assert_eq!(
        Some("FollowAcrossScope is incompatible with a Rooted filesystem"),
        error.reason(),
    );
}

/// Verifies an existing Rooted instance cannot enter an across-scope state.
#[test]
fn test_rooted_builder_rejects_follow_across_scope() {
    let directory = tempdir().expect("temporary directory should be created");
    let mut filesystem = LocalFileSystem::rooted(directory.path()).expect("Rooted filesystem should open");

    let error = filesystem
        .set_symlink_policy(LocalSymlinkPolicy::FollowAcrossScope)
        .expect_err("Rooted must reject FollowAcrossScope");

    assert_eq!(LocalFileErrorKind::InvalidOptions, error.kind());
    assert_eq!(LocalFileOperation::Configure, error.operation());
    assert_eq!(None, error.path());
}

/// Verifies per-operation Rooted overrides reject an across-scope policy.
#[test]
fn test_rooted_operation_overrides_reject_follow_across_scope() {
    let directory = tempdir().expect("temporary directory should be created");
    fs::write(directory.path().join("source"), b"source").expect("fixture should be written");
    let filesystem = LocalFileSystem::rooted(directory.path()).expect("Rooted filesystem should open");

    let list_error = filesystem
        .list_with_options(
            Path::new("source"),
            &LocalListOptions::new().with_symlink_policy(LocalSymlinkPolicy::FollowAcrossScope),
        )
        .expect_err("Rooted list override must reject FollowAcrossScope");
    assert_eq!(LocalFileErrorKind::InvalidOptions, list_error.kind());
    assert_eq!(LocalFileOperation::List, list_error.operation());

    let copy_error = filesystem
        .copy_with_options(
            Path::new("source"),
            Path::new("target"),
            &LocalCopyOptions::new().with_symlink_policy(LocalSymlinkPolicy::FollowAcrossScope),
        )
        .expect_err("Rooted copy override must reject FollowAcrossScope");
    assert_eq!(LocalFileErrorKind::InvalidOptions, copy_error.error().kind());
    assert_eq!(LocalFileOperation::Copy, copy_error.error().operation());
}

/// Verifies Host retains support for an across-scope policy.
#[test]
fn test_host_builder_accepts_follow_across_scope() {
    let mut filesystem = LocalFileSystem::host().expect("Host filesystem should open");
    filesystem
        .set_symlink_policy(LocalSymlinkPolicy::FollowAcrossScope)
        .expect("Host should accept FollowAcrossScope");

    assert_eq!(LocalSymlinkPolicy::FollowAcrossScope, filesystem.symlink_policy(),);
}

/// Verifies an actual escaping link remains a path error under the rooted
/// default policy, distinct from an invalid policy configuration.
#[cfg(unix)]
#[test]
fn test_rooted_follow_within_reports_escaping_link_as_invalid_path() {
    use std::os::unix::fs::symlink;

    let parent = tempdir().expect("parent should be created");
    let root = parent.path().join("root");
    let outside = parent.path().join("outside");
    fs::create_dir(&root).expect("root should be created");
    fs::create_dir(&outside).expect("outside should be created");
    fs::write(outside.join("payload"), b"outside").expect("outside fixture should be written");
    symlink("../../outside", root.join("link")).expect("escaping link should be created");

    let error = LocalFileSystem::rooted(&root)
        .expect("Rooted filesystem should open")
        .metadata(Path::new("link/payload"))
        .expect_err("FollowWithinScope must reject an escaping link");

    assert_eq!(LocalFileErrorKind::InvalidPath, error.kind());
    assert_eq!(LocalFileOperation::Metadata, error.operation());
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
    fs::create_dir_all(source.join("nested")).expect("source directory should be created");
    fs::create_dir(&linked).expect("linked directory should be created");
    fs::write(linked.join("entry"), b"entry").expect("linked entry should be written");
    symlink("/linked", source.join("link")).expect("in-scope virtual absolute link should be created");

    let filesystem = LocalFileSystem::rooted(&root).expect("rooted filesystem should open");
    let _ = filesystem
        .copy_with_options(
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

/// Verifies Rooted listing ignores a replacement diagnostic root.
#[cfg(unix)]
#[test]
fn test_rooted_list_uses_opened_authority_after_diagnostic_root_replacement() {
    let parent = tempdir().expect("parent should be created");
    let original = parent.path().join("root");
    let renamed = parent.path().join("renamed-root");
    fs::create_dir(&original).expect("root should be created");
    fs::write(original.join("original-entry"), b"original").expect("original entry should be written");
    let filesystem = LocalFileSystem::rooted(&original).expect("Rooted filesystem should open");

    fs::rename(&original, &renamed).expect("opened root should be renamed");
    fs::create_dir(&original).expect("replacement root should be created");
    fs::write(original.join("replacement-entry"), b"replacement").expect("replacement entry should be written");

    let entries = filesystem
        .list_with_options(Path::new(""), &LocalListOptions::new())
        .expect("Rooted listing should open through retained authority")
        .collect::<Result<Vec<_>, _>>()
        .expect("Rooted listing should remain readable");
    let paths = entries.iter().map(|entry| entry.relative_path()).collect::<Vec<_>>();

    assert_eq!(vec![Path::new("original-entry")], paths);
}

/// Verifies Rooted symlink-aware copy ignores a replacement diagnostic root.
#[cfg(unix)]
#[test]
fn test_rooted_copy_uses_opened_authority_after_diagnostic_root_replacement() {
    use std::os::unix::fs::symlink;

    let parent = tempdir().expect("parent should be created");
    let original = parent.path().join("root");
    let renamed = parent.path().join("renamed-root");
    fs::create_dir_all(original.join("source")).expect("original source should be created");
    fs::create_dir(original.join("linked")).expect("original link target should be created");
    fs::write(original.join("linked/entry"), b"original").expect("original linked entry should be written");
    symlink("../linked", original.join("source/link")).expect("original in-scope link should be created");
    let filesystem = LocalFileSystem::rooted(&original).expect("Rooted filesystem should open");

    fs::rename(&original, &renamed).expect("opened root should be renamed");
    fs::create_dir_all(original.join("source")).expect("replacement source should be created");
    fs::create_dir(original.join("linked")).expect("replacement link target should be created");
    fs::write(original.join("linked/entry"), b"replacement").expect("replacement linked entry should be written");
    symlink("../linked", original.join("source/link")).expect("replacement in-scope link should be created");

    let _ = filesystem
        .copy_with_options(
            Path::new("source"),
            Path::new("target"),
            &LocalCopyOptions::new().with_tree_source(),
        )
        .expect("Rooted copy should use the retained authority");

    assert_eq!(
        b"original",
        fs::read(renamed.join("target/link/entry"))
            .expect("original authority should receive copied content")
            .as_slice(),
    );
    assert!(
        !original.join("target").exists(),
        "replacement diagnostic root must remain unchanged",
    );
}
