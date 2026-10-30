// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use qubit_local_files::LocalFileSystem;
#[cfg(target_os = "linux")]
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
fn directory_security_observation_stays_with_retained_authority_after_rename() {
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
    assert_eq!(after.identity(), initial.identity());
    assert_eq!(after.link_count(), Some(2));
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
