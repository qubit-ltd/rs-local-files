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

use super::RootedLocalFileSystem;
use super::probe_rooted_file;
use super::resolve_rooted_path;
use super::rooted_io_error;
use super::rooted_metadata;
use crate::LocalFileMetadata;
use crate::LocalFileOperation;
use crate::LocalFileSystemLimits;
use crate::LocalFileSystemSpace;
use crate::LocalResult;
use crate::LocalSymlinkPolicy;

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
