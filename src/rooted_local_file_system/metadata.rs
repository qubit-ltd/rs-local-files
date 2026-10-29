// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Rooted metadata, limits, and space observations.

use std::path::Path;

use super::RootedLocalFileSystem;
use super::resolve_rooted_path;
use super::rooted_io_error;
use super::rooted_metadata;
use crate::LocalFileMetadata;
use crate::LocalFileOperation;
use crate::LocalResult;
use crate::LocalSymlinkPolicy;

impl RootedLocalFileSystem {
    pub(crate) fn canonicalize_virtual(
        &self,
        path: &Path,
        symlink_policy: LocalSymlinkPolicy,
    ) -> LocalResult<std::path::PathBuf> {
        if path.as_os_str().is_empty() {
            let _ = self
                .root
                .metadata()
                .map_err(|error| rooted_io_error(LocalFileOperation::Metadata, path, error))?;
            return Ok(Path::new(std::path::MAIN_SEPARATOR_STR).to_path_buf());
        }
        let relative = super::resolve_rooted_path_allow_root(
            &self.root,
            path,
            symlink_policy,
            true,
            LocalFileOperation::Metadata,
        )?;
        if relative.as_os_str().is_empty() {
            let _metadata = self
                .root
                .metadata()
                .map_err(|error| rooted_io_error(LocalFileOperation::Metadata, path, error))?;
        } else {
            let relative_path = crate::local::LocalRelativePath::new(&relative)
                .map_err(|error| rooted_io_error(LocalFileOperation::Metadata, path, error))?;
            let _metadata = self
                .root
                .symlink_metadata(&relative_path)
                .map_err(|error| rooted_io_error(LocalFileOperation::Metadata, path, error))?;
        }
        let mut absolute = Path::new(std::path::MAIN_SEPARATOR_STR).to_path_buf();
        absolute.push(relative);
        Ok(absolute)
    }

    /// Reads metadata for the final target after applying the selected link
    /// policy.
    pub(crate) fn target_metadata(
        &self,
        path: &Path,
        symlink_policy: LocalSymlinkPolicy,
    ) -> LocalResult<LocalFileMetadata> {
        if path.as_os_str().is_empty() {
            return self.metadata(path, symlink_policy);
        }
        let relative = super::resolve_rooted_path_allow_root(
            &self.root,
            path,
            symlink_policy,
            true,
            LocalFileOperation::Metadata,
        )?;
        if relative.as_os_str().is_empty() {
            return self
                .root
                .metadata()
                .map(rooted_metadata)
                .map_err(|error| rooted_io_error(LocalFileOperation::Metadata, path, error));
        }
        let relative_path = crate::local::LocalRelativePath::new(&relative)
            .map_err(|error| rooted_io_error(LocalFileOperation::Metadata, path, error))?;
        self.root
            .symlink_metadata(&relative_path)
            .map(rooted_metadata)
            .map_err(|error| rooted_io_error(LocalFileOperation::Metadata, path, error))
    }

    /// Reads metadata through the opened root authority.
    /// An empty `path` inspects the retained root. Otherwise applies
    /// `symlink_policy` to intermediate components and inspects the final entry
    /// without following its link. Returns path/policy or native inspection
    /// failures with metadata-operation context.
    ///
    /// # Parameters
    ///
    /// - `path`: Relative descendant path, or an empty path for the opened
    ///   root.
    /// - `symlink_policy`: Policy applied while resolving intermediate path
    ///   components.
    ///
    /// # Returns
    ///
    /// Normalized metadata for the opened root or final entry.
    ///
    /// # Errors
    ///
    /// Returns `LocalFileError` when the path is invalid, violates the
    /// symlink policy, or native metadata inspection fails.
    pub fn metadata(&self, path: &Path, symlink_policy: LocalSymlinkPolicy) -> LocalResult<LocalFileMetadata> {
        if path.as_os_str().is_empty() {
            return self
                .root
                .metadata()
                .map(rooted_metadata)
                .map_err(|error| rooted_io_error(LocalFileOperation::Metadata, path, error));
        }
        let relative = resolve_rooted_path(&self.root, path, symlink_policy, false, LocalFileOperation::Metadata)?;
        self.root
            .symlink_metadata(&relative)
            .map(rooted_metadata)
            .map_err(|error| rooted_io_error(LocalFileOperation::Metadata, path, error))
    }
}

#[cfg(test)]
mod native_metadata_failure_tests {
    use std::fs;
    use std::path::Path;

    use super::RootedLocalFileSystem;
    use crate::LocalFileOperation;
    use crate::LocalSymlinkPolicy;
    use crate::tests::native_io_fault_tests::NativeIoFaultGuard;
    use crate::tests::native_io_fault_tests::NativeIoPoint;

    #[test]
    fn test_root_metadata_native_failures_keep_canonicalization_and_metadata_context() {
        let fixture = tempfile::tempdir().expect("root fixture should exist");
        let filesystem = RootedLocalFileSystem::open(fixture.path()).expect("rooted filesystem should open");
        for (path, target, canonicalize) in [
            (Path::new(""), false, true),
            (Path::new("."), false, true),
            (Path::new("."), true, false),
            (Path::new(""), false, false),
        ] {
            let guard = NativeIoFaultGuard::fail_on(NativeIoPoint::RootMetadata, 1);
            let error = if canonicalize {
                filesystem
                    .canonicalize_virtual(path, LocalSymlinkPolicy::FollowWithinScope)
                    .expect_err("root metadata failure must stop canonicalization")
            } else if target {
                filesystem
                    .target_metadata(path, LocalSymlinkPolicy::FollowWithinScope)
                    .expect_err("root metadata failure must stop target inspection")
            } else {
                filesystem
                    .metadata(path, LocalSymlinkPolicy::FollowWithinScope)
                    .expect_err("root metadata failure must stop inspection")
            };
            assert!(guard.fired());
            assert_eq!(error.operation(), LocalFileOperation::Metadata);
            assert_eq!(error.path(), Some(path));
            assert_eq!(error.io_error_kind(), std::io::ErrorKind::Other);
            assert_eq!(
                error
                    .io_error()
                    .expect("native source must survive context mapping")
                    .to_string(),
                "injected native I/O contract failure"
            );
        }
    }

    #[test]
    fn test_post_resolution_entry_metadata_failure_stops_canonicalization() {
        let fixture = tempfile::tempdir().expect("root fixture should exist");
        fs::write(fixture.path().join("payload"), b"data").expect("file should exist");
        let filesystem = RootedLocalFileSystem::open(fixture.path()).expect("rooted filesystem should open");
        let path = Path::new("payload");
        let guard = NativeIoFaultGuard::fail_on(NativeIoPoint::EntryMetadata, 1);

        let error = filesystem
            .canonicalize_virtual(path, LocalSymlinkPolicy::FollowWithinScope)
            .expect_err("successful path resolution must not mask the final metadata failure");

        assert!(guard.fired());
        assert_eq!(error.operation(), LocalFileOperation::Metadata);
        assert_eq!(error.path(), Some(path));
        assert_eq!(error.io_error_kind(), std::io::ErrorKind::Other);
    }
}
