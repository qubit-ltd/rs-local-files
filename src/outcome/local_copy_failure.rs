// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

//! Typed failures from unified copy operations.

use std::{
    error::Error,
    fmt::{
        Display,
        Formatter,
        Result as FmtResult,
    },
    io,
    path::{
        Path,
        PathBuf,
    },
};

use crate::{
    LocalCopyDirError,
    LocalCopyDirStage,
    LocalCopyStats,
    LocalFileError,
    LocalFileOperation,
};

use super::LocalCopyFailureState;
use super::internal::LocalCopyFailureDetails;

/// Failure details retained when a unified copy does not complete.
#[derive(Debug)]
pub struct LocalCopyFailure {
    /// Heap-owned failure details kept off the result's hot path.
    details: Box<LocalCopyFailureDetails>,
}

impl Display for LocalCopyFailure {
    /// Formats the primary copy failure and its proven destination state.
    #[inline(always)]
    fn fmt(&self, formatter: &mut Formatter<'_>) -> FmtResult {
        write!(
            formatter,
            "copy failed with {:?} state: {}",
            self.details.state, self.details.error
        )
    }
}

impl Error for LocalCopyFailure {
    /// Returns the primary typed filesystem error.
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        Some(&self.details.error)
    }
}

impl LocalCopyFailure {
    /// Coverage-only conversion entry point for the native copy error value.
    #[cfg(coverage)]
    pub fn coverage_from_copy_dir_error(
        source: &Path,
        target: &Path,
        error: LocalCopyDirError,
    ) -> Self {
        Self::from_copy_dir_error(source, target, error)
    }

    /// Creates a typed copy failure from implementation facts.
    #[must_use]
    pub(crate) fn new(
        error: LocalFileError,
        state: LocalCopyFailureState,
        partial_stats: LocalCopyStats,
        staging_path: Option<PathBuf>,
        cleanup_error: Option<LocalFileError>,
    ) -> Self {
        Self {
            details: Box::new(LocalCopyFailureDetails {
                error,
                state,
                partial_stats,
                staging_path,
                cleanup_error,
            }),
        }
    }

    /// Converts a structured native copy-pipeline error without losing facts.
    #[must_use]
    pub(crate) fn from_copy_dir_error(
        source: &Path,
        target: &Path,
        error: LocalCopyDirError,
    ) -> Self {
        let (
            stage,
            _failed_source,
            _failed_target,
            stats,
            staging_path,
            cleanup_error,
            primary,
        ) = error.into_parts();
        let partial_stats = LocalCopyStats::from_internal(stats);
        let state = copy_failure_state(stage, partial_stats);
        let primary_kind = primary.kind();
        let error = LocalFileError::from_io(
            LocalFileOperation::Copy,
            Some(source.to_path_buf()),
            Some(target.to_path_buf()),
            io::Error::new(primary_kind, primary),
        );
        let cleanup_error = cleanup_error.map(|cleanup_error| {
            let cleanup_kind = cleanup_error.kind();
            LocalFileError::from_io(
                LocalFileOperation::Copy,
                Some(source.to_path_buf()),
                Some(target.to_path_buf()),
                io::Error::new(cleanup_kind, cleanup_error),
            )
        });
        let staging_path = cleanup_error
            .as_ref()
            .and(staging_path.as_deref())
            .map(Path::to_path_buf);
        Self::new(error, state, partial_stats, staging_path, cleanup_error)
    }

    /// Returns the primary typed filesystem error.
    #[must_use]
    pub fn error(&self) -> &LocalFileError {
        &self.details.error
    }

    /// Returns the most precise destination state proven by native operations.
    pub fn state(&self) -> LocalCopyFailureState {
        self.details.state
    }

    /// Returns statistics accumulated before the failure.
    #[must_use = "partial statistics retain copy progress"]
    pub fn partial_stats(&self) -> &LocalCopyStats {
        &self.details.partial_stats
    }

    /// Returns the retained staging path when cleanup failed.
    #[must_use]
    pub fn staging_path(&self) -> Option<&Path> {
        self.details.staging_path.as_deref()
    }

    /// Returns the secondary staging-cleanup error when cleanup failed.
    #[must_use]
    pub fn cleanup_error(&self) -> Option<&LocalFileError> {
        self.details.cleanup_error.as_ref()
    }

    /// Consumes this failure and returns every retained part.
    pub fn into_parts(
        self,
    ) -> (
        LocalFileError,
        LocalCopyFailureState,
        LocalCopyStats,
        Option<PathBuf>,
        Option<LocalFileError>,
    ) {
        let details = *self.details;
        (
            details.error,
            details.state,
            details.partial_stats,
            details.staging_path,
            details.cleanup_error,
        )
    }
}

/// Maps structured native copy facts to the strongest proven failure state.
const fn copy_failure_state(
    stage: LocalCopyDirStage,
    partial_stats: LocalCopyStats,
) -> LocalCopyFailureState {
    if partial_stats.files() > 0 || partial_stats.directories() > 0 {
        return LocalCopyFailureState::PartiallyPublished;
    }
    match stage {
        LocalCopyDirStage::InspectSource
        | LocalCopyDirStage::InspectSourceEntry
        | LocalCopyDirStage::ReadSourceDirectory
        | LocalCopyDirStage::SynchronizeFile
        | LocalCopyDirStage::CleanupTemporaryFile => {
            LocalCopyFailureState::Unchanged
        }
        LocalCopyDirStage::PrepareDestination
        | LocalCopyDirStage::CopyFileContents
        | LocalCopyDirStage::PreservePermissions
        | LocalCopyDirStage::CommitFile
        | LocalCopyDirStage::UpdateStatistics => {
            LocalCopyFailureState::Indeterminate
        }
    }
}

/// Result returned by unified copy operations.
pub type LocalCopyResult = Result<super::LocalCopyOutcome, LocalCopyFailure>;
