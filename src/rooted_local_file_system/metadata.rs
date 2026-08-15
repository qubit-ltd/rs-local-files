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
use super::resolve_rooted_path;
use super::rooted_io_error;
use super::rooted_metadata;
use crate::LocalFileMetadata;
use crate::LocalFileOperation;
use crate::LocalResult;
use crate::LocalSymlinkPolicy;

impl RootedLocalFileSystem {
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
