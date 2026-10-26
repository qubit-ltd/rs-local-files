// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::error::Error;
use std::fmt;

use crate::LocalFileError;
use crate::error::LocalConditionalFailureReason;
use crate::error::LocalConditionalWriteStage;
use crate::outcome::LocalConditionalPublicationState;
use crate::recovery::LocalRecoveryFile;
use crate::recovery::LocalRecoveryFileRole;

/// Failure of a conditional commit with its publication state and recoverable
/// files.
///
/// This error owns every retained recovery file. Inspect
/// [`candidate`](Self::candidate) and read it with
/// [`LocalRecoveryFile::open_reader`] while the error is alive, or consume the
/// error with [`into_recovery_files`](Self::into_recovery_files) to transfer
/// ownership. Dropping those handles does not delete their files.
///
/// The publication state, reason, and stage describe the observed attempt;
/// `Indeterminate` must not be treated as safe to retry blindly.
///
/// # Examples
///
/// ```
/// use qubit_io::Output;
/// use qubit_local_files::LocalFileSystem;
/// use qubit_local_files::error::LocalConditionalFailureReason;
/// use qubit_local_files::options::{LocalWriteMode, LocalWriteOptions, LocalWritePrecondition};
/// use qubit_local_files::outcome::LocalConditionalPublicationState;
/// use qubit_local_files::error::LocalConditionalWriteStage;
/// use sha2::Sha256;
/// use tempfile::tempdir;
///
/// let directory = tempdir()?;
/// let target = directory.path().canonicalize()?.join("model");
/// std::fs::write(&target, b"existing")?;
/// let mut filesystem = LocalFileSystem::host()?;
/// filesystem.set_symlink_policy(qubit_local_files::policy::LocalSymlinkPolicy::Reject)?;
/// let mut writer = filesystem.open_conditional_writer::<Sha256>(
///     &target,
///     LocalWritePrecondition::Absent,
///     &LocalWriteOptions::new(LocalWriteMode::CreateOrReplace),
/// )?;
/// writer.write_fully(b"candidate bytes")?;
/// let error = writer.commit().expect_err("the target already exists");
/// assert_eq!(error.publication(), LocalConditionalPublicationState::NotPublished);
/// assert_eq!(error.reason(), LocalConditionalFailureReason::TargetExists);
/// assert_eq!(error.stage(), LocalConditionalWriteStage::Precheck);
/// let candidate = error.candidate().expect("verified candidate is retained");
/// let mut reader = candidate.open_reader()?;
/// let mut bytes = Vec::new();
/// qubit_io::Streams::copy_input_to_output(&mut reader, &mut bytes)?;
/// assert_eq!(bytes, b"candidate bytes");
/// drop(reader);
/// let role = candidate.role();
/// let retained_path = candidate.diagnostic_path().expect("host diagnostic path").to_path_buf();
/// let recovery_files = error.into_recovery_files();
/// let transferred = recovery_files.iter().find(|file| file.role() == role)
///     .expect("candidate ownership transferred");
/// let mut reader = transferred.open_reader()?;
/// let mut bytes = Vec::new();
/// qubit_io::Streams::copy_input_to_output(&mut reader, &mut bytes)?;
/// assert_eq!(bytes, b"candidate bytes");
/// drop(reader);
/// drop(recovery_files);
/// assert!(retained_path.exists(), "dropping recovery handles preserves files");
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
///
/// The temporary directory owns the example's files and is removed after all
/// recovery handles have been dropped.
#[must_use]
#[derive(Debug)]
pub struct LocalConditionalCommitError {
    /// Structured operation error, including its native source.
    error: LocalFileError,
    /// Stable classification of the conditional failure.
    reason: LocalConditionalFailureReason,
    /// Commit stage where the attempt stopped.
    stage: LocalConditionalWriteStage,
    /// Observed destination publication state; `Indeterminate` requires
    /// inspection before deciding whether to retry.
    publication: LocalConditionalPublicationState,
    /// Recovery files retained for caller inspection or transfer.
    recovery_files: Vec<LocalRecoveryFile>,
}

impl LocalConditionalCommitError {
    /// Creates the structured terminal failure and transfers retained
    /// resources.
    pub(crate) fn new(
        error: LocalFileError,
        reason: LocalConditionalFailureReason,
        stage: LocalConditionalWriteStage,
        publication: LocalConditionalPublicationState,
        recovery_files: Vec<LocalRecoveryFile>,
    ) -> Self {
        Self {
            error,
            reason,
            stage,
            publication,
            recovery_files,
        }
    }

    /// Supplies the public operation path when the failure has no more specific
    /// path.
    ///
    /// The existing error and its native source are retained when a path is
    /// already present.
    pub(crate) fn with_fallback_path(mut self, path: std::path::PathBuf) -> Self {
        if self.error.path().is_none() {
            self.error = self.error.with_path(path);
        }
        self
    }

    /// Adds the operation's captured current directory when the native error
    /// does not already carry one. The native source and recovery ownership
    /// are preserved.
    pub(crate) fn with_fallback_current_directory(mut self, current_directory: Option<std::path::PathBuf>) -> Self {
        if self.error.current_directory().is_none()
            && let Some(current_directory) = current_directory
        {
            self.error = self.error.with_current_directory(current_directory);
        }
        self
    }

    /// Returns the structured local filesystem error, including its native
    /// source.
    pub const fn error(&self) -> &LocalFileError {
        &self.error
    }

    /// Returns the stable conditional failure classification.
    pub const fn reason(&self) -> LocalConditionalFailureReason {
        self.reason
    }

    /// Returns the operation stage at which the attempt stopped.
    pub const fn stage(&self) -> LocalConditionalWriteStage {
        self.stage
    }

    /// Returns the observed destination state when the attempt stopped.
    pub const fn publication(&self) -> LocalConditionalPublicationState {
        self.publication
    }

    /// Returns the independently verified candidate copy, if retained.
    #[must_use]
    pub fn candidate(&self) -> Option<&LocalRecoveryFile> {
        self.recovery_files
            .iter()
            .find(|file| file.role() == LocalRecoveryFileRole::CandidateCopy)
    }

    /// Returns every file retained to support recovery.
    pub fn recovery_files(&self) -> &[LocalRecoveryFile] {
        &self.recovery_files
    }

    /// Consumes the error and transfers every retained recovery file.
    #[must_use]
    pub fn into_recovery_files(self) -> Vec<LocalRecoveryFile> {
        self.recovery_files
    }
}

impl fmt::Display for LocalConditionalCommitError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "conditional commit failed at {:?}: {}",
            self.stage, self.error
        )
    }
}

impl Error for LocalConditionalCommitError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        Some(&self.error)
    }
}

#[cfg(test)]
mod conditional_types_tests {
    use std::error::Error;
    use std::io;

    use super::LocalConditionalCommitError;
    use crate::LocalFileError;
    use crate::LocalFileOperation;
    use crate::error::LocalConditionalFailureReason;
    use crate::error::LocalConditionalWriteStage;
    use crate::outcome::LocalConditionalPublicationState;

    #[test]
    fn conditional_commit_error_preserves_native_io_source() {
        let error = LocalConditionalCommitError::new(
            LocalFileError::from_io(
                LocalFileOperation::Commit,
                None,
                None,
                io::Error::new(io::ErrorKind::PermissionDenied, "native failure"),
            ),
            LocalConditionalFailureReason::Io,
            LocalConditionalWriteStage::Publish,
            LocalConditionalPublicationState::NotPublished,
            Vec::new(),
        );

        let local_error = Error::source(&error).expect("structured local error source");
        let native_error = Error::source(local_error).expect("native I/O error source");
        assert_eq!(
            native_error.downcast_ref::<io::Error>().map(io::Error::kind),
            Some(io::ErrorKind::PermissionDenied)
        );
        assert!(error.to_string().contains("conditional commit failed at Publish"));
        assert!(error.source().is_some());
        assert!(error.into_recovery_files().is_empty());
    }
}
