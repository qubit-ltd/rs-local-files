// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Recursive directory copy errors.

use std::error::Error;
use std::fmt::{Display, Formatter, Result as FmtResult};
use std::io;
use std::path::PathBuf;

use crate::{LocalCopyDirStage, LocalCopyDirStats};

/// Error returned by a recursive directory copy operation.
#[derive(Debug)]
pub struct LocalCopyDirError {
    /// Stage at which the copy failed.
    pub stage: LocalCopyDirStage,
    /// Source entry being processed when the failure occurred.
    pub source_path: PathBuf,
    /// Destination entry being processed when the failure occurred.
    pub destination_path: PathBuf,
    /// Statistics accumulated before the failure.
    pub stats: LocalCopyDirStats,
    /// Native I/O error that caused the failure.
    pub source: io::Error,
}

impl LocalCopyDirError {
    /// Creates a recursive-copy error.
    ///
    /// # Parameters
    /// - `stage`: Stage at which the copy failed.
    /// - `source_path`: Source entry being processed.
    /// - `destination_path`: Destination entry being processed.
    /// - `stats`: Statistics accumulated before the failure.
    /// - `source`: Native I/O error that caused the failure.
    ///
    /// # Returns
    /// New recursive-copy error retaining the native source error.
    pub(crate) fn new(
        stage: LocalCopyDirStage,
        source_path: PathBuf,
        destination_path: PathBuf,
        stats: LocalCopyDirStats,
        source: io::Error,
    ) -> Self {
        Self {
            stage,
            source_path,
            destination_path,
            stats,
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

impl Display for LocalCopyDirError {
    /// Formats the copy stage, paths, partial statistics, and source error.
    fn fmt(&self, formatter: &mut Formatter<'_>) -> FmtResult {
        write!(
            formatter,
            "failed to copy '{}' to '{}' during {:?} after {:?}: {}",
            self.source_path.display(),
            self.destination_path.display(),
            self.stage,
            self.stats,
            self.source
        )
    }
}

impl Error for LocalCopyDirError {
    /// Returns the retained native I/O error.
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        Some(&self.source)
    }
}
