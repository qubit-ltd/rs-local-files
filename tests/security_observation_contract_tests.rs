// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use qubit_local_files::LocalFileSystem;
#[cfg(all(unix, not(target_os = "macos")))]
use qubit_local_files::observation::LocalObservationRequirement;
use qubit_local_files::observation::LocalSecurityObservationOptions;
use qubit_local_files::options::LocalDirectoryOpenOptions;
use qubit_local_files::options::LocalObservedReadOptions;

#[cfg(unix)]
#[test]
fn reader_security_observation_stays_with_open_file_after_rename() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let original = directory.path().join("original");
    let moved = directory.path().join("moved");
    std::fs::write(&original, b"data").expect("create source");
    let filesystem = LocalFileSystem::host().expect("host filesystem");
    let reader = filesystem
        .open_observed_reader(&original, &LocalObservedReadOptions::new())
        .expect("open reader");
    let initial = reader
        .security_observation(&LocalSecurityObservationOptions::new())
        .expect("security observation");
    std::fs::rename(&original, &moved).expect("rename retained object");
    std::fs::write(&original, b"replacement").expect("replace old name");
    let after = reader
        .security_observation(&LocalSecurityObservationOptions::new())
        .expect("observe retained object after replacement");
    assert_eq!(after.identity(), initial.identity());
    assert_eq!(after.link_count(), Some(1));
}

#[cfg(unix)]
#[test]
fn observed_reader_exposes_native_unix_details_from_its_open_handle() {
    use std::os::unix::fs::MetadataExt;

    let directory = tempfile::tempdir().expect("temporary directory");
    let path = directory.path().join("file");
    std::fs::write(&path, b"data").expect("create source");
    let expected = std::fs::metadata(&path).expect("source metadata");
    let reader = LocalFileSystem::host()
        .expect("host filesystem")
        .open_observed_reader(&path, &LocalObservedReadOptions::new())
        .expect("open reader");

    let observation = reader.observe_now().expect("observe open file");
    let details = observation
        .native_details()
        .and_then(|native| native.unix())
        .expect("Unix native details are available");
    assert_eq!(details.device(), expected.dev());
    assert_eq!(details.inode(), expected.ino());
    assert_eq!(details.modified_seconds(), expected.mtime());
    assert_eq!(details.modified_nanos(), expected.mtime_nsec());
    assert_eq!(details.changed_seconds(), expected.ctime());
    assert_eq!(details.changed_nanos(), expected.ctime_nsec());
}

#[cfg(unix)]
#[test]
fn directory_security_observation_stays_with_retained_authority_after_rename() {
    use std::os::unix::fs::MetadataExt;

    let parent = tempfile::tempdir().expect("temporary directory");
    let original = parent.path().join("original");
    let moved = parent.path().join("moved");
    std::fs::create_dir(&original).expect("create directory");
    let filesystem = LocalFileSystem::host().expect("host filesystem");
    let handle = filesystem
        .open_directory_with_options(&original, &LocalDirectoryOpenOptions::new())
        .expect("open directory handle");
    let initial = handle
        .security_observation(&LocalSecurityObservationOptions::new())
        .expect("security observation");
    std::fs::rename(&original, &moved).expect("rename retained directory");
    std::fs::create_dir(&original).expect("replace old name");
    let after = handle
        .security_observation(&LocalSecurityObservationOptions::new())
        .expect("observe retained directory after replacement");
    let expected_links = std::fs::metadata(&moved)
        .expect("retained directory metadata should remain readable")
        .nlink();
    assert_eq!(after.identity(), initial.identity());
    assert_eq!(after.link_count(), initial.link_count());
    assert_eq!(after.link_count(), Some(expected_links));
}

#[cfg(unix)]
#[test]
fn options_default_to_optional_and_allow_zero_acl_limits() {
    let options = LocalSecurityObservationOptions::new()
        .with_max_acl_entries(0)
        .with_max_acl_bytes(0);
    let directory = tempfile::tempdir().expect("temporary directory");
    let path = directory.path().join("file");
    std::fs::write(&path, b"data").expect("create source");
    let filesystem = LocalFileSystem::host().expect("host filesystem");
    let reader = filesystem
        .open_observed_reader(&path, &LocalObservedReadOptions::new())
        .expect("open reader");
    let observation = reader.security_observation(&options).expect("optional observation");
    assert!(observation.identity().is_some());
}

#[cfg(target_os = "linux")]
#[test]
fn required_fields_and_each_individual_requirement_are_reported() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let path = directory.path().join("file");
    std::fs::write(&path, b"data").expect("create source");
    let filesystem = LocalFileSystem::host().expect("host filesystem");
    let reader = filesystem
        .open_observed_reader(&path, &LocalObservedReadOptions::new())
        .expect("open reader");
    let required = LocalObservationRequirement::Required;

    let all = reader
        .security_observation(
            &LocalSecurityObservationOptions::new()
                .with_filesystem(required)
                .with_access(required)
                .with_link_count(required),
        )
        .expect("all Unix security facts are available");
    assert!(all.filesystem().is_some());
    assert!(all.access().and_then(|access| access.unix()).is_some());
    assert!(all.link_count().is_some());

    for options in [
        LocalSecurityObservationOptions::new().with_filesystem(required),
        LocalSecurityObservationOptions::new().with_access(required),
        LocalSecurityObservationOptions::new().with_link_count(required),
    ] {
        reader
            .security_observation(&options)
            .expect("each individually required Unix fact is available");
    }
}

#[cfg(all(unix, not(any(target_os = "linux", target_os = "macos"))))]
#[test]
fn unimplemented_unix_filesystem_observation_obeys_optional_and_required_requests() {
    use qubit_local_files::error::LocalFileErrorKind;
    use qubit_local_files::error::LocalFileOperation;

    let directory = tempfile::tempdir().expect("temporary directory");
    let path = directory.path().join("file");
    std::fs::write(&path, b"data").expect("create source");
    let filesystem = LocalFileSystem::host().expect("host filesystem");
    let reader = filesystem
        .open_observed_reader(&path, &LocalObservedReadOptions::new())
        .expect("open reader");

    let optional = reader
        .security_observation(&LocalSecurityObservationOptions::new())
        .expect("optional facts should succeed");
    assert!(optional.identity().is_some());
    assert!(optional.link_count().is_some());
    assert!(optional.filesystem().is_none());

    let error = reader
        .security_observation(
            &LocalSecurityObservationOptions::new().with_filesystem(LocalObservationRequirement::Required),
        )
        .expect_err("required filesystem facts are unimplemented on this Unix target");
    assert_eq!(error.kind(), LocalFileErrorKind::RequirementNotMet);
    assert_eq!(error.operation(), LocalFileOperation::Metadata);

    let directory_handle = filesystem
        .open_directory_with_options(directory.path(), &LocalDirectoryOpenOptions::new())
        .expect("open directory handle");
    let optional = directory_handle
        .security_observation(&LocalSecurityObservationOptions::new())
        .expect("optional directory facts should succeed");
    assert!(optional.identity().is_some());
    assert!(optional.link_count().is_some());
    assert!(optional.filesystem().is_none());

    let error = directory_handle
        .security_observation(
            &LocalSecurityObservationOptions::new().with_filesystem(LocalObservationRequirement::Required),
        )
        .expect_err("required directory filesystem facts are unimplemented");
    assert_eq!(error.kind(), LocalFileErrorKind::RequirementNotMet);
    assert_eq!(error.operation(), LocalFileOperation::Metadata);
}
