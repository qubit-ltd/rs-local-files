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
use super::RootedLocalFileSystem;
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
        let root = self.root.try_clone_authority().map_err(|error| {
            LocalFileError::from_io(
                LocalFileOperation::OpenWriter,
                Some(namespace_absolute.to_path_buf()),
                None,
                error,
            )
        })?;
        ConditionalWriterBackend::rooted(root, relative, namespace_absolute, diagnostic, options).map_err(|error| {
            LocalFileError::from_io(
                LocalFileOperation::OpenWriter,
                Some(namespace_absolute.to_path_buf()),
                None,
                error,
            )
        })
    }
}
