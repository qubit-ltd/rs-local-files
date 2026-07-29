// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Recursive directory copy errors.

use std::error::Error;
use std::fmt::{
    Display,
    Formatter,
    Result as FmtResult,
};
use std::io;
use std::path::{
    Path,
    PathBuf,
};

use crate::{
    LocalCopyDirStage,
    LocalCopyDirStats,
};

/// Error returned by a recursive directory copy operation.
///
/// Diagnostic state is exposed through read-only accessors so callers cannot
/// mutate the stage, paths, statistics, or retained errors independently.
///
/// ```compile_fail
/// use qubit_local_files::copy::Error;
///
/// fn overwrite_stage(error: CopyError) {
///     let _ = error.stage;
/// }
/// ```
#[non_exhaustive]
#[derive(Debug)]
pub struct LocalCopyDirError {
    /// Stage at which the copy failed.
    stage: LocalCopyDirStage,
    /// Source entry being processed when the failure occurred.
    source_path: PathBuf,
    /// Destination entry being processed when the failure occurred.
    destination_path: PathBuf,
    /// Statistics accumulated before the failure.
    stats: Box<LocalCopyDirStats>,
    /// Same-directory staging path when file staging had already started.
    ///
    /// The path may no longer exist when cleanup succeeded or a later retry
    /// removed it.
    temporary_path: Option<Box<Path>>,
    /// Secondary error reported while removing an uncommitted staging file.
    ///
    /// The primary operation error remains available through [`Self::error`]
    /// and the [`Error`] implementation.
    cleanup_error: Option<io::Error>,
    /// Native I/O error that caused the failure.
    error: io::Error,
}

impl LocalCopyDirError {
    /// Creates a recursive-copy error.
    ///
    /// # Parameters
    /// - `stage`: Stage at which the copy failed.
    /// - `source_path`: Source entry being processed.
    /// - `destination_path`: Destination entry being processed.
    /// - `stats`: Statistics accumulated before the failure.
    /// - `error`: Native I/O error that caused the failure.
    ///
    /// # Returns
    /// New recursive-copy error retaining the native source error.
    #[inline]
    pub(crate) fn new(
        stage: LocalCopyDirStage,
        source_path: PathBuf,
        destination_path: PathBuf,
        stats: LocalCopyDirStats,
        error: io::Error,
    ) -> Self {
        Self {
            stage,
            source_path,
            destination_path,
            stats: Box::new(stats),
            temporary_path: None,
            cleanup_error: None,
            error,
        }
    }

    /// Returns the stage at which the copy failed.
    ///
    /// # Returns
    /// Failed recursive-copy stage.
    pub const fn stage(&self) -> LocalCopyDirStage {
        self.stage
    }

    /// Returns the source entry associated with the failure.
    ///
    /// # Returns
    /// Source path being processed.
    #[must_use]
    pub fn source_path(&self) -> &Path {
        &self.source_path
    }

    /// Returns the destination entry associated with the failure.
    ///
    /// # Returns
    /// Destination path being processed.
    #[must_use]
    pub fn destination_path(&self) -> &Path {
        &self.destination_path
    }

    /// Returns statistics accumulated before the failure.
    ///
    /// # Returns
    /// Partial recursive-copy statistics.
    #[must_use]
    pub const fn stats(&self) -> &LocalCopyDirStats {
        &self.stats
    }

    /// Returns the same-directory staging path, when one was created.
    ///
    /// # Returns
    /// Staging path retained for diagnostics.
    pub fn temporary_path(&self) -> Option<&Path> {
        self.temporary_path.as_deref()
    }

    /// Returns the secondary staging cleanup error, when cleanup failed.
    ///
    /// # Returns
    /// Cleanup error without replacing the primary source error.
    pub fn cleanup_error(&self) -> Option<&io::Error> {
        self.cleanup_error.as_ref()
    }

    /// Returns the native I/O error that caused the copy to fail.
    ///
    /// # Returns
    /// Retained primary I/O error.
    #[must_use]
    pub const fn error(&self) -> &io::Error {
        &self.error
    }

    /// Returns the native I/O error kind.
    ///
    /// # Returns
    /// Error kind reported by the retained source error.
    #[must_use]
    pub fn kind(&self) -> io::ErrorKind {
        self.error.kind()
    }

    #[inline(never)]
    pub(crate) fn into_parts(
        self,
    ) -> (
        LocalCopyDirStage,
        PathBuf,
        PathBuf,
        LocalCopyDirStats,
        Option<Box<Path>>,
        Option<io::Error>,
        io::Error,
    ) {
        (
            self.stage,
            self.source_path,
            self.destination_path,
            *self.stats,
            self.temporary_path,
            self.cleanup_error,
            self.error,
        )
    }

    /// Attaches staging-path and cleanup-failure context.
    ///
    /// # Parameters
    /// - `temporary_path`: Same-directory staging path used by the failed copy.
    /// - `cleanup_error`: Secondary error raised while removing that path.
    ///
    /// # Returns
    /// This copy error enriched with staging cleanup context.
    #[inline]
    pub(crate) fn with_staging_context(
        mut self,
        temporary_path: PathBuf,
        cleanup_error: Option<io::Error>,
    ) -> Self {
        self.temporary_path = Some(temporary_path.into_boxed_path());
        self.cleanup_error = cleanup_error;
        self
    }
}

impl Display for LocalCopyDirError {
    /// Formats the copy stage, paths, partial statistics, and source error.
    fn fmt(&self, formatter: &mut Formatter<'_>) -> FmtResult {
        let source = self.source().expect("copy errors always retain a source");
        write!(
            formatter,
            "failed to copy '{}' to '{}' during {:?} after {:?}: {}",
            self.source_path.display(),
            self.destination_path.display(),
            self.stage,
            self.stats,
            source,
        )?;
        if let Some(temporary_path) = self.temporary_path.as_ref() {
            write!(formatter, "; staging path '{}'", temporary_path.display())?;
        }
        if let Some(cleanup_error) = self.cleanup_error.as_ref() {
            return write!(
                formatter,
                "; staging cleanup also failed: {cleanup_error}"
            );
        }
        Ok(())
    }
}

impl Error for LocalCopyDirError {
    /// Returns the retained native I/O error.
    #[inline(always)]
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        Some(&self.error)
    }
}
