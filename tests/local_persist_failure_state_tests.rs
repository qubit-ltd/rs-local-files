// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Public persistence failure-state coverage.

use std::fs;
use std::path::Path;

use qubit_local_files::LocalFileSystem;
use qubit_local_files::options::LocalTempDirectoryOptions;
use qubit_local_files::options::LocalTempFileOptions;
use qubit_local_files::outcome::LocalPersistFailureState;
use qubit_local_files::outcome::LocalPersistStage;
use qubit_local_files::outcome::LocalTempSourceState;

/// Verifies publication and source authority remain separate after replacement.
#[test]
fn test_temp_source_replacement_host_file() {
    let parent = tempfile::tempdir().expect("create fixture parent");
    let filesystem = LocalFileSystem::host().expect("open authority");
    let creation = parent.path();
    let resource = filesystem
        .create_temp_file_with_options(&LocalTempFileOptions::new().with_parent(creation))
        .expect("create source");
    let source = resource.path().to_path_buf();
    let original = parent.path().join("original");
    let target = creation.join("target");
    fs::rename(&source, &original).expect("retain original entity");

    fs::write(&source, b"replacement").expect("create replacement payload");
    let error = resource.persist(&target).expect_err("reject replacement");
    assert_eq!(error.state(), LocalPersistFailureState::NotPublished);
    assert_eq!(error.source_state(), LocalTempSourceState::Indeterminate);
    let mut parts = error.into_parts();
    assert_eq!(parts.source_state, LocalTempSourceState::Indeterminate);
    assert_eq!(parts.resource.source_state(), LocalTempSourceState::Indeterminate);
    assert!(parts.resource.cleanup().is_err());
    assert!(parts.resource.as_file_mut().is_err());
    drop(parts);
    assert!(!parent.path().join("target").exists());
    assert_eq!(fs::read(&source).expect("read replacement"), b"replacement");
    assert!(original.exists());
}

/// Verifies publication and source authority remain separate after replacement.
#[test]
fn test_temp_source_replacement_host_directory() {
    let parent = tempfile::tempdir().expect("create fixture parent");
    let filesystem = LocalFileSystem::host().expect("open authority");
    let creation = parent.path();
    let resource = filesystem
        .create_temp_directory_with_options(&LocalTempDirectoryOptions::new().with_parent(creation))
        .expect("create source");
    let source = resource.path().to_path_buf();
    let original = parent.path().join("original");
    let target = creation.join("target");
    fs::rename(&source, &original).expect("retain original entity");
    fs::create_dir(&source).expect("create replacement");
    fs::write(source.join("sentinel"), b"replacement").expect("create replacement payload");
    let error = resource.persist(&target).expect_err("reject replacement");
    assert_eq!(error.state(), LocalPersistFailureState::NotPublished);
    assert_eq!(error.source_state(), LocalTempSourceState::Indeterminate);
    let mut parts = error.into_parts();
    assert_eq!(parts.source_state, LocalTempSourceState::Indeterminate);
    assert_eq!(parts.resource.source_state(), LocalTempSourceState::Indeterminate);
    assert!(parts.resource.cleanup().is_err());

    drop(parts);
    assert!(!parent.path().join("target").exists());
    assert_eq!(
        fs::read(source.join("sentinel")).expect("read replacement"),
        b"replacement"
    );
    assert!(original.exists());
}

/// Verifies publication and source authority remain separate after replacement.
#[test]
fn test_temp_source_replacement_rooted_file() {
    let parent = tempfile::tempdir().expect("create fixture parent");
    let filesystem = LocalFileSystem::rooted(parent.path()).expect("open authority");
    let creation = Path::new(std::path::MAIN_SEPARATOR_STR);
    let resource = filesystem
        .create_temp_file_with_options(&LocalTempFileOptions::new().with_parent(creation))
        .expect("create source");
    let source = parent
        .path()
        .join(resource.path().strip_prefix(creation).expect("virtual root"));
    let original = parent.path().join("original");
    let target = creation.join("target");
    fs::rename(&source, &original).expect("retain original entity");

    fs::write(&source, b"replacement").expect("create replacement payload");
    let error = resource.persist(&target).expect_err("reject replacement");
    assert_eq!(error.state(), LocalPersistFailureState::NotPublished);
    assert_eq!(error.source_state(), LocalTempSourceState::Indeterminate);
    let mut parts = error.into_parts();
    assert_eq!(parts.source_state, LocalTempSourceState::Indeterminate);
    assert_eq!(parts.resource.source_state(), LocalTempSourceState::Indeterminate);
    assert!(parts.resource.cleanup().is_err());
    assert!(parts.resource.as_file_mut().is_err());
    drop(parts);
    assert!(!parent.path().join("target").exists());
    assert_eq!(fs::read(&source).expect("read replacement"), b"replacement");
    assert!(original.exists());
}

/// Verifies publication and source authority remain separate after replacement.
#[test]
fn test_temp_source_replacement_rooted_directory() {
    let parent = tempfile::tempdir().expect("create fixture parent");
    let filesystem = LocalFileSystem::rooted(parent.path()).expect("open authority");
    let creation = Path::new(std::path::MAIN_SEPARATOR_STR);
    let resource = filesystem
        .create_temp_directory_with_options(&LocalTempDirectoryOptions::new().with_parent(creation))
        .expect("create source");
    let source = parent
        .path()
        .join(resource.path().strip_prefix(creation).expect("virtual root"));
    let original = parent.path().join("original");
    let target = creation.join("target");
    fs::rename(&source, &original).expect("retain original entity");
    fs::create_dir(&source).expect("create replacement");
    fs::write(source.join("sentinel"), b"replacement").expect("create replacement payload");
    let error = resource.persist(&target).expect_err("reject replacement");
    assert_eq!(error.state(), LocalPersistFailureState::NotPublished);
    assert_eq!(error.source_state(), LocalTempSourceState::Indeterminate);
    let mut parts = error.into_parts();
    assert_eq!(parts.source_state, LocalTempSourceState::Indeterminate);
    assert_eq!(parts.resource.source_state(), LocalTempSourceState::Indeterminate);
    assert!(parts.resource.cleanup().is_err());

    drop(parts);
    assert!(!parent.path().join("target").exists());
    assert_eq!(
        fs::read(source.join("sentinel")).expect("read replacement"),
        b"replacement"
    );
    assert!(original.exists());
}

/// Builds a source authority and matching public parent for lifecycle matrices.
fn state_fixture(rooted: bool) -> (tempfile::TempDir, LocalFileSystem, std::path::PathBuf) {
    let parent = tempfile::tempdir().expect("create matrix parent");
    let filesystem = if rooted {
        LocalFileSystem::rooted(parent.path())
    } else {
        LocalFileSystem::host()
    }
    .expect("open matrix authority");
    let creation = if rooted {
        Path::new(std::path::MAIN_SEPARATOR_STR)
    } else {
        parent.path()
    }
    .to_path_buf();
    (parent, filesystem, creation)
}

/// Conflict retains ownership, permitting another target through both backends.
#[test]
fn test_temp_retry_after_conflict() {
    for rooted in [false, true] {
        let (parent, filesystem, creation) = state_fixture(rooted);
        let resource = filesystem
            .create_temp_file_with_options(&LocalTempFileOptions::new().with_parent(&creation))
            .expect("create source");
        let conflict = creation.join("conflict");
        fs::write(parent.path().join("conflict"), b"existing").expect("create conflict");
        let error = resource.persist(&conflict).expect_err("reject conflict");
        assert_eq!(error.state(), LocalPersistFailureState::NotPublished);
        assert_eq!(error.source_state(), LocalTempSourceState::Owned);
        assert_eq!(error.resource().source_state(), LocalTempSourceState::Owned);
        let outcome = error
            .into_parts()
            .resource
            .persist(creation.join("target"))
            .expect("retry publish");
        assert_eq!(outcome.path(), creation.join("target"));
        assert!(parent.path().join("target").exists());
        assert!(parent.path().join("conflict").exists());
    }
    for rooted in [false, true] {
        let (parent, filesystem, creation) = state_fixture(rooted);
        let resource = filesystem
            .create_temp_directory_with_options(&LocalTempDirectoryOptions::new().with_parent(&creation))
            .expect("create source");
        let conflict = creation.join("conflict");
        fs::create_dir(parent.path().join("conflict")).expect("create conflict");
        let error = resource.persist(&conflict).expect_err("reject conflict");
        assert_eq!(error.state(), LocalPersistFailureState::NotPublished);
        assert_eq!(error.source_state(), LocalTempSourceState::Owned);
        assert_eq!(error.resource().source_state(), LocalTempSourceState::Owned);
        let outcome = error
            .into_parts()
            .resource
            .persist(creation.join("target"))
            .expect("retry publish");
        assert_eq!(outcome.path(), creation.join("target"));
        assert!(parent.path().join("target").exists());
        assert!(parent.path().join("conflict").exists());
    }
}

/// Invalid retry operands and keep cannot restore authority after replacement.
#[test]
fn test_temp_invalid_target_preserves_indeterminate_source() {
    for rooted in [false, true] {
        let (parent, filesystem, creation) = state_fixture(rooted);
        let resource = filesystem
            .create_temp_file_with_options(&LocalTempFileOptions::new().with_parent(&creation))
            .expect("create source");
        let source = if rooted {
            parent
                .path()
                .join(resource.path().strip_prefix(&creation).expect("virtual root"))
        } else {
            resource.path().to_path_buf()
        };
        fs::rename(&source, parent.path().join("original")).expect("retain original");
        fs::write(&source, b"replacement").expect("create replacement");
        let error = resource
            .persist(creation.join("target"))
            .expect_err("reject replacement");
        let mut error = error
            .into_parts()
            .resource
            .persist(Path::new(""))
            .expect_err("reject invalid retry");
        assert_eq!(error.state(), LocalPersistFailureState::NotPublished);
        assert_eq!(error.source_state(), LocalTempSourceState::Indeterminate);
        assert_eq!(error.stage(), LocalPersistStage::InstallDestination);
        assert!(error.resolved_target().is_none());
        assert!(error.resource_mut().cleanup().is_err());
        let mut error = error.into_parts().resource.keep().expect_err("reject keep");
        assert_eq!(error.state(), LocalPersistFailureState::NotPublished);
        assert_eq!(error.source_state(), LocalTempSourceState::Indeterminate);
        assert!(error.resource_mut().cleanup().is_err());
        drop(error);
        assert!(source.exists());
        assert!(parent.path().join("original").exists());
        assert!(!parent.path().join("target").exists());
    }
    for rooted in [false, true] {
        let (parent, filesystem, creation) = state_fixture(rooted);
        let resource = filesystem
            .create_temp_directory_with_options(&LocalTempDirectoryOptions::new().with_parent(&creation))
            .expect("create source");
        let source = if rooted {
            parent
                .path()
                .join(resource.path().strip_prefix(&creation).expect("virtual root"))
        } else {
            resource.path().to_path_buf()
        };
        fs::rename(&source, parent.path().join("original")).expect("retain original");
        fs::create_dir(&source).expect("create replacement");
        let error = resource
            .persist(creation.join("target"))
            .expect_err("reject replacement");
        let mut error = error
            .into_parts()
            .resource
            .persist(Path::new(""))
            .expect_err("reject invalid retry");
        assert_eq!(error.state(), LocalPersistFailureState::NotPublished);
        assert_eq!(error.source_state(), LocalTempSourceState::Indeterminate);
        assert_eq!(error.stage(), LocalPersistStage::InstallDestination);
        assert!(error.resolved_target().is_none());
        assert!(error.resource_mut().cleanup().is_err());
        let mut error = error.into_parts().resource.keep().expect_err("reject keep");
        assert_eq!(error.state(), LocalPersistFailureState::NotPublished);
        assert_eq!(error.source_state(), LocalTempSourceState::Indeterminate);
        assert!(error.resource_mut().cleanup().is_err());
        drop(error);
        assert!(source.exists());
        assert!(parent.path().join("original").exists());
        assert!(!parent.path().join("target").exists());
    }
}

/// Released resources remain released across repeated cleanup and publication
/// rejection.
#[test]
fn test_temp_cleanup_is_idempotent() {
    for rooted in [false, true] {
        let (parent, filesystem, creation) = state_fixture(rooted);
        let mut resource = filesystem
            .create_temp_file_with_options(&LocalTempFileOptions::new().with_parent(&creation))
            .expect("create source");
        assert_eq!(resource.source_state(), LocalTempSourceState::Owned);
        resource.cleanup().expect("remove source and sandbox");
        assert_eq!(resource.source_state(), LocalTempSourceState::Released);
        resource.cleanup().expect("idempotent cleanup");
        let mut error = resource
            .persist(Path::new(""))
            .expect_err("reject released publication");
        assert_eq!(error.state(), LocalPersistFailureState::NotPublished);
        assert_eq!(error.source_state(), LocalTempSourceState::Released);
        error
            .resource_mut()
            .cleanup()
            .expect("error resource cleanup stays idempotent");
        let mut error = error.into_parts().resource.keep().expect_err("reject released keep");
        assert_eq!(error.state(), LocalPersistFailureState::NotPublished);
        assert_eq!(error.source_state(), LocalTempSourceState::Released);
        error.resource_mut().cleanup().expect("released keep resource cleanup");
        drop(error);
        assert_eq!(fs::read_dir(parent.path()).expect("read parent").count(), 0);
    }
    for rooted in [false, true] {
        let (parent, filesystem, creation) = state_fixture(rooted);
        let mut resource = filesystem
            .create_temp_directory_with_options(&LocalTempDirectoryOptions::new().with_parent(&creation))
            .expect("create source");
        assert_eq!(resource.source_state(), LocalTempSourceState::Owned);
        resource.cleanup().expect("remove source and sandbox");
        assert_eq!(resource.source_state(), LocalTempSourceState::Released);
        resource.cleanup().expect("idempotent cleanup");
        let mut error = resource
            .persist(Path::new(""))
            .expect_err("reject released publication");
        assert_eq!(error.state(), LocalPersistFailureState::NotPublished);
        assert_eq!(error.source_state(), LocalTempSourceState::Released);
        error
            .resource_mut()
            .cleanup()
            .expect("error resource cleanup stays idempotent");
        let mut error = error.into_parts().resource.keep().expect_err("reject released keep");
        assert_eq!(error.state(), LocalPersistFailureState::NotPublished);
        assert_eq!(error.source_state(), LocalTempSourceState::Released);
        error.resource_mut().cleanup().expect("released keep resource cleanup");
        drop(error);
        assert_eq!(fs::read_dir(parent.path()).expect("read parent").count(), 0);
    }
}
