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
        let mut absolute = Path::new(std::path::MAIN_SEPARATOR_STR).to_path_buf();
        absolute.push(relative);
        Ok(absolute)
    }

    /// Reads metadata for the final target after applying the selected link policy.
    pub(crate) fn target_metadata(
        &self,
        path: &Path,
        symlink_policy: LocalSymlinkPolicy,
    ) -> LocalResult<LocalFileMetadata> {
        if path.as_os_str().is_empty() {
            return self.metadata(path, symlink_policy);
        }
        let relative = resolve_rooted_path(
            &self.root,
            path,
            symlink_policy,
            true,
            LocalFileOperation::Metadata,
        )?;
        self.root
            .symlink_metadata(&relative)
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
