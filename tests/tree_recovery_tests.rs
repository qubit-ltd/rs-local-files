// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::error::Error;
use std::ffi::OsStr;

use qubit_local_files::LocalFileSystem;
use qubit_local_files::LocalTreePublishError;
use qubit_local_files::options::LocalDirectoryOpenOptions;
use qubit_local_files::options::LocalTempDirectoryOptions;
use qubit_local_files::options::LocalTreePublishOptions;
use qubit_local_files::outcome::LocalTreePublication;

#[test]
fn test_recovery_error_publish_source_retains_same_cause_and_owners() {
    let fixture = tempfile::tempdir().expect("fixture should be created");
    let target = fixture.path().join("target");
    std::fs::create_dir(&target).expect("target should be created");
    std::fs::write(target.join("old"), b"original").expect("target contents should be written");

    let filesystem = LocalFileSystem::host().expect("Host filesystem should open");
    let parent = filesystem
        .open_directory_with_options(fixture.path(), &LocalDirectoryOpenOptions::new())
        .expect("parent directory should open");
    let stage = parent
        .create_staged_directory(&LocalTempDirectoryOptions::new())
        .expect("stage should be created");
    let failure: LocalTreePublishError = stage
        .publish_new(&parent, OsStr::new("target"), &LocalTreePublishOptions::new())
        .expect_err("existing destination should prevent publication");

    assert_eq!(LocalTreePublication::NotPublished, failure.publication());
    let source = Error::source(&failure)
        .and_then(|source| source.downcast_ref::<qubit_local_files::LocalFileError>())
        .expect("standard source should be the structured cause");
    assert!(std::ptr::eq(source, failure.cause()));
    let (_, stage, retained) = failure.into_parts();
    if let Some(stage) = stage {
        stage.cleanup().expect("recovered stage should be explicitly cleanable");
    }
    for resource in retained {
        resource.cleanup().expect("retained resource should be explicitly cleanable");
    }
    assert_eq!(
        b"original",
        std::fs::read(target.join("old"))
            .expect("old target should remain readable")
            .as_slice()
    );
}
