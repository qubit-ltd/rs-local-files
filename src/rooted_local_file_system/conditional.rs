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
