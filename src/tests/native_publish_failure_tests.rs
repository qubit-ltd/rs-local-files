// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
use std::ffi::OsStr;

use crate::LocalFileErrorKind;
use crate::LocalFileOperation;
use crate::LocalFileSystem;
use crate::options::LocalDirectoryOpenOptions;
use crate::options::LocalTempDirectoryOptions;
use crate::options::LocalTreePublishOptions;
use crate::outcome::LocalTreeAccessState;
use crate::outcome::LocalTreeMutationState;
use crate::outcome::LocalTreePublication;
use crate::policy::LocalTreeFinalAccess;
use crate::tests::native_io_fault_tests::NativeIoFaultGuard;
use crate::tests::native_io_fault_tests::NativeIoPoint;

#[test]
fn test_publish_readonly_root_clone_failure_retains_access_changed_stage() {
    let fixture = tempfile::tempdir().expect("fixture");
    let parent = LocalFileSystem::host()
        .expect("host")
        .open_directory_with_options(fixture.path(), &LocalDirectoryOpenOptions::new())
        .expect("parent");
    let stage = parent
        .create_staged_directory(&LocalTempDirectoryOptions::new())
        .expect("stage");
    let path = stage.directory().diagnostic_path().to_path_buf();
    // Two parent observations and two root access handles precede the
    // publication-only writable root acquisition for this empty tree.
    let guard = NativeIoFaultGuard::fail_on(NativeIoPoint::CloneAuthority, 5);
    let error = stage
        .publish_new(
            &parent,
            OsStr::new("published"),
            &LocalTreePublishOptions::new().with_access(LocalTreeFinalAccess::ReadOnlyCurrentUser),
        )
        .expect_err("publication root authority clone fails after access application");
    assert!(guard.fired());
    assert_eq!(error.cause().kind(), LocalFileErrorKind::Io);
    assert_eq!(error.cause().operation(), LocalFileOperation::Metadata);
    assert_eq!(error.cause().path(), Some(path.as_path()));
    assert_eq!(error.mutation_state(), LocalTreeMutationState::Started);
    assert_eq!(error.publication(), LocalTreePublication::NotPublished);
    assert_eq!(error.access_state(), LocalTreeAccessState::ReadOnlyCurrentUser);
    assert!(!fixture.path().join("published").exists());
    drop(guard);
    let stage = error.into_stage().expect("failed publication retains stage owner");
    stage.cleanup().expect("read-only stage remains recoverable");
    assert!(!path.exists());
}
