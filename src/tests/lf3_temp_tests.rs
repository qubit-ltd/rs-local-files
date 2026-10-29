// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
use std::fs;
use std::fs::OpenOptions;

use crate::LocalFileSystem;
use crate::LocalSealOptions;
use crate::LocalTempAccessState;
use crate::options::LocalTempCleanupLimits;
use crate::options::LocalTempDirectoryOptions;
use crate::options::LocalTempFileOptions;
use crate::policy::LocalPrivacyRequirement;

#[test]
#[cfg(feature = "test-support")]
fn lf_revision_private_attempt_budget_is_optional_and_bounded() {
    for rooted in [false, true] {
        for directory in [false, true] {
            for collision in [
                "temp-private-sandbox-collision",
                if directory {
                    "temp-private-directory-target-collision"
                } else {
                    "temp-private-file-collision"
                },
            ] {
                let fixture = tempfile::tempdir().expect("fixture");
                let host = LocalFileSystem::host().expect("Host");
                let rooted_fs = LocalFileSystem::rooted(fixture.path()).expect("Rooted");
                for limit in [None, Some(128)] {
                    let guard = crate::local::install_test_fault_repeated(collision, 129).expect("bounded fault");
                    let result = match (rooted, directory, limit) {
                        (false, false, None) => host
                            .create_temp_file_with_options(
                                &LocalTempFileOptions::new()
                                    .with_parent(fixture.path())
                                    .with_privacy(LocalPrivacyRequirement::Required),
                            )
                            .map(|_| ()),
                        (false, false, Some(n)) => host
                            .create_temp_file_with_options(
                                &LocalTempFileOptions::new()
                                    .with_parent(fixture.path())
                                    .with_privacy(LocalPrivacyRequirement::Required)
                                    .with_max_attempts(n),
                            )
                            .map(|_| ()),
                        (true, false, None) => rooted_fs
                            .create_temp_file_with_options(
                                &LocalTempFileOptions::new().with_privacy(LocalPrivacyRequirement::Required),
                            )
                            .map(|_| ()),
                        (true, false, Some(n)) => rooted_fs
                            .create_temp_file_with_options(
                                &LocalTempFileOptions::new()
                                    .with_privacy(LocalPrivacyRequirement::Required)
                                    .with_max_attempts(n),
                            )
                            .map(|_| ()),
                        (false, true, None) => host
                            .create_temp_directory_with_options(
                                &LocalTempDirectoryOptions::new()
                                    .with_parent(fixture.path())
                                    .with_privacy(LocalPrivacyRequirement::Required),
                            )
                            .map(|_| ()),
                        (false, true, Some(n)) => host
                            .create_temp_directory_with_options(
                                &LocalTempDirectoryOptions::new()
                                    .with_parent(fixture.path())
                                    .with_privacy(LocalPrivacyRequirement::Required)
                                    .with_max_attempts(n),
                            )
                            .map(|_| ()),
                        (true, true, None) => rooted_fs
                            .create_temp_directory_with_options(
                                &LocalTempDirectoryOptions::new().with_privacy(LocalPrivacyRequirement::Required),
                            )
                            .map(|_| ()),
                        (true, true, Some(n)) => rooted_fs
                            .create_temp_directory_with_options(
                                &LocalTempDirectoryOptions::new()
                                    .with_privacy(LocalPrivacyRequirement::Required)
                                    .with_max_attempts(n),
                            )
                            .map(|_| ()),
                    };
                    assert_eq!(
                        crate::local::test_fault_occurrence_count(),
                        if limit.is_none() { 130 } else { 128 },
                        "collision attempt count"
                    );
                    drop(guard);
                    match (limit, result) {
                        (None, Ok(())) => (),
                        (Some(128), Err(_)) => assert_eq!(fs::read_dir(fixture.path()).expect("entries").count(), 0),
                        (None, Err(error)) => panic!("unbounded budget must recover after 129 faults: {error}"),
                        (Some(128), Ok(_)) => panic!("finite budget must exhaust after 128 attempts"),
                        _ => unreachable!(),
                    }
                    assert_eq!(fs::read_dir(fixture.path()).expect("no sandbox residue").count(), 0);
                }
                for limit in [0, 1] {
                    let guard = crate::local::install_test_fault_repeated(collision, 129).expect("bounded fault");
                    let result = if rooted {
                        if directory {
                            rooted_fs
                                .create_temp_directory_with_options(
                                    &LocalTempDirectoryOptions::new()
                                        .with_privacy(LocalPrivacyRequirement::Required)
                                        .with_max_attempts(limit),
                                )
                                .map(|_| ())
                        } else {
                            rooted_fs
                                .create_temp_file_with_options(
                                    &LocalTempFileOptions::new()
                                        .with_privacy(LocalPrivacyRequirement::Required)
                                        .with_max_attempts(limit),
                                )
                                .map(|_| ())
                        }
                    } else if directory {
                        host.create_temp_directory_with_options(
                            &LocalTempDirectoryOptions::new()
                                .with_parent(fixture.path())
                                .with_privacy(LocalPrivacyRequirement::Required)
                                .with_max_attempts(limit),
                        )
                        .map(|_| ())
                    } else {
                        host.create_temp_file_with_options(
                            &LocalTempFileOptions::new()
                                .with_parent(fixture.path())
                                .with_privacy(LocalPrivacyRequirement::Required)
                                .with_max_attempts(limit),
                        )
                        .map(|_| ())
                    };
                    assert_eq!(
                        crate::local::test_fault_occurrence_count(),
                        limit,
                        "boundary attempt count"
                    );
                    drop(guard);
                    assert!(result.is_err(), "limit {limit} must fail");
                    assert_eq!(fs::read_dir(fixture.path()).expect("no sandbox residue").count(), 0);
                }
            }
        }
    }
}

#[test]
fn lf_review_stage_cleanup_retains_creation_limit() {
    use std::path::Path;

    use crate::error::LocalFileErrorKind;
    use crate::options::LocalDirectoryOpenOptions;

    let fixture = tempfile::tempdir().expect("fixture");
    let fs = LocalFileSystem::host().expect("Host");
    let parent = fs
        .open_directory_with_options(fixture.path(), &LocalDirectoryOpenOptions::new())
        .expect("parent");
    let mut stage = parent
        .create_staged_directory(
            &LocalTempDirectoryOptions::new().with_cleanup_limits(LocalTempCleanupLimits::new().with_max_entries(0)),
        )
        .expect("stage");
    stage
        .write_file_new(Path::new("file"), &mut b"x".as_slice(), 1)
        .expect("write");
    let native = fixture.path().join(stage.relative_name());
    let failure = stage.cleanup().expect_err("zero budget must reject cleanup");
    assert_eq!(failure.cause().kind(), LocalFileErrorKind::ResourceLimit);
    assert!(native.join("file").exists());
    let mut owners = failure.into_retained_resources();
    assert_eq!(owners.len(), 1);
    let mut owner = owners.pop().expect("retained owner");
    owner.set_cleanup_limits(LocalTempCleanupLimits::new());
    owner.cleanup().expect("retry with relaxed budget");
    assert!(!native.exists());
}

#[test]
fn lf_review_stage_cleanup_drop_retains_zero_entry_budget() {
    use std::path::Path;

    use crate::options::LocalDirectoryOpenOptions;

    let fixture = tempfile::tempdir().expect("fixture");
    let fs = LocalFileSystem::host().expect("Host");
    let parent = fs
        .open_directory_with_options(fixture.path(), &LocalDirectoryOpenOptions::new())
        .expect("parent");
    let mut stage = parent
        .create_staged_directory(
            &LocalTempDirectoryOptions::new().with_cleanup_limits(LocalTempCleanupLimits::new().with_max_entries(0)),
        )
        .expect("stage");
    stage
        .write_file_new(Path::new("file"), &mut b"x".as_slice(), 1)
        .expect("write");
    let native = fixture.path().join(stage.relative_name());
    drop(stage);
    assert!(native.join("file").exists());
    assert!(native.is_dir());
}

#[test]
fn lf_review_stage_cleanup_retains_zero_deadline() {
    use std::path::Path;
    use std::time::Duration;

    use crate::error::LocalFileErrorKind;
    use crate::options::LocalDirectoryOpenOptions;

    let fixture = tempfile::tempdir().expect("fixture");
    let fs = LocalFileSystem::host().expect("Host");
    let parent = fs
        .open_directory_with_options(fixture.path(), &LocalDirectoryOpenOptions::new())
        .expect("parent");
    let mut stage = parent
        .create_staged_directory(
            &LocalTempDirectoryOptions::new()
                .with_cleanup_limits(LocalTempCleanupLimits::new().with_deadline(Duration::ZERO)),
        )
        .expect("stage");
    stage
        .write_file_new(Path::new("file"), &mut b"x".as_slice(), 1)
        .expect("write");
    let native = fixture.path().join(stage.relative_name());
    let failure = stage.cleanup().expect_err("zero deadline must reject cleanup");
    assert_eq!(failure.cause().kind(), LocalFileErrorKind::ResourceLimit);
    assert!(native.join("file").exists());
    let mut owners = failure.into_retained_resources();
    let mut owner = owners.pop().expect("retained owner");
    owner.set_cleanup_limits(LocalTempCleanupLimits::new());
    owner.cleanup().expect("retry with relaxed deadline");
    assert!(!native.exists());
}

#[test]
fn lf_review_stage_cleanup_creation_failure_retains_creation_limit() {
    use crate::error::LocalFileErrorKind;
    use crate::options::LocalDirectoryOpenOptions;

    let fixture = tempfile::tempdir().expect("fixture");
    let fs = LocalFileSystem::host().expect("Host");
    let parent = fs
        .open_directory_with_options(fixture.path(), &LocalDirectoryOpenOptions::new())
        .expect("parent");
    let fixture_path = fixture.path().to_path_buf();
    let original_stage = fixture.path().join("original-stage");
    let original_stage_for_hook = original_stage.clone();
    let _hook = crate::directory::install_stage_created_hook(move |name| {
        let stage = fixture_path.join(name);
        std::fs::rename(&stage, &original_stage_for_hook).expect("move created stage");
        std::fs::write(&stage, b"residual").expect("replace with residual file");
    });
    let failure = parent
        .create_staged_directory(
            &LocalTempDirectoryOptions::new()
                .with_privacy(LocalPrivacyRequirement::Required)
                .with_cleanup_limits(LocalTempCleanupLimits::new().with_max_entries(0)),
        )
        .expect_err("opening the replaced stage must fail");
    assert_eq!(failure.cause().kind(), LocalFileErrorKind::NotDirectory);
    let path = failure
        .retained_resources()
        .first()
        .expect("creation failure owner")
        .diagnostic_path()
        .to_path_buf();
    assert!(path.is_file());
    std::fs::remove_file(&path).expect("remove injected replacement");
    std::fs::rename(&original_stage, &path).expect("restore original stage identity");
    let mut owners = failure.into_retained_resources();
    assert_eq!(owners.len(), 1);
    let owner = owners.pop().expect("retained owner");
    let error = owner.cleanup().expect_err("creation limit must be preserved");
    assert_eq!(error.cause().kind(), LocalFileErrorKind::ResourceLimit);
    assert!(path.exists());
    let mut owners = error.into_retained_resources();
    let mut owner = owners.pop().expect("owner retained after cleanup failure");
    owner.set_cleanup_limits(LocalTempCleanupLimits::new());
    owner.cleanup().expect("retry with relaxed budget");
    assert!(!path.exists());
}

#[test]
fn lf3_temp_seal_returns_read_only_owner() {
    let fixture = tempfile::tempdir().expect("fixture");
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
fn lf3_temp_replaced_handle_cannot_seal() {
    let fixture = tempfile::tempdir().expect("fixture");
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

#[test]
fn lf3_temp_closed_handle_cannot_seal() {
    let fixture = tempfile::tempdir().expect("fixture");
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
fn lf3_temp_cleanup_replacement_is_preserved() {
    let fixture = tempfile::tempdir().expect("fixture");
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
fn lf3_temp_cleanup_limit_retains_owner_for_retry() {
    let fixture = tempfile::tempdir().expect("fixture");
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
fn lf3_temp_recovery_survives_error_source_and_io_conversion() {
    use crate::error::LocalFileError;
    use crate::error::LocalFileErrorKind;
    use crate::error::LocalFileErrorSource;
    use crate::error::LocalFileOperation;
    use crate::temp::LocalTempRecovery;
    let fixture = tempfile::tempdir().expect("fixture");
    let filesystem = LocalFileSystem::host().expect("host filesystem");
    let resource = filesystem
        .create_temp_file_with_options(&LocalTempFileOptions::new().with_parent(fixture.path()))
        .expect("temporary file");
    let recovery = LocalTempRecovery::from_file(resource);
    let error = LocalFileError::new(LocalFileErrorKind::RequirementNotMet, LocalFileOperation::Commit)
        .with_temp_recovery(recovery);
    match error.into_source().expect("recovery source") {
        LocalFileErrorSource::Recovery(error) => assert!(error.temp_recovery().is_some()),
        other => panic!("unexpected source: {other:?}"),
    }

    let resource = filesystem
        .create_temp_file_with_options(&LocalTempFileOptions::new().with_parent(fixture.path()))
        .expect("second temporary file");
    let error = LocalFileError::new(LocalFileErrorKind::RequirementNotMet, LocalFileOperation::Commit)
        .with_temp_recovery(LocalTempRecovery::from_file(resource));
    let io_error = error.into_io_error();
    let source = io_error.into_inner().expect("structured source");
    let structured = *source.downcast::<LocalFileError>().expect("local error source");
    let (_, mut recovery) = structured.into_temp_recovery_parts();
    recovery
        .as_mut()
        .expect("recovery owner")
        .cleanup()
        .expect("recovery cleanup");
}

#[cfg(unix)]
#[test]
fn lf3_temp_required_privacy_is_applied_at_creation() {
    use std::os::unix::fs::PermissionsExt;

    let fixture = tempfile::tempdir().expect("fixture");
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
fn lf3_temp_required_privacy_is_applied_under_rooted_authority() {
    use std::os::unix::fs::PermissionsExt;

    let fixture = tempfile::tempdir().expect("fixture");
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

#[cfg(all(feature = "test-support", any(unix, windows)))]
#[test]
fn lf3_temp_private_guard_handle_reservations_precede_child_mutation() {
    use std::path::Path;

    use crate::LocalFileError;

    fn create(
        filesystem: &LocalFileSystem,
        rooted: bool,
        directory: bool,
        parent: &Path,
    ) -> Result<(), LocalFileError> {
        if directory {
            let options = if rooted {
                LocalTempDirectoryOptions::new().with_privacy(LocalPrivacyRequirement::Required)
            } else {
                LocalTempDirectoryOptions::new()
                    .with_parent(parent)
                    .with_privacy(LocalPrivacyRequirement::Required)
            };
            filesystem.create_temp_directory_with_options(&options).map(drop)
        } else {
            let options = if rooted {
                LocalTempFileOptions::new().with_privacy(LocalPrivacyRequirement::Required)
            } else {
                LocalTempFileOptions::new()
                    .with_parent(parent)
                    .with_privacy(LocalPrivacyRequirement::Required)
            };
            filesystem.create_temp_file_with_options(&options).map(drop)
        }
    }

    for rooted in [false, true] {
        for directory in [false, true] {
            for (fault, retains_recovery) in [
                ("temp-private-parent-clone", false),
                ("temp-private-sandbox-clone", true),
            ] {
                let fixture = tempfile::tempdir().expect("fixture");
                let filesystem = if rooted {
                    LocalFileSystem::rooted(fixture.path()).expect("rooted filesystem")
                } else {
                    LocalFileSystem::host().expect("host filesystem")
                };
                let guard = crate::local::install_test_fault(fault).expect("fault controller");
                let error = create(&filesystem, rooted, directory, fixture.path()).expect_err("clone failure");
                drop(guard);
                if retains_recovery {
                    let (_, mut recovery) = error.into_temp_recovery_parts();
                    recovery
                        .as_mut()
                        .expect("sandbox recovery owner")
                        .cleanup()
                        .expect("cleanup recovery");
                } else {
                    let (_, recovery) = error.into_temp_recovery_parts();
                    assert!(recovery.is_none(), "parent clone failed before mutation");
                }
                assert_eq!(fs::read_dir(fixture.path()).expect("fixture entries").count(), 0);
            }
        }
    }
}

#[test]
fn lf3_temp_sealed_native_path_follows_authority_kind() {
    let fixture = tempfile::tempdir().expect("fixture");
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

#[cfg(feature = "test-support")]
#[test]
fn lf3_temp_seal_faults_retain_owner_and_access_state() {
    use std::io::Seek;
    use std::io::SeekFrom;

    use qubit_io::Output;
    let fixture = tempfile::tempdir().expect("fixture");
    let filesystem = LocalFileSystem::host().expect("host filesystem");
    for (fault, expected_state) in [
        (
            "temp-seal-file-sync-before",
            crate::outcome::LocalTempAccessState::Writable,
        ),
        (
            "temp-seal-access-change",
            crate::outcome::LocalTempAccessState::Indeterminate,
        ),
        (
            "temp-seal-file-sync-after-access",
            crate::outcome::LocalTempAccessState::ReadOnly,
        ),
        (
            "temp-seal-readonly-reopen",
            crate::outcome::LocalTempAccessState::ReadOnly,
        ),
    ] {
        let temporary = filesystem
            .create_temp_file_with_options(&LocalTempFileOptions::new().with_parent(fixture.path()))
            .expect("temporary file");
        let guard = crate::local::install_test_fault(fault).expect("fault controller");
        let error = temporary
            .seal_with_options(&LocalSealOptions::new())
            .expect_err("injected seal failure");
        assert_eq!(error.access_state(), expected_state);
        let mut resource = error.into_resource();
        drop(guard);
        if expected_state != crate::outcome::LocalTempAccessState::Writable {
            assert!(resource.as_file_mut().is_err());
            assert!(resource.write_fully(b"blocked").is_err());
            assert!(Seek::seek(&mut resource, SeekFrom::Start(0)).is_err());
            let error = resource.keep().expect_err("sealed temp cannot be published");
            resource = error.into_parts().resource;
        }
        resource.cleanup().expect("retained resource cleanup");
    }
}

#[cfg(feature = "test-support")]
#[test]
fn lf3_temp_created_file_identity_failure_returns_recovery_owner() {
    let fixture = tempfile::tempdir().expect("fixture");
    let filesystem = LocalFileSystem::host().expect("host filesystem");
    let guard = crate::local::install_test_fault("temp-identity-file").expect("fault controller");
    let error = filesystem
        .create_temp_file_with_options(
            &LocalTempFileOptions::new()
                .with_parent(fixture.path())
                .with_privacy(LocalPrivacyRequirement::Required),
        )
        .expect_err("injected identity failure");
    drop(guard);
    let (error, mut recovery) = error.into_temp_recovery_parts();
    assert!(error.temp_recovery().is_none());
    let mut recovery = recovery.take().expect("partial native owner");
    let path = recovery.path().to_path_buf();
    let sandbox = path.parent().expect("private sandbox").to_path_buf();
    recovery.cleanup().expect("cleanup partial entry");
    assert!(!path.exists());
    assert!(!sandbox.exists());
}

#[cfg(feature = "test-support")]
#[test]
fn lf3_temp_private_creation_retries_collisions_and_cleans_own_sandbox() {
    let fixture = tempfile::tempdir().expect("fixture");
    let filesystem = LocalFileSystem::host().expect("host filesystem");
    let fault = crate::local::install_test_fault("temp-private-file-collision").expect("fault controller");
    let file = filesystem
        .create_temp_file_with_options(
            &LocalTempFileOptions::new()
                .with_parent(fixture.path())
                .with_privacy(LocalPrivacyRequirement::Required),
        )
        .expect("collision retry succeeds");
    drop(fault);
    let mut file = file;
    file.cleanup().expect("file cleanup");

    let rooted = LocalFileSystem::rooted(fixture.path()).expect("rooted filesystem");
    let fault = crate::local::install_test_fault("temp-private-file-collision").expect("fault controller");
    let rooted_file = rooted
        .create_temp_file_with_options(&LocalTempFileOptions::new().with_privacy(LocalPrivacyRequirement::Required))
        .expect("rooted file collision retry succeeds");
    drop(fault);
    let mut rooted_file = rooted_file;
    rooted_file.cleanup().expect("rooted file cleanup");

    let fault = crate::local::install_test_fault("temp-private-directory-target-collision").expect("fault controller");
    let directory = rooted
        .create_temp_directory_with_options(
            &LocalTempDirectoryOptions::new().with_privacy(LocalPrivacyRequirement::Required),
        )
        .expect("directory collision retry succeeds");
    drop(fault);
    let mut directory = directory;
    directory.cleanup().expect("directory cleanup");

    let fault = crate::local::install_test_fault("temp-private-file-collision").expect("fault controller");
    let error = filesystem.create_temp_file_with_options(
        &LocalTempFileOptions::new()
            .with_parent(fixture.path())
            .with_max_attempts(1)
            .with_privacy(LocalPrivacyRequirement::Required),
    );
    drop(fault);
    assert!(error.is_err());
    assert_eq!(fs::read_dir(fixture.path()).expect("fixture entries").count(), 0);
}

#[cfg(unix)]
#[test]
fn lf3_temp_recovery_unknown_or_replaced_entries_are_not_removed() {
    use std::ffi::OsString;
    use std::fs::File;

    use crate::observation::LocalObservationRequirement;
    use crate::observation::query;
    use crate::temp::types::NativePrivateEntry;

    let fixture = tempfile::tempdir().expect("fixture");
    let parent = File::open(fixture.path()).expect("parent handle");
    let path = fixture.path().join("entry");
    fs::write(&path, b"original").expect("original entry");
    let original = OpenOptions::new().read(true).open(&path).expect("original handle");
    let identity = query::query_observation(&original, LocalObservationRequirement::Required)
        .expect("identity")
        .identity()
        .cloned();
    let mut recovery = crate::temp::LocalTempRecovery::from_native(
        path.clone(),
        NativePrivateEntry {
            parent: parent.try_clone().expect("clone parent"),
            name: OsString::from("entry"),
            file: Some(original),
            identity,
            directory: false,
        },
    );
    fs::remove_file(&path).expect("remove original name");
    fs::write(&path, b"replacement").expect("replacement entry");
    assert!(recovery.cleanup().is_err());
    assert_eq!(fs::read(&path).expect("replacement remains"), b"replacement");

    let unknown_path = fixture.path().join("unknown");
    fs::write(&unknown_path, b"unknown").expect("unknown entry");
    let unknown_file = OpenOptions::new()
        .read(true)
        .open(&unknown_path)
        .expect("unknown handle");
    let mut unknown = crate::temp::LocalTempRecovery::from_native(
        unknown_path.clone(),
        NativePrivateEntry {
            parent,
            name: OsString::from("unknown"),
            file: Some(unknown_file),
            identity: None,
            directory: false,
        },
    );
    assert!(unknown.cleanup().is_err());
    assert_eq!(fs::read(unknown_path).expect("unknown entry remains"), b"unknown");

    let directory_path = fixture.path().join("native-directory");
    fs::create_dir(&directory_path).expect("native directory");
    let directory_handle = File::open(&directory_path).expect("directory handle");
    let directory_identity = query::query_observation(&directory_handle, LocalObservationRequirement::Required)
        .expect("directory identity")
        .identity()
        .cloned();
    let mut directory_recovery = crate::temp::LocalTempRecovery::from_native(
        directory_path.clone(),
        NativePrivateEntry {
            parent: File::open(fixture.path()).expect("parent handle"),
            name: OsString::from("native-directory"),
            file: Some(directory_handle),
            identity: directory_identity,
            directory: true,
        },
    );
    directory_recovery.cleanup().expect("remove retained directory");
    assert!(!directory_path.exists());
}

#[cfg(unix)]
#[test]
fn lf_revision_recovery_fifo_original_cleanup_and_symlink_are_safe() {
    use std::ffi::OsString;
    use std::fs::File;
    use std::os::unix::fs::symlink;

    let fixture = tempfile::tempdir().expect("fixture");
    let parent = File::open(fixture.path()).expect("parent handle");
    let original_path = fixture.path().join("original");
    fs::write(&original_path, b"original").expect("original");
    let original = OpenOptions::new()
        .read(true)
        .open(&original_path)
        .expect("open original");
    let identity = crate::observation::query::query_observation(
        &original,
        crate::observation::LocalObservationRequirement::Required,
    )
    .expect("identity")
    .identity()
    .cloned();
    let mut recovery = crate::LocalTempRecovery::from_native(
        original_path.clone(),
        crate::temp::types::NativePrivateEntry {
            parent: parent.try_clone().expect("clone parent"),
            name: OsString::from("original"),
            file: Some(original),
            identity,
            directory: false,
        },
    );
    recovery.cleanup().expect("original entry cleanup succeeds");
    assert!(!original_path.exists());

    let target = fixture.path().join("target");
    let link = fixture.path().join("link");
    fs::write(&target, b"target").expect("link target");
    fs::write(&link, b"original link name").expect("link original name");
    let original_link_entry = OpenOptions::new().read(true).open(&link).expect("open original");
    let identity = crate::observation::query::query_observation(
        &original_link_entry,
        crate::observation::LocalObservationRequirement::Required,
    )
    .expect("identity")
    .identity()
    .cloned();
    fs::remove_file(&link).expect("remove original link name");
    symlink(&target, &link).expect("replacement symlink");
    let mut recovery = crate::LocalTempRecovery::from_native(
        link.clone(),
        crate::temp::types::NativePrivateEntry {
            parent,
            name: OsString::from("link"),
            file: Some(original_link_entry),
            identity,
            directory: false,
        },
    );
    assert!(recovery.cleanup().is_err());
    assert!(
        fs::symlink_metadata(&link)
            .expect("replacement link remains")
            .file_type()
            .is_symlink()
    );
    assert_eq!(fs::read(&target).expect("target remains"), b"target");
}

#[cfg(unix)]
#[test]
fn lf_revision_recovery_fifo_explicit_cleanup_does_not_block() {
    use std::process::Command;
    use std::thread;
    use std::time::Duration;
    use std::time::Instant;

    const CHILD: &str = "LF_REVISION_FIFO_CHILD";
    const FIXTURE: &str = "LF_REVISION_FIFO_FIXTURE";
    const MODE: &str = "LF_REVISION_FIFO_MODE";
    const SENTINEL: &str = "LF_REVISION_FIFO_SENTINEL";
    const ENTERED: &str = "LF_REVISION_FIFO_ENTERED";

    if std::env::var_os(CHILD).is_some() {
        recovery_fifo_child(FIXTURE, MODE, SENTINEL, "explicit");
        return;
    }

    let fixture = tempfile::tempdir().expect("fixture");
    let sentinel = fixture.path().join("returned");
    let mut child = Command::new(std::env::current_exe().expect("test executable"))
        .args([
            "--exact",
            "tests::lf3_temp_tests::lf_revision_recovery_fifo_explicit_cleanup_does_not_block",
            "--nocapture",
        ])
        .env(CHILD, "1")
        .env(FIXTURE, fixture.path())
        .env(MODE, "explicit")
        .env(SENTINEL, &sentinel)
        .env(ENTERED, fixture.path().join("entered"))
        .spawn()
        .expect("spawn isolated cleanup child");
    let deadline = Instant::now() + Duration::from_secs(5);
    let status = loop {
        if let Some(status) = child.try_wait().expect("poll child") {
            break Some(status);
        }
        if Instant::now() >= deadline {
            child.kill().expect("kill timed out child");
            child.wait().expect("reap timed out child");
            break None;
        }
        thread::sleep(Duration::from_millis(10));
    };
    assert!(
        fixture.path().join("entered").exists(),
        "child never reached explicit cleanup call"
    );
    assert!(
        sentinel.exists(),
        "child never returned from explicit cleanup; status={status:?}"
    );
    assert!(status.expect("child exited").success(), "child cleanup failed");
    assert!(fixture.path().join("entry").exists(), "replacement FIFO must remain");
}

#[cfg(unix)]
#[test]
fn lf_revision_recovery_fifo_drop_does_not_block() {
    use std::process::Command;
    use std::thread;
    use std::time::Duration;
    use std::time::Instant;

    const CHILD: &str = "LF_REVISION_FIFO_CHILD";
    const FIXTURE: &str = "LF_REVISION_FIFO_FIXTURE";
    const MODE: &str = "LF_REVISION_FIFO_MODE";
    const SENTINEL: &str = "LF_REVISION_FIFO_SENTINEL";
    const ENTERED: &str = "LF_REVISION_FIFO_ENTERED";

    if std::env::var_os(CHILD).is_some() {
        recovery_fifo_child(FIXTURE, MODE, SENTINEL, "drop");
        return;
    }

    let fixture = tempfile::tempdir().expect("fixture");
    let sentinel = fixture.path().join("returned");
    let mut child = Command::new(std::env::current_exe().expect("test executable"))
        .args([
            "--exact",
            "tests::lf3_temp_tests::lf_revision_recovery_fifo_drop_does_not_block",
            "--nocapture",
        ])
        .env(CHILD, "1")
        .env(FIXTURE, fixture.path())
        .env(MODE, "drop")
        .env(SENTINEL, &sentinel)
        .env(ENTERED, fixture.path().join("entered"))
        .spawn()
        .expect("spawn isolated Drop child");
    let deadline = Instant::now() + Duration::from_secs(5);
    let status = loop {
        if let Some(status) = child.try_wait().expect("poll child") {
            break Some(status);
        }
        if Instant::now() >= deadline {
            child.kill().expect("kill timed out child");
            child.wait().expect("reap timed out child");
            break None;
        }
        thread::sleep(Duration::from_millis(10));
    };
    assert!(fixture.path().join("entered").exists(), "child never reached Drop call");
    assert!(sentinel.exists(), "child never returned from Drop; status={status:?}");
    assert!(status.expect("child exited").success(), "child Drop failed");
    assert!(fixture.path().join("entry").exists(), "replacement FIFO must remain");
}

#[cfg(unix)]
fn recovery_fifo_child(child_env: &str, mode_env: &str, sentinel_env: &str, expected_mode: &str) {
    use std::ffi::OsString;
    use std::fs::File;
    use std::os::fd::AsRawFd;
    use std::os::unix::ffi::OsStrExt;

    if std::env::var(mode_env).as_deref() != Ok(expected_mode) {
        return;
    }
    assert!(
        std::env::var_os(child_env).is_some(),
        "child sentinel reached test body"
    );
    let fixture = std::path::PathBuf::from(std::env::var_os("LF_REVISION_FIFO_FIXTURE").expect("fixture path"));
    let path = fixture.join("entry");
    let parent = File::open(&fixture).expect("open parent directory");
    fs::write(&path, b"original").expect("create original");
    let original = OpenOptions::new().read(true).open(&path).expect("open original");
    let identity = crate::observation::query::query_observation(
        &original,
        crate::observation::LocalObservationRequirement::Required,
    )
    .expect("original identity")
    .identity()
    .cloned();
    fs::remove_file(&path).expect("remove original namespace entry");
    let name = std::ffi::CString::new(path.file_name().expect("name").as_bytes()).expect("CString");
    assert_eq!(
        unsafe { libc::mkfifoat(parent.as_raw_fd(), name.as_ptr(), 0o600) },
        0,
        "create replacement FIFO"
    );
    let mut recovery = crate::LocalTempRecovery::from_native(
        path.clone(),
        crate::temp::types::NativePrivateEntry {
            parent,
            name: OsString::from("entry"),
            file: Some(original),
            identity,
            directory: false,
        },
    );
    if expected_mode == "explicit" {
        fs::write(
            std::env::var_os("LF_REVISION_FIFO_ENTERED").expect("entered marker"),
            b"entered",
        )
        .expect("write pre-cleanup sentinel");
        assert!(recovery.cleanup().is_err(), "replaced FIFO must fail identity check");
        drop(recovery);
    } else {
        fs::write(
            std::env::var_os("LF_REVISION_FIFO_ENTERED").expect("entered marker"),
            b"entered",
        )
        .expect("write pre-Drop sentinel");
        drop(recovery);
    }
    fs::write(std::env::var_os(sentinel_env).expect("sentinel path"), b"returned")
        .expect("write post-operation sentinel");
    assert!(path.exists(), "replacement FIFO remains after operation");
}

#[cfg(windows)]
#[test]
fn lf3_temp_windows_required_private_creation_and_seal() {
    let fixture = tempfile::tempdir().expect("fixture");
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

#[cfg(windows)]
#[test]
fn lf3_temp_windows_partial_acl_failure_retains_cleanup_owner() {
    use crate::temp::internal::privacy_windows;

    let fixture = tempfile::tempdir().expect("fixture");
    let filesystem = LocalFileSystem::host().expect("host filesystem");
    privacy_windows::fail_once_at(privacy_windows::FailurePoint::AclReadback);
    let error = filesystem
        .create_temp_file_with_options(
            &LocalTempFileOptions::new()
                .with_parent(fixture.path())
                .with_privacy(LocalPrivacyRequirement::Required),
        )
        .expect_err("injected ACL readback failure");
    assert!(error.temp_recovery().and_then(|recovery| recovery.identity()).is_some());
    let (error, mut recovery) = error.into_temp_recovery_parts();
    assert!(error.temp_recovery().is_none());
    let recovery = recovery.as_mut().expect("partial resource owner");
    let path = recovery.path().to_path_buf();
    recovery.cleanup().expect("cleanup ACL partial");
    assert!(!path.exists());
}

#[test]
fn lf3_temp_sealed_observation_after_cleanup_reports_closed_handle() {
    let fixture = tempfile::tempdir().expect("fixture");
    let filesystem = LocalFileSystem::host().expect("host filesystem");
    let temporary = filesystem
        .create_temp_file_with_options(&LocalTempFileOptions::new().with_parent(fixture.path()))
        .expect("temporary file");
    let mut sealed = temporary.seal_with_options(&LocalSealOptions::new()).expect("seal");
    let path = sealed.path().to_path_buf();
    sealed.cleanup().expect("cleanup sealed file");
    let error = sealed
        .observation()
        .expect_err("cleanup closes the retained observation handle");
    assert_eq!(error.operation(), crate::LocalFileOperation::Metadata);
    assert_eq!(error.path(), Some(path.as_path()));
    assert_eq!(error.io_error_kind(), std::io::ErrorKind::BrokenPipe);
    assert_eq!(
        error.io_error().expect("native closed-handle cause").to_string(),
        "temporary file handle is closed"
    );
    assert!(!path.exists());
    sealed.cleanup().expect("repeated cleanup is idempotent");
}

#[cfg(unix)]
#[test]
fn lf3_temp_host_directory_persist_reports_real_residual_sandbox() {
    let fixture = tempfile::tempdir().expect("fixture");
    let filesystem = LocalFileSystem::host().expect("host filesystem");
    let temporary = filesystem
        .create_temp_directory_with_options(&LocalTempDirectoryOptions::new().with_parent(fixture.path()))
        .expect("temporary directory");
    let source = temporary.path().to_path_buf();
    fs::write(source.join("payload"), b"published payload").expect("write source content");
    let sandbox = source.parent().expect("private sandbox").to_path_buf();
    let residual = sandbox.join("unexpected-sibling");
    fs::write(&residual, b"preserve sibling").expect("make sandbox nonempty after publication");
    let target = fixture.path().join("published");

    let outcome = temporary
        .persist(&target)
        .expect("publication survives sandbox cleanup failure");
    assert_eq!(outcome.path(), target);
    assert!(outcome.atomic());
    assert!(!outcome.durable());
    assert_eq!(
        outcome.cleanup_state(),
        crate::LocalPersistCleanupState::ResidualSandbox
    );
    let error = outcome.cleanup_error().expect("native cleanup failure retained");
    assert_eq!(error.operation(), crate::LocalFileOperation::Cleanup);
    assert_eq!(error.path(), Some(sandbox.as_path()));
    assert_eq!(
        error
            .io_error()
            .expect("native nonempty-directory error")
            .raw_os_error(),
        Some(libc::ENOTEMPTY)
    );
    assert_eq!(
        fs::read(target.join("payload")).expect("read published payload"),
        b"published payload"
    );
    assert_eq!(
        fs::read(&residual).expect("unrelated sandbox entry survives"),
        b"preserve sibling"
    );
    assert!(!source.exists());
}

#[cfg(feature = "test-support")]
#[test]
fn lf3_temp_default_seal_sync_policy_controls_native_sync_and_failure_diagnostics() {
    use crate::policy::LocalFileSyncRequirement;

    assert_eq!(
        LocalSealOptions::default().file_sync(),
        LocalFileSyncRequirement::Required
    );
    for requirement in [
        LocalFileSyncRequirement::Required,
        LocalFileSyncRequirement::NotRequired,
    ] {
        let fixture = tempfile::tempdir().expect("fixture");
        let temporary = LocalFileSystem::host()
            .expect("host")
            .create_temp_file_with_options(&LocalTempFileOptions::new().with_parent(fixture.path()))
            .expect("temporary file");
        let path = temporary.path().to_path_buf();
        let fault = crate::local::install_test_fault("temp-seal-file-sync-before").expect("sync fault");
        let options = LocalSealOptions::default().with_file_sync(requirement);
        let result = temporary.seal_with_options(&options);
        if requirement == LocalFileSyncRequirement::Required {
            let error = result.expect_err("default required synchronization propagates a native sync failure");
            assert_eq!(error.access_state(), LocalTempAccessState::Writable);
            assert_eq!(error.cause().operation(), crate::LocalFileOperation::Commit);
            assert_eq!(error.cause().path(), Some(path.as_path()));
            assert_eq!(
                error.to_string(),
                format!("temporary file sealing failed: {}", error.cause())
            );
            assert!(
                error.to_string().contains(
                    &error
                        .cause()
                        .io_error()
                        .expect("native synchronization failure")
                        .to_string()
                )
            );
            let mut resource = error.into_resource();
            drop(fault);
            resource.cleanup().expect("cleanup retained failed seal owner");
        } else {
            let mut sealed = result.expect("not-required synchronization bypasses native sync failure");
            assert!(
                sealed
                    .observation()
                    .expect("sealed native observation")
                    .identity()
                    .is_some()
            );
            drop(fault);
            sealed.cleanup().expect("cleanup sealed owner");
        }
        assert!(!path.exists());
    }
}

#[cfg(unix)]
#[test]
fn lf3_temp_tree_publish_durability_options_control_namespace_sync_outcome() {
    use std::ffi::OsStr;
    use std::path::Path;

    use crate::LocalDurabilityRequirement;
    use crate::options::LocalDirectoryOpenOptions;
    use crate::options::LocalTreePublishOptions;

    for requirement in [
        LocalDurabilityRequirement::NotRequired,
        LocalDurabilityRequirement::Required,
    ] {
        let fixture = tempfile::tempdir().expect("fixture");
        let parent = LocalFileSystem::host()
            .expect("host")
            .open_directory_with_options(fixture.path(), &LocalDirectoryOpenOptions::new())
            .expect("parent");
        let mut stage = parent
            .create_staged_directory(&LocalTempDirectoryOptions::new())
            .expect("stage");
        stage
            .write_file_new(Path::new("payload"), &mut &b"durable payload"[..], 32)
            .expect("staged payload");
        let source = fixture.path().join(stage.relative_name());
        let outcome = stage
            .publish_new(
                &parent,
                OsStr::new("published"),
                &LocalTreePublishOptions::default().with_durability(requirement),
            )
            .expect("publish with selected namespace synchronization policy");
        assert_eq!(outcome.publication(), crate::outcome::LocalTreePublication::Published);
        assert_eq!(outcome.durable(), requirement == LocalDurabilityRequirement::Required);
        assert!(!source.exists());
        assert_eq!(
            fs::read(fixture.path().join("published/payload")).expect("published contents"),
            b"durable payload"
        );
    }
}
