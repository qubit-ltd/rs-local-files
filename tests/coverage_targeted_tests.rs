// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::io::IoSlice;
use std::io::IoSliceMut;
use std::path::Path;

use qubit_local_files::LocalFileSystem;
use qubit_local_files::options::LocalDirectoryOpenOptions;
use qubit_local_files::options::LocalPersistOptions;
use qubit_local_files::options::LocalSealOptions;
use qubit_local_files::options::LocalTempDirectoryOptions;
use qubit_local_files::options::LocalWriteMode;
use qubit_local_files::options::LocalWriteOptions;
use qubit_local_files::policy::LocalFileSyncRequirement;
use tempfile::tempdir;

#[test]
fn test_local_file_writer_vectored_write_publishes_ordered_buffers() {
    let fixture = tempdir().expect("fixture");
    let target = fixture.path().join("vectored");
    let filesystem = LocalFileSystem::host().expect("host filesystem");
    let mut writer = filesystem
        .open_writer_with_options(&target, &LocalWriteOptions::new(LocalWriteMode::CreateNew))
        .expect("writer");
    let written = writer
        .write_vectored(&[IoSlice::new(b"first"), IoSlice::new(b"-"), IoSlice::new(b"second")])
        .expect("vectored write");
    assert!((1..=12).contains(&written));
    let _outcome = writer.commit().expect("publish write");
    assert_eq!(std::fs::read(target).expect("published bytes"), b"first-second");
}

#[test]
fn test_local_file_reader_vectored_read_fills_ordered_buffers() {
    let fixture = tempdir().expect("fixture");
    let target = fixture.path().join("vectored");
    std::fs::write(&target, b"first-second").expect("fixture bytes");
    let filesystem = LocalFileSystem::host().expect("host filesystem");
    let mut reader = filesystem.open_reader(Path::new(&target)).expect("reader");
    let mut first = [0; 5];
    let mut separator = [0; 1];
    let mut second = [0; 6];
    let count = reader
        .read_vectored(&mut [
            IoSliceMut::new(&mut first),
            IoSliceMut::new(&mut separator),
            IoSliceMut::new(&mut second),
        ])
        .expect("vectored read");
    assert_eq!(count, 12);
    assert_eq!(&first, b"first");
    assert_eq!(&separator, b"-");
    assert_eq!(&second, b"second");
}

#[test]
fn test_rooted_path_lexical_escape_is_rejected_with_namespace_context() {
    let fixture = tempdir().expect("fixture");
    let filesystem = LocalFileSystem::rooted(fixture.path()).expect("rooted filesystem");
    let error = filesystem
        .canonicalize(Path::new("../outside"))
        .expect_err("lexical parent must not leave the virtual root");
    assert_eq!(error.kind(), qubit_local_files::error::LocalFileErrorKind::InvalidPath);
    assert_eq!(error.path(), Some(Path::new("../outside")));
}

#[test]
fn test_filesystem_capability_limit_and_space_snapshots_expose_supported_fields() {
    let fixture = tempdir().expect("fixture");
    let filesystem = LocalFileSystem::host().expect("host filesystem");
    let capabilities = filesystem.capabilities();
    let _ = capabilities.supports_rooted_operations();
    let _ = capabilities.supports_atomic_replace();
    let _ = capabilities.can_attempt_atomic_temp_persist();
    #[allow(deprecated)]
    let _ = capabilities.supports_atomic_temp_persist();

    let limits = filesystem.limits();
    let _ = limits.max_path_length();
    let _ = limits.max_component_length();
    let _ = limits.length_unit();

    let probed_limits = filesystem.limits_at(fixture.path()).expect("path limits");
    let _ = probed_limits.max_path_length();
    let _ = probed_limits.max_component_length();
    let _ = probed_limits.length_unit();
    let space = filesystem.space_at(fixture.path()).expect("filesystem space");
    let _ = space.capacity_bytes();
    let _ = space.free_bytes();
    let _ = space.available_bytes();
}

#[test]
fn test_directory_qualified_file_operand_reports_not_directory() {
    let fixture = tempdir().expect("fixture");
    let file = fixture.path().join("file");
    std::fs::write(&file, b"payload").expect("file fixture");
    let filesystem = LocalFileSystem::host().expect("host filesystem");
    let directory_qualified = Path::new(&format!("{}/", file.display())).to_path_buf();
    let error = filesystem
        .target_metadata(&directory_qualified)
        .expect_err("a file cannot satisfy a directory-qualified operand");
    assert_eq!(error.kind(), qubit_local_files::error::LocalFileErrorKind::NotDirectory);
}

#[test]
fn test_writer_open_error_preserves_directory_operand_context() {
    let fixture = tempdir().expect("fixture");
    let filesystem = LocalFileSystem::host().expect("host filesystem");
    let error = filesystem
        .open_writer_with_options(fixture.path(), &LocalWriteOptions::new(LocalWriteMode::CreateNew))
        .expect_err("a directory cannot be opened as a new file writer");
    assert!(matches!(
        error.kind(),
        qubit_local_files::error::LocalFileErrorKind::IsDirectory
            | qubit_local_files::error::LocalFileErrorKind::AlreadyExists
    ));
    assert_eq!(error.path(), Some(fixture.path()));
}

#[test]
fn test_filesystem_default_rename_moves_file() {
    let fixture = tempdir().expect("fixture");
    let source = fixture.path().join("source");
    let destination = fixture.path().join("destination");
    std::fs::write(&source, b"payload").expect("source fixture");
    let filesystem = LocalFileSystem::host().expect("host filesystem");

    let _outcome = filesystem.rename(&source, &destination).expect("rename");

    assert!(!source.exists());
    assert_eq!(std::fs::read(destination).expect("renamed bytes"), b"payload");
}

#[test]
fn test_staged_directory_debug_reports_state_without_exposing_native_path() {
    let fixture = tempdir().expect("fixture");
    let filesystem = LocalFileSystem::host().expect("host filesystem");
    let parent = filesystem
        .open_directory_with_options(fixture.path(), &LocalDirectoryOpenOptions::new())
        .expect("parent directory");
    let stage = parent
        .create_staged_directory(&LocalTempDirectoryOptions::new())
        .expect("stage");

    let debug = format!("{stage:?}");
    assert!(debug.contains("LocalStagedDirectory"));
    assert!(debug.contains("owner_present: true"));
    assert!(!debug.contains(&fixture.path().to_string_lossy().to_string()));
    stage.cleanup().expect("cleanup stage");
}

#[test]
fn test_temp_file_persist_at_rejects_non_directory_and_dotted_bases() {
    let fixture = tempdir().expect("fixture");
    let file_base = fixture.path().join("file-base");
    std::fs::write(&file_base, b"not a directory").expect("base file");
    let filesystem = LocalFileSystem::host().expect("host filesystem");

    let temporary = filesystem
        .create_temp_file_with_options(&qubit_local_files::options::LocalTempFileOptions::new())
        .expect("temporary source");
    let error = temporary
        .persist_at(&file_base, Path::new("published"), LocalPersistOptions::new())
        .expect_err("a regular file cannot be a persistence base");
    assert_eq!(
        error.error().kind(),
        qubit_local_files::error::LocalFileErrorKind::NotDirectory
    );

    let temporary = filesystem
        .create_temp_file_with_options(&qubit_local_files::options::LocalTempFileOptions::new())
        .expect("second temporary source");
    let dotted_base = fixture.path().join("..");
    let error = temporary
        .persist_at(&dotted_base, Path::new("published"), LocalPersistOptions::new())
        .expect_err("explicit bases cannot contain parent components");
    assert_eq!(
        error.error().kind(),
        qubit_local_files::error::LocalFileErrorKind::InvalidPath
    );
}

#[test]
fn test_temp_file_creation_parent_keep_and_vectored_write() {
    let filesystem = LocalFileSystem::host().expect("host filesystem");
    let mut temporary = filesystem
        .create_temp_file_with_options(&qubit_local_files::options::LocalTempFileOptions::new())
        .expect("temporary file");
    assert!(temporary.creation_parent().is_absolute());
    assert!(temporary.path().starts_with(temporary.creation_parent()));
    let written = temporary
        .write_vectored(&[IoSlice::new(b"coverage"), IoSlice::new(b"-target")])
        .expect("temporary vectored write");
    assert!(written > 0);
    let published = temporary.keep().expect("keep temporary file");
    assert!(published.path().exists());
}

#[test]
fn test_sealed_temp_file_observation_and_cleanup_lifecycle() {
    let filesystem = LocalFileSystem::host().expect("host filesystem");
    let temporary = filesystem
        .create_temp_file_with_options(&qubit_local_files::options::LocalTempFileOptions::new())
        .expect("temporary file");
    let mut sealed = temporary
        .seal_with_options(&LocalSealOptions::new())
        .expect("seal temporary file");

    assert!(sealed.path().is_absolute());
    assert_eq!(sealed.native_path(), Some(sealed.path()));
    assert!(sealed.observation().expect("sealed observation").identity().is_some());

    sealed.cleanup().expect("cleanup sealed file");
    assert_eq!(
        sealed.observation().expect_err("closed sealed handle").kind(),
        qubit_local_files::error::LocalFileErrorKind::Io
    );
}

#[test]
fn test_seal_options_accept_file_sync_requirement() {
    let _options = LocalSealOptions::new().with_file_sync(LocalFileSyncRequirement::Preferred);
}

#[test]
fn test_filesystem_read_all_reports_complete_bytes_and_missing_path_context() {
    let fixture = tempdir().expect("fixture");
    let path = fixture.path().join("payload");
    std::fs::write(&path, b"complete payload").expect("write payload");
    let filesystem = LocalFileSystem::host().expect("host filesystem");

    assert_eq!(filesystem.read_all(&path).expect("read all bytes"), b"complete payload");
    let missing = filesystem
        .read_all(&fixture.path().join("missing"))
        .expect_err("missing file should fail");
    assert_eq!(missing.kind(), qubit_local_files::error::LocalFileErrorKind::NotFound);
    assert!(missing.path().is_some());
}
