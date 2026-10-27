use std::fs;
use std::fs::OpenOptions;

use crate::LocalFileSystem;
use crate::LocalSealOptions;
use crate::LocalTempAccessState;
use crate::options::LocalTempFileOptions;
use crate::options::LocalTempDirectoryOptions;
use crate::options::LocalTempCleanupLimits;
use crate::policy::LocalPrivacyRequirement;

#[test]
fn lf3_temp_seal_returns_read_only_owner() {
    let fixture = tempfile::tempdir().expect("fixture");
    let filesystem = LocalFileSystem::host().expect("host filesystem");
    let temporary = filesystem
        .create_temp_file_with_options(&LocalTempFileOptions::new().with_parent(fixture.path()))
        .expect("temporary file");
    let sealed = temporary
        .seal_with_options(&LocalSealOptions::new())
        .expect("seal");
    assert!(
        sealed
            .observation()
            .expect("observation")
            .identity()
            .is_some()
    );
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
    assert_eq!(
        fs::read(replacement_path).expect("read replacement"),
        b"keep"
    );
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
    use crate::error::{
        LocalFileError, LocalFileErrorKind, LocalFileErrorSource, LocalFileOperation,
    };
    use crate::temp::LocalTempRecovery;
    let fixture = tempfile::tempdir().expect("fixture");
    let filesystem = LocalFileSystem::host().expect("host filesystem");
    let resource = filesystem
        .create_temp_file_with_options(&LocalTempFileOptions::new().with_parent(fixture.path()))
        .expect("temporary file");
    let recovery = LocalTempRecovery::from_file(resource);
    let error = LocalFileError::new(
        LocalFileErrorKind::RequirementNotMet,
        LocalFileOperation::Commit,
    )
    .with_temp_recovery(recovery);
    match error.into_source().expect("recovery source") {
        LocalFileErrorSource::Recovery(error) => assert!(error.temp_recovery().is_some()),
        other => panic!("unexpected source: {other:?}"),
    }

    let resource = filesystem
        .create_temp_file_with_options(&LocalTempFileOptions::new().with_parent(fixture.path()))
        .expect("second temporary file");
    let error = LocalFileError::new(
        LocalFileErrorKind::RequirementNotMet,
        LocalFileOperation::Commit,
    )
    .with_temp_recovery(LocalTempRecovery::from_file(resource));
    let io_error = error.into_io_error();
    let source = io_error.into_inner().expect("structured source");
    let structured = *source.downcast::<LocalFileError>().expect("local error source");
    let (_, mut recovery) = structured.into_temp_recovery_parts();
    recovery.as_mut().expect("recovery owner").cleanup().expect("recovery cleanup");
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
    assert_eq!(fs::metadata(file.path()).expect("file metadata").permissions().mode() & 0o777, 0o600);
    let mut file = file;
    file.cleanup().expect("file cleanup");

    let directory = filesystem
        .create_temp_directory_with_options(
            &LocalTempDirectoryOptions::new()
                .with_parent(fixture.path())
                .with_privacy(LocalPrivacyRequirement::Required),
        )
        .expect("private directory");
    assert_eq!(fs::metadata(directory.path()).expect("directory metadata").permissions().mode() & 0o777, 0o700);
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
        .create_temp_file_with_options(
            &LocalTempFileOptions::new().with_privacy(LocalPrivacyRequirement::Required),
        )
        .expect("private rooted file");
    let file_path = file.path().strip_prefix("/").unwrap_or(file.path());
    assert_eq!(fs::metadata(fixture.path().join(file_path)).expect("file metadata").permissions().mode() & 0o777, 0o600);
    let mut file = file;
    file.cleanup().expect("file cleanup");

    let directory = filesystem
        .create_temp_directory_with_options(
            &LocalTempDirectoryOptions::new().with_privacy(LocalPrivacyRequirement::Required),
        )
        .expect("private rooted directory");
    let directory_path = directory.path().strip_prefix("/").unwrap_or(directory.path());
    assert_eq!(fs::metadata(fixture.path().join(directory_path)).expect("directory metadata").permissions().mode() & 0o777, 0o700);
    let mut directory = directory;
    directory.cleanup().expect("directory cleanup");
}

#[cfg(all(feature = "test-support", any(unix, windows)))]
#[test]
fn lf3_temp_private_guard_handle_reservations_precede_child_mutation() {
    use crate::LocalFileError;
    use std::path::Path;

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
                    recovery.as_mut().expect("sandbox recovery owner").cleanup().expect("cleanup recovery");
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
    let host_sealed = host_temp.seal_with_options(&LocalSealOptions::new()).expect("host seal");
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
    use qubit_io::Output;
    use std::io::Seek;
    use std::io::SeekFrom;
    let fixture = tempfile::tempdir().expect("fixture");
    let filesystem = LocalFileSystem::host().expect("host filesystem");
    for (fault, expected_state) in [
        ("temp-seal-file-sync-before", crate::outcome::LocalTempAccessState::Writable),
        ("temp-seal-access-change", crate::outcome::LocalTempAccessState::Indeterminate),
        ("temp-seal-file-sync-after-access", crate::outcome::LocalTempAccessState::ReadOnly),
        ("temp-seal-readonly-reopen", crate::outcome::LocalTempAccessState::ReadOnly),
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
        .create_temp_file_with_options(
            &LocalTempFileOptions::new().with_privacy(LocalPrivacyRequirement::Required),
        )
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
    use crate::observation::{LocalObservationRequirement, query};
    use crate::temp::types::NativePrivateEntry;
    use std::ffi::OsString;
    use std::fs::File;

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
    let unknown_file = OpenOptions::new().read(true).open(&unknown_path).expect("unknown handle");
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
