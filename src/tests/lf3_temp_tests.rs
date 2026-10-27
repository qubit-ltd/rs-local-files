use std::fs;
use std::fs::OpenOptions;

use crate::LocalFileSystem;
use crate::LocalSealOptions;
use crate::LocalTempAccessState;
use crate::options::LocalTempFileOptions;
use crate::options::LocalTempDirectoryOptions;
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
        resource.cleanup().expect("retained resource cleanup");
    }
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
        },
    );
    assert!(unknown.cleanup().is_err());
    assert_eq!(fs::read(unknown_path).expect("unknown entry remains"), b"unknown");
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
