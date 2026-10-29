// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::fs;

use crate::LocalFileErrorKind;
use crate::LocalFileOperation;
use crate::LocalSymlinkPolicy;
use crate::local::HostLocalFileSystem;
use crate::options::LocalTempDirectoryOptions;
use crate::options::LocalTempFileOptions;
use crate::policy::LocalPrivacyRequirement;

#[test]
fn test_host_temp_backend_rejects_invalid_affixes_before_creating_sandboxes() {
    let parent = tempfile::tempdir().expect("parent fixture should be created");
    let parent_path = fs::canonicalize(parent.path()).expect("parent fixture should resolve");
    for affix in ["invalid/name", "invalid\0name"] {
        let file_error = HostLocalFileSystem::create_temp_file_with_policy(
            &LocalTempFileOptions::new().with_parent(&parent_path).with_prefix(affix),
            LocalSymlinkPolicy::FollowWithinScope,
        )
        .expect_err("backend should validate file affixes independently");
        assert_eq!(file_error.kind(), LocalFileErrorKind::InvalidOptions);
        assert_eq!(file_error.operation(), LocalFileOperation::CreateTempFile);
        assert_eq!(file_error.path(), Some(parent_path.as_path()));
        let directory_error = HostLocalFileSystem::create_temp_directory_with_policy(
            &LocalTempDirectoryOptions::new()
                .with_parent(&parent_path)
                .with_suffix(affix),
            LocalSymlinkPolicy::FollowWithinScope,
        )
        .expect_err("backend should validate directory affixes independently");
        assert_eq!(directory_error.kind(), LocalFileErrorKind::InvalidOptions);
        assert_eq!(directory_error.operation(), LocalFileOperation::CreateTempDirectory);
        assert_eq!(directory_error.path(), Some(parent_path.as_path()));
    }
    assert_eq!(
        fs::read_dir(parent.path())
            .expect("parent should remain readable")
            .count(),
        0
    );
}

#[test]
#[cfg(any(unix, windows))]
fn test_host_temp_backend_rejects_zero_attempts_for_each_privacy_mode() {
    let parent = tempfile::tempdir().expect("parent fixture should be created");
    for privacy in [
        LocalPrivacyRequirement::PlatformDefault,
        LocalPrivacyRequirement::Required,
    ] {
        let file_error = HostLocalFileSystem::create_temp_file_with_policy(
            &LocalTempFileOptions::new()
                .with_parent(parent.path())
                .with_max_attempts(0)
                .with_privacy(privacy),
            LocalSymlinkPolicy::FollowWithinScope,
        )
        .expect_err("backend should reject a zero file creation budget");
        assert_eq!(file_error.kind(), LocalFileErrorKind::InvalidOptions);
        assert_eq!(file_error.operation(), LocalFileOperation::CreateTempFile);
        assert_eq!(file_error.io_error_kind(), std::io::ErrorKind::InvalidInput);
        let directory_error = HostLocalFileSystem::create_temp_directory_with_policy(
            &LocalTempDirectoryOptions::new()
                .with_parent(parent.path())
                .with_max_attempts(0)
                .with_privacy(privacy),
            LocalSymlinkPolicy::FollowWithinScope,
        )
        .expect_err("backend should reject a zero directory creation budget");
        assert_eq!(directory_error.kind(), LocalFileErrorKind::InvalidOptions);
        assert_eq!(directory_error.operation(), LocalFileOperation::CreateTempDirectory);
        assert_eq!(directory_error.io_error_kind(), std::io::ErrorKind::InvalidInput);
        assert_eq!(
            fs::read_dir(parent.path())
                .expect("parent should remain readable")
                .count(),
            0
        );
    }
}
