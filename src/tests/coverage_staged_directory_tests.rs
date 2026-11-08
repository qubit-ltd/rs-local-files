// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

#![cfg(unix)]

use std::path::Path;

use crate::LocalFileErrorKind;
use crate::LocalFileSystem;
use crate::LocalListOptions;
use crate::options::LocalDirectoryOpenOptions;
use crate::options::LocalTempDirectoryOptions;
use crate::policy::LocalOwnerAccessPolicy;

#[test]
#[cfg(unix)]
fn test_staged_owner_policy_limits_fail_before_mutation_and_keep_the_stage() {
    let fixture = tempfile::tempdir().expect("fixture should be created");
    let filesystem = LocalFileSystem::host().expect("host filesystem should open");
    let parent = filesystem
        .open_directory_with_options(fixture.path(), &LocalDirectoryOpenOptions::new())
        .expect("parent should open");
    let mut stage = parent
        .create_staged_directory(&LocalTempDirectoryOptions::new())
        .expect("stage should be created");
    stage
        .write_file_new(Path::new("one"), &mut &b"1"[..], 1)
        .expect("first file should be written");
    stage
        .write_file_new(Path::new("two"), &mut &b"2"[..], 1)
        .expect("second file should be written");

    let policy = LocalOwnerAccessPolicy::new().with_unix_modes(0o700, 0o600);
    let error = stage
        .apply_owner_access_policy_with_options(&policy, &LocalListOptions::new().with_max_entries(1))
        .expect_err("entry ceiling should reject the larger tree");
    assert_eq!(error.cause().kind(), LocalFileErrorKind::ResourceLimit);
    stage.cleanup().expect("stage should clean up");
}
