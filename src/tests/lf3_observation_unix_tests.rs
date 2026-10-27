use std::fs::File;
use std::io;

use crate::error::LocalFileErrorKind;
use crate::error::LocalFileErrorSource;
use crate::observation::LocalObservationRequirement::Optional;
use crate::observation::LocalObservationRequirement::Required;
use crate::observation::unix::query_native;
use crate::observation::unix::query_native_with;

#[test]
fn lf3_native_unix_refreshes_same_handle() {
    let directory = tempfile::tempdir().expect("temporary directory should be created");
    let path = directory.path().join("observed");
    std::fs::write(&path, b"a").expect("fixture should be written");
    let file = File::open(&path).expect("fixture should be opened");
    let before = query_native(&file, Required).expect("initial native observation should work");
    std::fs::write(&path, b"abcd").expect("fixture should be updated");
    let after = query_native(&file, Required).expect("refreshed native observation should work");

    assert_eq!(before.metadata().len(), 1);
    assert_eq!(after.metadata().len(), 4);
    assert_eq!(before.identity(), after.identity());
    assert!(after.native_details().and_then(|details| details.unix()).is_some());
}

#[test]
fn lf3_native_unix_optional_extension_failure_keeps_metadata() {
    let directory = tempfile::tempdir().expect("temporary directory should be created");
    let path = directory.path().join("observed");
    std::fs::write(&path, b"data").expect("fixture should be written");
    let file = File::open(&path).expect("fixture should be opened");
    let observation = query_native_with(&file, Optional, |_| Err(io::Error::from_raw_os_error(libc::EIO)))
        .expect("optional native extension failure should preserve metadata");

    assert_eq!(observation.metadata().len(), 4);
    assert!(observation.identity().is_none());
    assert!(observation.native_details().is_none());
}

#[test]
fn lf3_native_unix_required_extension_failure_preserves_io_source() {
    let directory = tempfile::tempdir().expect("temporary directory should be created");
    let path = directory.path().join("observed");
    std::fs::write(&path, b"data").expect("fixture should be written");
    let file = File::open(&path).expect("fixture should be opened");
    let error = query_native_with(&file, Required, |_| Err(io::Error::from_raw_os_error(libc::EIO)))
        .expect_err("required native extension failure should fail");

    assert_eq!(error.kind(), LocalFileErrorKind::Io);
    assert!(
        matches!(error.typed_source(), Some(LocalFileErrorSource::Io(source)) if source.raw_os_error() == Some(libc::EIO))
    );
}

#[test]
fn lf3_native_unix_rejects_out_of_range_nanoseconds_when_required() {
    let directory = tempfile::tempdir().expect("temporary directory should be created");
    let path = directory.path().join("observed");
    std::fs::write(&path, b"data").expect("fixture should be written");
    let file = File::open(&path).expect("fixture should be opened");
    let error = query_native_with(&file, Required, |file| {
        let mut stat = rustix::fs::fstat(file).map_err(io::Error::from)?;
        stat.st_mtime_nsec = 1_000_000_000;
        Ok(stat)
    })
    .expect_err("invalid native nanoseconds should fail a required observation");

    assert_eq!(error.kind(), LocalFileErrorKind::RequirementNotMet);
}
