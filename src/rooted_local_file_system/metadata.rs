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
