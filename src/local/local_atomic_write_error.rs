// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Atomic-write errors.

use std::error::Error;
use std::fmt::{
    Display,
    Formatter,
    Result as FmtResult,
};
use std::io;
use std::path::PathBuf;

use crate::LocalAtomicWriteStage;

/// Error returned by an atomic whole-file replacement.
#[derive(Debug)]
pub struct LocalAtomicWriteError {
    /// Stage at which the operation failed.
    pub stage: LocalAtomicWriteStage,
    /// Requested destination path.
    pub path: PathBuf,
    /// Same-directory temporary path when one had already been created.
    pub temporary_path: Option<PathBuf>,
    /// Whether destination replacement completed before the failure.
    pub committed: bool,
    /// Native I/O error that caused the failure.
    pub source: io::Error,
}

impl LocalAtomicWriteError {
    /// Creates an atomic-write error.
    ///
    /// # Parameters
    /// - `stage`: Stage at which the operation failed.
    /// - `path`: Requested destination path.
    /// - `temporary_path`: Optional same-directory temporary path.
    /// - `committed`: Whether destination replacement already completed.
    /// - `source`: Native I/O error that caused the failure.
    ///
    /// # Returns
    /// New atomic-write error retaining the native source error.
    pub(crate) fn new(
        stage: LocalAtomicWriteStage,
        path: PathBuf,
        temporary_path: Option<PathBuf>,
        committed: bool,
        source: io::Error,
    ) -> Self {
        Self {
            stage,
            path,
            temporary_path,
            committed,
            source,
        }
    }

    /// Returns the native I/O error kind.
    ///
    /// # Returns
    /// Error kind reported by the retained source error.
    #[inline]
    pub fn kind(&self) -> io::ErrorKind {
        self.source.kind()
    }
}

impl Display for LocalAtomicWriteError {
    /// Formats the failed stage, destination, commit state, and source error.
    fn fmt(&self, formatter: &mut Formatter<'_>) -> FmtResult {
        write!(
            formatter,
            "atomic write to '{}' failed during {:?} (committed={}): {}",
            self.path.display(),
            self.stage,
            self.committed,
            self.source
        )
    }
}

impl Error for LocalAtomicWriteError {
    /// Returns the retained native I/O error.
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        Some(&self.source)
    }
}
