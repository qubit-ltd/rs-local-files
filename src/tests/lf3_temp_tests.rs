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
    let source = io_error.get_ref().expect("structured source");
    let structured = source
        .downcast_ref::<LocalFileError>()
        .expect("local error source");
    assert!(structured.temp_recovery().is_some());
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
