// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::path::Path;

use super::LocalFileError;
use super::LocalFileOperation;
use super::LocalResult;
use super::LocalSymlinkPolicy;
use super::RootedLocalFileSystem;
use super::resolve_rooted_path;
use crate::local::conditional::ConditionalWriterBackend;
use crate::options::LocalWriteOptions;

impl RootedLocalFileSystem {
    /// Opens a conditional-write backend relative to this rooted authority.
    ///
    /// The target must remain beneath the opened root, and symbolic-link
    /// traversal is rejected. The backend receives the namespace path for
    /// error reporting and the optional diagnostic path unchanged.
    ///
    /// # Parameters
    ///
    /// - `relative`: Target path relative to the opened root.
    /// - `namespace_absolute`: Absolute namespace path used in operation
    ///   errors.
    /// - `diagnostic`: Optional path supplied for backend diagnostics.
    /// - `options`: Conditional-write policy and resource options.
    ///
    /// # Returns
    ///
    /// A backend bound to a clone of this rooted authority.
    ///
    /// # Errors
    ///
    /// Returns `LocalFileError` if path resolution, authority cloning, or
    /// backend opening fails. Errors identify the `OpenWriter` operation and
    /// the namespace path.
    pub(crate) fn open_conditional_backend(
        &self,
        relative: &Path,
        namespace_absolute: &Path,
        diagnostic: Option<&Path>,
        options: &LocalWriteOptions,
    ) -> LocalResult<ConditionalWriterBackend> {
        let relative = resolve_rooted_path(
            &self.root,
            relative,
            LocalSymlinkPolicy::Reject,
            false,
            LocalFileOperation::OpenWriter,
        )?;
        let root = self.root.try_clone_authority().map_err(|error| {
            LocalFileError::from_io(
                LocalFileOperation::OpenWriter,
                Some(namespace_absolute.to_path_buf()),
                None,
                error,
            )
        })?;
        ConditionalWriterBackend::rooted(root, relative.as_path(), namespace_absolute, diagnostic, options).map_err(
            |error| {
                LocalFileError::from_io(
                    LocalFileOperation::OpenWriter,
                    Some(namespace_absolute.to_path_buf()),
                    None,
                    error,
                )
            },
        )
    }
}
