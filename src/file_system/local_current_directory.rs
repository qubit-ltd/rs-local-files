// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Process-backed and virtual current-directory state.

use std::path::Path;
use std::path::PathBuf;

use crate::LocalFileError;
use crate::LocalFileOperation;
use crate::LocalResult;

/// Selects whether a filesystem reads the process PWD or owns a virtual PWD.
#[derive(Clone, Debug)]
pub(crate) enum LocalCurrentDirectory {
    /// Reads the native process PWD when an operation needs a relative anchor.
    Process,
    /// Retains a namespace-absolute virtual PWD for a Rooted filesystem.
    Virtual(PathBuf),
}

impl LocalCurrentDirectory {
    /// Captures the PWD used by one operation.
    ///
    /// `operation` and `path` are attached to failures from querying the
    /// process PWD. A virtual PWD is cloned without native I/O.
    pub(crate) fn snapshot(&self, operation: LocalFileOperation, path: Option<&Path>) -> LocalResult<PathBuf> {
        match self {
            Self::Process => std::env::current_dir().map_err(|source| {
                LocalFileError::from_io(operation, path.map(Path::to_path_buf), None, source)
                    .with_reason("failed to read the process current directory")
            }),
            Self::Virtual(path) => Ok(path.clone()),
        }
    }

    /// Returns the retained virtual PWD, or `None` for process-backed state.
    #[inline]
    pub(crate) fn virtual_path(&self) -> Option<&Path> {
        match self {
            Self::Process => None,
            Self::Virtual(path) => Some(path),
        }
    }

    /// Replaces a retained virtual PWD.
    ///
    /// Returns `false` when called for process-backed state.
    pub(crate) fn replace_virtual(&mut self, path: PathBuf) -> bool {
        match self {
            Self::Process => false,
            Self::Virtual(current) => {
                *current = path;
                true
            }
        }
    }
}
