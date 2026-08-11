// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

//! Typed failures from unified copy operations.

use std::error::Error;
use std::fmt::Display;
use std::fmt::Formatter;
use std::fmt::Result as FmtResult;
use std::io;
use std::path::Path;
use std::path::PathBuf;

use super::LocalCopyFailureState;
use super::internal::LocalCopyFailureDetails;
use crate::LocalCopyDirError;
use crate::LocalCopyDirStage;
use crate::LocalCopyStats;
use crate::LocalFileError;
use crate::LocalFileOperation;

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
        )?;
        if let (
            Some(request_source),
            Some(request_target),
            Some(failed_source),
            Some(failed_target),
        ) = (
            self.details.request_source_path.as_deref(),
            self.details.request_target_path.as_deref(),
            self.details.failed_source_path.as_deref(),
            self.details.failed_target_path.as_deref(),
        ) && (request_source != failed_source
            || request_target != failed_target)
        {
            write!(
                formatter,
                " while processing {} -> {}",
                failed_source.display(),
                failed_target.display(),
            )?;
        }
        Ok(())
    }
}

impl Error for LocalCopyFailure {
    /// Returns the primary typed filesystem error.
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        Some(&self.details.error)
    }
}

impl LocalCopyFailure {
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
                request_source_path: error.path().map(Path::to_path_buf),
                request_target_path: error.target().map(Path::to_path_buf),
                failed_source_path: error.path().map(Path::to_path_buf),
                failed_target_path: error.target().map(Path::to_path_buf),
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
            failed_source,
            failed_target,
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
        let mut failure =
            Self::new(error, state, partial_stats, staging_path, cleanup_error);
        failure.details.failed_source_path = Some(failed_source);
        failure.details.failed_target_path = Some(failed_target);
        failure
    }

    /// Returns the primary typed filesystem error.
    #[must_use]
    pub fn error(&self) -> &LocalFileError {
        &self.details.error
    }

    /// Returns the source path supplied for the copy request.
    #[must_use]
    pub fn request_source_path(&self) -> Option<&Path> {
        self.details.request_source_path.as_deref()
    }

    /// Returns the destination path supplied for the copy request.
    #[must_use]
    pub fn request_target_path(&self) -> Option<&Path> {
        self.details.request_target_path.as_deref()
    }

    /// Returns the source entry being processed when the copy failed.
    #[must_use]
    pub fn failed_source_path(&self) -> Option<&Path> {
        self.details.failed_source_path.as_deref()
    }

    /// Returns the destination entry being processed when the copy failed.
    #[must_use]
    pub fn failed_target_path(&self) -> Option<&Path> {
        self.details.failed_target_path.as_deref()
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
