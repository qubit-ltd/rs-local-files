// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
use std::fs;
use std::fs::OpenOptions;

use qubit_local_files::LocalFileSystem;
use qubit_local_files::LocalSealOptions;
use qubit_local_files::LocalTempAccessState;
use qubit_local_files::options::LocalTempCleanupLimits;
use qubit_local_files::options::LocalTempDirectoryOptions;
use qubit_local_files::options::LocalTempFileOptions;
use qubit_local_files::policy::LocalPrivacyRequirement;
use tempfile::tempdir;

#[test]
fn test_lf3_temp_cleanup_limit_retains_owner_for_retry() {
    let fixture = tempdir().expect("fixture");
    let filesystem = LocalFileSystem::host().expect("host filesystem");
    let mut temporary = filesystem
        .create_temp_directory_with_options(
            &LocalTempDirectoryOptions::new()
                .with_parent(fixture.path())
                .with_cleanup_limits(LocalTempCleanupLimits::new().with_max_entries(1)),
        )
        .expect("temporary directory");
    fs::write(temporary.path().join("child"), b"data").expect("child entry");
    assert!(temporary.cleanup().is_err());
    assert!(temporary.path().exists());
    temporary.set_cleanup_limits(LocalTempCleanupLimits::new());
    temporary.cleanup().expect("retry cleanup");
}

#[test]
fn test_lf3_temp_cleanup_replacement_is_preserved() {
    let fixture = tempdir().expect("fixture");
    let filesystem = LocalFileSystem::host().expect("host filesystem");
    let mut temporary = filesystem
        .create_temp_file_with_options(&LocalTempFileOptions::new().with_parent(fixture.path()))
        .expect("temporary file");
    let path = temporary.path().to_path_buf();
    fs::remove_file(&path).expect("remove original entry");
    fs::write(&path, b"replacement").expect("create replacement");
    assert!(temporary.cleanup().is_err());
    assert_eq!(fs::read(path).expect("replacement remains"), b"replacement");
}

#[test]
fn test_lf3_temp_closed_handle_cannot_seal() {
    let fixture = tempdir().expect("fixture");
    let filesystem = LocalFileSystem::host().expect("host filesystem");
    let mut temporary = filesystem
        .create_temp_file_with_options(&LocalTempFileOptions::new().with_parent(fixture.path()))
        .expect("temporary file");
    temporary.close();
    let error = temporary
        .seal_with_options(&LocalSealOptions::new())
        .expect_err("closed handle rejected");
    let mut recovered = error.into_resource();
    recovered.cleanup().expect("cleanup");
}

#[test]
fn test_lf3_temp_replaced_handle_cannot_seal() {
    let fixture = tempdir().expect("fixture");
    let filesystem = LocalFileSystem::host().expect("host filesystem");
    let mut temporary = filesystem
        .create_temp_file_with_options(&LocalTempFileOptions::new().with_parent(fixture.path()))
        .expect("temporary file");
    let replacement_path = fixture.path().join("replacement");
    fs::write(&replacement_path, b"keep").expect("replacement");
    let replacement = OpenOptions::new()
        .read(true)
        .write(true)
        .open(&replacement_path)
        .expect("open replacement");
    *temporary.as_file_mut().expect("original handle") = replacement;
    let error = temporary
        .seal_with_options(&LocalSealOptions::new())
        .expect_err("replacement rejected");
    assert_eq!(error.access_state(), LocalTempAccessState::Writable);
    let mut recovered = error.into_resource();
    recovered.cleanup().expect("cleanup original temp");
    assert_eq!(fs::read(replacement_path).expect("read replacement"), b"keep");
}

#[cfg(unix)]
#[test]
fn test_lf3_temp_required_privacy_is_applied_at_creation() {
    use std::os::unix::fs::PermissionsExt;

    let fixture = tempdir().expect("fixture");
    let filesystem = LocalFileSystem::host().expect("host filesystem");
    let file = filesystem
        .create_temp_file_with_options(
            &LocalTempFileOptions::new()
                .with_parent(fixture.path())
                .with_privacy(LocalPrivacyRequirement::Required),
        )
        .expect("private file");
    assert_eq!(
        fs::metadata(file.path()).expect("file metadata").permissions().mode() & 0o777,
        0o600
    );
    let mut file = file;
    file.cleanup().expect("file cleanup");

    let directory = filesystem
        .create_temp_directory_with_options(
            &LocalTempDirectoryOptions::new()
                .with_parent(fixture.path())
                .with_privacy(LocalPrivacyRequirement::Required),
        )
        .expect("private directory");
    assert_eq!(
        fs::metadata(directory.path())
            .expect("directory metadata")
            .permissions()
            .mode()
            & 0o777,
        0o700
    );
    let mut directory = directory;
    directory.cleanup().expect("directory cleanup");
}

#[cfg(unix)]
#[test]
fn test_lf3_temp_required_privacy_is_applied_under_rooted_authority() {
    use std::os::unix::fs::PermissionsExt;

    let fixture = tempdir().expect("fixture");
    let filesystem = LocalFileSystem::rooted(fixture.path()).expect("rooted filesystem");
    let file = filesystem
        .create_temp_file_with_options(&LocalTempFileOptions::new().with_privacy(LocalPrivacyRequirement::Required))
        .expect("private rooted file");
    let file_path = file.path().strip_prefix("/").unwrap_or(file.path());
    assert_eq!(
        fs::metadata(fixture.path().join(file_path))
            .expect("file metadata")
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
    let mut file = file;
    file.cleanup().expect("file cleanup");

    let directory = filesystem
        .create_temp_directory_with_options(
            &LocalTempDirectoryOptions::new().with_privacy(LocalPrivacyRequirement::Required),
        )
        .expect("private rooted directory");
    let directory_path = directory.path().strip_prefix("/").unwrap_or(directory.path());
    assert_eq!(
        fs::metadata(fixture.path().join(directory_path))
            .expect("directory metadata")
            .permissions()
            .mode()
            & 0o777,
        0o700
    );
    let mut directory = directory;
    directory.cleanup().expect("directory cleanup");
}

#[test]
fn test_lf3_temp_seal_returns_read_only_owner() {
    let fixture = tempdir().expect("fixture");
    let filesystem = LocalFileSystem::host().expect("host filesystem");
    let temporary = filesystem
        .create_temp_file_with_options(&LocalTempFileOptions::new().with_parent(fixture.path()))
        .expect("temporary file");
    let sealed = temporary.seal_with_options(&LocalSealOptions::new()).expect("seal");
    assert!(sealed.observation().expect("observation").identity().is_some());
    assert!(OpenOptions::new().write(true).open(sealed.path()).is_err());
    let mut sealed = sealed;
    sealed.cleanup().expect("cleanup");
}

#[test]
fn test_lf3_temp_sealed_native_path_follows_authority_kind() {
    let fixture = tempdir().expect("fixture");
    let host = LocalFileSystem::host().expect("host filesystem");
    let host_temp = host
        .create_temp_file_with_options(&LocalTempFileOptions::new().with_parent(fixture.path()))
        .expect("host temp");
    let host_sealed = host_temp
        .seal_with_options(&LocalSealOptions::new())
        .expect("host seal");
    assert_eq!(host_sealed.native_path(), Some(host_sealed.path()));
    let mut host_sealed = host_sealed;
    host_sealed.cleanup().expect("host cleanup");

    let rooted = LocalFileSystem::rooted(fixture.path()).expect("rooted filesystem");
    let rooted_temp = rooted
        .create_temp_file_with_options(&LocalTempFileOptions::new())
        .expect("rooted temp");
    let rooted_sealed = rooted_temp
        .seal_with_options(&LocalSealOptions::new())
        .expect("rooted seal");
    assert!(rooted_sealed.native_path().is_none());
    let mut rooted_sealed = rooted_sealed;
    rooted_sealed.cleanup().expect("rooted cleanup");
}

#[cfg(windows)]
#[test]
fn test_lf3_temp_windows_required_private_creation_and_seal() {
    let fixture = tempdir().expect("fixture");
    let filesystem = LocalFileSystem::host().expect("host filesystem");
    let file = filesystem
        .create_temp_file_with_options(
            &LocalTempFileOptions::new()
                .with_parent(fixture.path())
                .with_privacy(LocalPrivacyRequirement::Required),
        )
        .expect("protected host temp");
    let sealed = file.seal_with_options(&LocalSealOptions::new()).expect("host seal");
    assert_eq!(sealed.native_path(), Some(sealed.path()));
    let mut sealed = sealed;
    sealed.cleanup().expect("host cleanup");

    let rooted = LocalFileSystem::rooted(fixture.path()).expect("rooted filesystem");
    let directory = rooted
        .create_temp_directory_with_options(
            &LocalTempDirectoryOptions::new().with_privacy(LocalPrivacyRequirement::Required),
        )
        .expect("protected rooted temp directory");
    let mut directory = directory;
    directory.cleanup().expect("rooted cleanup");
}
