// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Rooted metadata, limits, and space observations.

use std::path::Path;

use crate::{
    LocalFileMetadata, LocalFileOperation, LocalFileSystemLimits, LocalFileSystemSpace,
    LocalResult, LocalSymlinkPolicy,
};

use super::{
    RootedLocalFileSystem, probe_rooted_file, resolve_rooted_path, rooted_io_error, rooted_metadata,
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
                || LocalFileSystemLimits::new(crate::SizeLimit::Unknown, crate::SizeLimit::Unknown),
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
            return self
                .root
                .metadata()
                .map(rooted_metadata)
                .map_err(|error| rooted_io_error(LocalFileOperation::Metadata, path, error));
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
            .map_err(|error| rooted_io_error(LocalFileOperation::Metadata, path, error))
    }
}
