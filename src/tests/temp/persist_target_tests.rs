// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::path::Path;

use crate::LocalSymlinkPolicy;
use crate::error::LocalFileErrorKind;
use crate::path::LocalFileSystemScope;
use crate::temp::internal::HostTempResourceBackend;
use crate::temp::internal::LocalTempResourceBackend;
use crate::temp::internal::TempEntryIdentity;
use crate::temp::internal::prepare_persist_target;
use crate::temp::internal::validate_persist_base;

#[test]
fn test_persist_target_validation_covers_bases_and_namespace_rules() {
    let fixture = tempfile::tempdir().expect("fixture");
    let directory = std::fs::canonicalize(fixture.path()).expect("canonicalize fixture directory");
    let file = directory.join("file");
    std::fs::write(&file, b"base file").expect("file base");

    let backend = LocalTempResourceBackend::Host(HostTempResourceBackend {
        identity: TempEntryIdentity::from_path(&directory).expect("directory identity"),
        sandbox_path: directory.clone(),
    });
    validate_persist_base(&backend, &directory, LocalSymlinkPolicy::Reject).expect("directory base");

    let error = validate_persist_base(&backend, &file, LocalSymlinkPolicy::Reject)
        .expect_err("regular file is not a persistence base");
    assert_eq!(error.kind(), LocalFileErrorKind::NotDirectory);
    let missing = directory.join("missing");
    assert!(validate_persist_base(&backend, &missing, LocalSymlinkPolicy::Reject).is_err());

    let target = directory.join("published");
    assert!(prepare_persist_target(LocalFileSystemScope::Host, None, &target).is_ok());
    assert!(prepare_persist_target(LocalFileSystemScope::Host, Some(&directory), Path::new("child")).is_ok());

    let dotted = fixture.path().join("..");
    let error = prepare_persist_target(LocalFileSystemScope::Host, Some(&dotted), Path::new("child"))
        .expect_err("persistence base cannot contain parent components");
    assert_eq!(error.kind(), LocalFileErrorKind::InvalidPath);
    let error = prepare_persist_target(LocalFileSystemScope::Rooted, None, Path::new("../escape"))
        .expect_err("rooted persistence cannot escape its namespace");
    assert_eq!(error.kind(), LocalFileErrorKind::InvalidPath);
}
