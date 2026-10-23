// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::path::Path;

use crate::local::conditional::ConditionalWriterBackend;
use crate::options::LocalWriteOptions;

use super::{LocalFileError, LocalFileOperation, LocalResult, RootedLocalFileSystem};

impl RootedLocalFileSystem {
    pub(crate) fn open_conditional_backend(
        &self,
        relative: &Path,
        diagnostic: &Path,
        options: &LocalWriteOptions,
    ) -> LocalResult<ConditionalWriterBackend> {
        let root = self.root.try_clone_authority().map_err(|error| {
            LocalFileError::from_io(
                LocalFileOperation::OpenWriter,
                Some(diagnostic.to_path_buf()),
                None,
                error,
            )
        })?;
        ConditionalWriterBackend::rooted(root, relative, diagnostic, options).map_err(|error| {
            LocalFileError::from_io(
                LocalFileOperation::OpenWriter,
                Some(diagnostic.to_path_buf()),
                None,
                error,
            )
        })
    }
}
