// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
use std::path::Path;
use std::path::PathBuf;

use crate::LocalFileError;
use crate::LocalFileErrorKind;
use crate::LocalFileKind;
use crate::LocalFileOperation;
use crate::LocalResult;

/// One expected entry in a staged directory tree.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LocalTreeEntry {
    relative_path: PathBuf,
    kind: LocalFileKind,
}

impl LocalTreeEntry {
    /// Creates an expected entry from a safe relative path and an ordinary tree
    /// kind.
    ///
    /// Returns `InvalidPath` for empty, absolute, dotted, parent, or prefixed
    /// paths and `InvalidOptions` for kinds other than file, directory, and
    /// symbolic link.
    pub fn new(relative_path: PathBuf, kind: LocalFileKind) -> LocalResult<Self> {
        let relative_path = crate::LocalRelativePath::new(&relative_path)
            .map_err(|source| {
                LocalFileError::from_io(
                    LocalFileOperation::ValidateName,
                    Some(relative_path.clone()),
                    None,
                    source,
                )
            })?
            .as_path()
            .to_path_buf();
        let valid_kind = matches!(
            kind,
            LocalFileKind::File | LocalFileKind::Directory | LocalFileKind::Symlink
        );
        if !valid_kind {
            return Err(LocalFileError::new(
                LocalFileErrorKind::InvalidOptions,
                LocalFileOperation::ValidateName,
            ));
        }
        Ok(Self { relative_path, kind })
    }

    /// Returns this entry's path relative to the staged directory.
    #[must_use]
    pub fn relative_path(&self) -> &Path {
        &self.relative_path
    }

    /// Returns the expected native entry kind.
    pub const fn kind(&self) -> LocalFileKind {
        self.kind
    }
}
