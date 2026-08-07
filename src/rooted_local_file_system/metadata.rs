// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Rooted metadata, limits, and space observations.
// qubit-style: allow inline-tests
// qubit-style: allow explicit-imports

use std::path::Path;

use crate::{
    LocalFileMetadata,
    LocalFileOperation,
    LocalFileSystemLimits,
    LocalFileSystemSpace,
    LocalResult,
    LocalSymlinkPolicy,
};

use super::{
    RootedLocalFileSystem,
    probe_rooted_file,
    resolve_rooted_path,
    rooted_io_error,
    rooted_metadata,
};

impl RootedLocalFileSystem {
    /// Observes path limits at a rooted path or its nearest existing ancestor.
    pub fn limits_at(
        &self,
        path: &Path,
        symlink_policy: LocalSymlinkPolicy,
    ) -> LocalResult<LocalFileSystemLimits> {
        probe_rooted_file(
            &self.root,
            path,
            symlink_policy,
            LocalFileOperation::Metadata,
        )
        .map(|file| {
            file.map_or_else(
                || {
                    LocalFileSystemLimits::new(
                        crate::SizeLimit::Unknown,
                        crate::SizeLimit::Unknown,
                    )
                },
                |file| crate::capability::probe_limits(&file),
            )
        })
    }

    /// Observes dynamic space at a rooted path or its nearest existing
    /// ancestor.
    pub fn space_at(
        &self,
        path: &Path,
        symlink_policy: LocalSymlinkPolicy,
    ) -> LocalResult<LocalFileSystemSpace> {
        probe_rooted_file(
            &self.root,
            path,
            symlink_policy,
            LocalFileOperation::Metadata,
        )
        .map(|file| {
            file.map_or_else(
                || LocalFileSystemSpace::new(None, None, None),
                |file| crate::capability::probe_space(&file),
            )
        })
    }

    /// Reads metadata through the opened root authority.
    pub fn metadata(
        &self,
        path: &Path,
        symlink_policy: LocalSymlinkPolicy,
    ) -> LocalResult<LocalFileMetadata> {
        if path.as_os_str().is_empty() {
            return self.root.metadata().map(rooted_metadata).map_err(
                |error| {
                    rooted_io_error(LocalFileOperation::Metadata, path, error)
                },
            );
        }
        let relative = resolve_rooted_path(
            &self.root,
            path,
            symlink_policy,
            false,
            LocalFileOperation::Metadata,
        )?;
        self.root
            .symlink_metadata(&relative)
            .map(rooted_metadata)
            .map_err(|error| {
                rooted_io_error(LocalFileOperation::Metadata, path, error)
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn observes_root_limits_space_and_metadata() {
        let filesystem = RootedLocalFileSystem::open(Path::new("."))
            .expect("current directory can be opened");
        let limits = filesystem
            .limits_at(Path::new("Cargo.toml"), LocalSymlinkPolicy::Reject)
            .expect("limits are queryable");
        let _ = (limits.max_file_name_bytes(), limits.max_path_bytes());
        let space = filesystem
            .space_at(Path::new("Cargo.toml"), LocalSymlinkPolicy::Reject)
            .expect("space is queryable");
        let _ = (
            space.available_bytes(),
            space.capacity_bytes(),
            space.free_bytes(),
        );
        assert_eq!(
            filesystem
                .metadata(Path::new(""), LocalSymlinkPolicy::Reject)
                .expect("root metadata")
                .kind(),
            crate::LocalFileKind::Directory
        );
        assert_eq!(
            filesystem
                .metadata(Path::new("Cargo.toml"), LocalSymlinkPolicy::Reject)
                .expect("file metadata")
                .kind(),
            crate::LocalFileKind::File
        );
        let _ = filesystem
            .limits_at(Path::new("missing/entry"), LocalSymlinkPolicy::Reject)
            .expect("nearest existing ancestor provides limits");
        let _ = filesystem
            .space_at(Path::new("missing/entry"), LocalSymlinkPolicy::Reject)
            .expect("nearest existing ancestor provides space");
        assert!(
            filesystem
                .metadata(
                    Path::new("missing/entry"),
                    LocalSymlinkPolicy::Reject
                )
                .is_err()
        );
    }
}
