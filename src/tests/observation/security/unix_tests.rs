// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::fs;
use std::fs::File;
use std::os::unix::fs::MetadataExt;

#[cfg(target_os = "macos")]
use crate::error::LocalFileErrorKind;
use crate::observation::LocalFileIdentity;
use crate::observation::LocalObservationRequirement;
use crate::observation::LocalSecurityObservationOptions;
use crate::observation::security::query_security;

#[test]
fn test_observes_retained_file_after_path_replacement() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let path = directory.path().join("object");
    fs::write(&path, b"original").expect("create original");
    let retained = File::open(&path).expect("open original");
    let expected = retained.metadata().expect("metadata").ino();
    fs::remove_file(&path).expect("remove name");
    fs::write(&path, b"replacement").expect("create replacement");

    #[cfg(target_os = "linux")]
    let options = LocalSecurityObservationOptions::new().with_access(LocalObservationRequirement::Required);
    #[cfg(not(target_os = "linux"))]
    let options = LocalSecurityObservationOptions::new();
    let observation = query_security(&retained, &options).expect("observe retained object");
    assert_eq!(
        observation.identity().expect("identity").clone(),
        LocalFileIdentity::unix(retained.metadata().unwrap().dev(), expected)
    );
    assert_eq!(observation.link_count(), Some(0));
    #[cfg(target_os = "macos")]
    assert_eq!(
        query_security(
            &retained,
            &LocalSecurityObservationOptions::new().with_access(LocalObservationRequirement::Required),
        )
        .expect_err("macOS cannot provide the required complete Unix ACL observation")
        .kind(),
        LocalFileErrorKind::RequirementNotMet,
    );
}

#[test]
fn test_reports_same_handle_link_count() {
    let directory = tempfile::tempdir().expect("temporary directory");
    let path = directory.path().join("object");
    let alias = directory.path().join("alias");
    fs::write(&path, b"same inode").expect("create file");
    let retained = File::open(&path).expect("open file");
    let options = LocalSecurityObservationOptions::new().with_link_count(LocalObservationRequirement::Required);
    assert_eq!(query_security(&retained, &options).unwrap().link_count(), Some(1));
    fs::hard_link(&path, alias).expect("create hard link");
    assert_eq!(query_security(&retained, &options).unwrap().link_count(), Some(2));
}
