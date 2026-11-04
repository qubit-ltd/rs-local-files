// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::io::IoSlice;
use std::io::Seek;
use std::io::SeekFrom;
use std::path::Component;
use std::path::Path;

use crate::LocalFileSystem;
use crate::LocalPersistOptions;
use crate::LocalSealOptions;
use crate::options::LocalReadAllOptions;
use crate::options::LocalSnapshotOptions;
use crate::options::LocalTempFileOptions;
use crate::policy::LocalFileSyncRequirement;

#[test]
fn test_temp_file_accessors_writes_and_keep_cover_lifecycle() {
    let filesystem = LocalFileSystem::host().expect("host filesystem");
    let mut temporary = filesystem
        .create_temp_file_with_options(&LocalTempFileOptions::new())
        .expect("temporary file");
    assert!(temporary.creation_parent().is_absolute());
    assert!(temporary.path().starts_with(temporary.creation_parent()));

    let written = temporary
        .write_vectored(&[IoSlice::new(b"first"), IoSlice::new(b"-second")])
        .expect("vectored write");
    assert!(written > 0);
    qubit_io::Output::flush(&mut temporary).expect("flush temporary file");
    temporary.seek(SeekFrom::Start(0)).expect("seek temporary file");

    let outcome = temporary.keep().expect("keep temporary file");
    assert!(outcome.path().exists());
    assert_eq!(std::fs::read(outcome.path()).expect("read kept file").len(), written);
}

#[test]
fn test_temp_file_persist_entry_points_publish_to_requested_paths() {
    let fixture = tempfile::tempdir().expect("fixture");
    let filesystem = LocalFileSystem::host().expect("host filesystem");

    let first_target = fixture.path().join("persisted-default");
    let first = filesystem
        .create_temp_file_with_options(&LocalTempFileOptions::new().with_parent(fixture.path()))
        .expect("first temporary file");
    let first_outcome = first.persist(&first_target).expect("default persist");
    assert_eq!(first_outcome.path(), first_target);

    let second_target = fixture.path().join("persisted-options");
    let second = filesystem
        .create_temp_file_with_options(&LocalTempFileOptions::new().with_parent(fixture.path()))
        .expect("second temporary file");
    let second_outcome = second
        .persist_with(&second_target, LocalPersistOptions::new())
        .expect("persist with options");
    assert_eq!(second_outcome.path(), second_target);
}

#[test]
fn test_sealed_temp_file_publication_view_and_closed_handle_error() {
    let filesystem = LocalFileSystem::host().expect("host filesystem");
    let temporary = filesystem
        .create_temp_file_with_options(&LocalTempFileOptions::new())
        .expect("temporary file");
    let mut sealed = temporary
        .seal_with_options(&LocalSealOptions::new().with_file_sync(LocalFileSyncRequirement::NotRequired))
        .expect("seal temporary file");

    assert!(sealed.path().is_absolute());
    assert_eq!(sealed.native_path(), Some(sealed.path()));
    assert!(sealed.observation().expect("sealed observation").identity().is_some());

    sealed.cleanup().expect("cleanup sealed file");
    assert_eq!(
        sealed.observation().expect_err("cleaned handle is closed").kind(),
        crate::LocalFileErrorKind::Io
    );
}

#[test]
fn test_rooted_temp_file_reports_namespace_path_without_native_path() {
    let fixture = tempfile::tempdir().expect("fixture");
    let filesystem = LocalFileSystem::rooted(fixture.path()).expect("rooted filesystem");
    let temporary = filesystem
        .create_temp_file_with_options(&LocalTempFileOptions::new())
        .expect("rooted temporary file");

    assert_rooted_namespace_absolute(temporary.path());
    assert_rooted_namespace_absolute(temporary.creation_parent());
    assert!(temporary.path().starts_with(temporary.creation_parent()));
    let sealed = temporary
        .seal_with_options(&LocalSealOptions::new().with_file_sync(LocalFileSyncRequirement::NotRequired))
        .expect("seal rooted temporary file");
    assert_rooted_namespace_absolute(sealed.path());
    assert_eq!(sealed.native_path(), None);
    assert!(sealed.observation().is_ok());
    let mut sealed = sealed;
    sealed.cleanup().expect("cleanup rooted sealed file");
}

/// Rooted paths are absolute within the virtual namespace. On Windows they
/// have a root directory without a native drive prefix, so `Path::is_absolute`
/// is not the right assertion for this public path contract.
fn assert_rooted_namespace_absolute(path: &Path) {
    assert_eq!(path.components().next(), Some(Component::RootDir), "{path:?}");
}

#[test]
fn test_filesystem_read_helpers_cover_bytes_text_limits_and_consistent_snapshot() {
    let fixture = tempfile::tempdir().expect("fixture");
    let bytes_path = fixture.path().join("bytes");
    let text_path = fixture.path().join("text");
    let invalid_text_path = fixture.path().join("invalid-text");
    std::fs::write(&bytes_path, b"payload").expect("write bytes");
    std::fs::write(&text_path, b"hello").expect("write text");
    std::fs::write(&invalid_text_path, [0xff, 0xfe]).expect("write invalid text");
    let filesystem = LocalFileSystem::host().expect("host filesystem");

    assert_eq!(filesystem.read_all(&bytes_path).expect("read all bytes"), b"payload");
    assert_eq!(filesystem.read_text(&text_path).expect("read text"), "hello");
    assert_eq!(
        filesystem
            .read_all_with_options(&bytes_path, &LocalReadAllOptions::new().with_max_bytes(7))
            .expect("read bytes at exact limit"),
        b"payload"
    );
    assert_eq!(
        filesystem
            .read_text_with_options(&text_path, &LocalReadAllOptions::new().with_max_bytes(5))
            .expect("read text at exact limit"),
        "hello"
    );
    let too_large = filesystem
        .read_all_with_options(&bytes_path, &LocalReadAllOptions::new().with_max_bytes(6))
        .expect_err("reject file above configured limit");
    assert_eq!(too_large.kind(), crate::LocalFileErrorKind::ResourceLimit);
    let invalid = filesystem
        .read_text(Path::new(&invalid_text_path))
        .expect_err("reject invalid UTF-8");
    assert_eq!(invalid.kind(), crate::LocalFileErrorKind::InvalidEncoding);

    let snapshot = filesystem
        .read_consistent_with_options(&bytes_path, &LocalSnapshotOptions::builder(7).build())
        .expect("read consistent snapshot");
    assert_eq!(snapshot.bytes(), b"payload");

    let copied_path = fixture.path().join("copied");
    let _outcome = filesystem
        .copy(&bytes_path, &copied_path)
        .expect("default copy entry point");
    assert_eq!(
        b"payload",
        std::fs::read(copied_path).expect("read copied bytes").as_slice()
    );
}

#[test]
fn test_text_limit_and_prefix_read_report_expected_results() {
    let fixture = tempfile::tempdir().expect("fixture");
    let path = fixture.path().join("text");
    std::fs::write(&path, b"bounded text").expect("write text");
    let filesystem = LocalFileSystem::host().expect("host filesystem");

    let text_limit = filesystem
        .read_text_with_options(&path, &LocalReadAllOptions::new().with_max_bytes(5))
        .expect_err("text beyond the configured byte limit must be rejected");
    assert_eq!(crate::LocalFileErrorKind::ResourceLimit, text_limit.kind());
    assert_eq!(
        filesystem.read_prefix(&path, 7).expect("read bounded prefix"),
        b"bounded"
    );
    assert!(filesystem.read_prefix(&path, 0).expect("zero-length prefix").is_empty());
}

#[test]
fn test_consistent_snapshot_rejects_a_directory_as_file_content() {
    let fixture = tempfile::tempdir().expect("fixture");
    let filesystem = LocalFileSystem::host().expect("host filesystem");

    let error = filesystem
        .read_consistent_with_options(fixture.path(), &LocalSnapshotOptions::builder(1024).build())
        .expect_err("snapshot reading must reject directory input");

    assert_eq!(crate::LocalFileErrorKind::TypeConflict, error.kind());
    assert!(error.path().is_some());
}

#[test]
fn test_rooted_persist_at_validates_root_and_descendant_bases() {
    let fixture = tempfile::tempdir().expect("fixture");
    std::fs::create_dir(fixture.path().join("nested")).expect("nested base");
    let filesystem = LocalFileSystem::rooted(fixture.path()).expect("rooted filesystem");

    let root_based = filesystem
        .create_temp_file_with_options(&LocalTempFileOptions::new())
        .expect("rooted temporary file")
        .persist_at(Path::new("/"), Path::new("root-target"), LocalPersistOptions::new())
        .expect("root authority should be accepted as the base");
    assert_eq!(Path::new("/root-target"), root_based.path());

    let descendant_based = filesystem
        .create_temp_file_with_options(&LocalTempFileOptions::new())
        .expect("rooted temporary file")
        .persist_at(
            Path::new("/nested"),
            Path::new("child-target"),
            LocalPersistOptions::new(),
        )
        .expect("an existing rooted descendant directory should be accepted as the base");
    assert_eq!(Path::new("/nested/child-target"), descendant_based.path());
}

#[test]
fn test_filesystem_rename_and_directory_listing_entry_points() {
    let fixture = tempfile::tempdir().expect("fixture");
    let filesystem = LocalFileSystem::host().expect("host filesystem");
    let directory = fixture.path().join("directory");
    let _created = filesystem.create_directory(&directory).expect("create directory");
    let source = directory.join("source");
    let destination = directory.join("destination");
    std::fs::write(&source, b"payload").expect("source fixture");

    let _outcome = filesystem.rename(&source, &destination).expect("rename");
    let entries = filesystem.list(&directory).expect("list directory");
    let root = std::hint::black_box(
        crate::LocalDirectoryWalker::root as for<'a> fn(&'a crate::LocalDirectoryWalker) -> &'a Path,
    );
    assert_eq!(directory, root(&entries), "walker exposes its bound traversal root");
    let entries = entries
        .collect::<crate::LocalResult<Vec<_>>>()
        .expect("collect entries");

    assert_eq!(1, entries.len());
    assert_eq!(
        b"payload",
        std::fs::read(destination).expect("renamed bytes").as_slice()
    );
}

#[test]
fn test_temp_directory_persist_entry_points_publish_contents() {
    use crate::options::LocalTempDirectoryOptions;

    let fixture = tempfile::tempdir().expect("fixture");
    let filesystem = LocalFileSystem::host().expect("host filesystem");

    let first_target = fixture.path().join("persisted-directory-default");
    let first = filesystem
        .create_temp_directory_with_options(&LocalTempDirectoryOptions::new().with_parent(fixture.path()))
        .expect("first temporary directory");
    std::fs::write(first.path().join("payload"), b"default").expect("write first payload");
    let first_outcome = first.persist(&first_target).expect("default directory persist");
    assert_eq!(first_target, first_outcome.path());
    assert_eq!(
        b"default",
        std::fs::read(first_target.join("payload"))
            .expect("published payload")
            .as_slice()
    );

    let second_target = fixture.path().join("persisted-directory-options");
    let second = filesystem
        .create_temp_directory_with_options(&LocalTempDirectoryOptions::new().with_parent(fixture.path()))
        .expect("second temporary directory");
    std::fs::write(second.path().join("payload"), b"options").expect("write second payload");
    let second_outcome = second
        .persist_with(&second_target, LocalPersistOptions::new())
        .expect("directory persist with options");
    assert_eq!(second_target, second_outcome.path());
    assert_eq!(
        b"options",
        std::fs::read(second_target.join("payload"))
            .expect("published payload")
            .as_slice()
    );

    let third = filesystem
        .create_temp_directory_with_options(&LocalTempDirectoryOptions::new().with_parent(fixture.path()))
        .expect("third temporary directory");
    std::fs::write(third.path().join("payload"), b"relative").expect("write third payload");
    let third_outcome = third
        .persist_at(
            fixture.path(),
            Path::new("persisted-directory-relative"),
            LocalPersistOptions::new(),
        )
        .expect("directory persist at explicit base");
    assert_eq!(
        fixture.path().join("persisted-directory-relative"),
        third_outcome.path()
    );
    assert_eq!(
        b"relative",
        std::fs::read(third_outcome.path().join("payload"))
            .expect("published relative payload")
            .as_slice()
    );
}

#[test]
fn test_host_current_directory_failure_preserves_existing_directory() {
    let mut filesystem = LocalFileSystem::host().expect("host filesystem");
    let original = filesystem.current_directory().expect("current directory");
    let missing = original.join("codex-rlf9-current-directory-missing");

    let error = filesystem
        .set_current_directory(&missing)
        .expect_err("missing host directory must fail");

    assert_eq!(std::io::ErrorKind::NotFound, error.io_error_kind());
    assert_eq!(crate::LocalFileOperation::SetCurrentDirectory, error.operation());
    assert_eq!(Some(missing.as_path()), error.path());
    assert_eq!(
        original,
        filesystem
            .current_directory()
            .expect("current directory remains unchanged")
    );
}
