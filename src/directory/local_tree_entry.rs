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
///
/// The path is stored relative to the stage root and must be a nonempty,
/// normalized relative path. Entries describe one expected kind; they do not
/// make verification atomic against concurrent namespace changes.
///
/// # Examples
///
/// ```
/// use std::path::PathBuf;
/// use qubit_local_files::LocalFileKind;
/// use qubit_local_files::directory::LocalTreeEntry;
///
/// # fn main() -> Result<(), qubit_local_files::LocalFileError> {
/// let entry = LocalTreeEntry::new(PathBuf::from("assets/logo.bin"), LocalFileKind::File)?;
/// assert_eq!(entry.relative_path(), std::path::Path::new("assets/logo.bin"));
/// # Ok(())
/// # }
/// ```
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LocalTreeEntry {
    /// Validated path beneath the staged directory root.
    relative_path: PathBuf,
    /// Expected native kind for this path.
    kind: LocalFileKind,
}

impl LocalTreeEntry {
    /// Creates an expected entry from a safe relative path and ordinary tree
    /// kind.
    ///
    /// Returns `InvalidPath` for empty, absolute, dotted, parent, or prefixed
    /// paths and `InvalidOptions` for kinds other than file, directory, and
    /// symbolic link.
    ///
    /// # Errors
    ///
    /// Returns `InvalidPath` when `relative_path` is empty, absolute, or has
    /// dotted, parent, or platform-prefix components. Returns `InvalidOptions`
    /// when `kind` is not a file, directory, or symbolic link.
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
