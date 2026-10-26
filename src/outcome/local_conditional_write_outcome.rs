// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use digest::Digest;
use digest::Output;

use crate::LocalFileError;
use crate::LocalWriteOutcome;
use crate::recovery::LocalRecoveryFile;

/// Verified result of a successfully published conditional write.
///
/// A successful value means publication and required synchronization completed.
/// Cleanup failures are reported separately and do not change that result.
/// Retained files are owned by this outcome; dropping a recovery handle never
/// deletes the file, so cleanup policy remains with the caller.
///
/// # Example
///
/// ```
/// use qubit_io::Output;
/// use qubit_local_files::LocalFileSystem;
/// use qubit_local_files::options::{LocalWriteMode, LocalWriteOptions, LocalWritePrecondition};
/// use sha2::{Digest, Sha256};
/// use tempfile::tempdir;
///
/// let directory = tempdir()?;
/// let target = directory.path().join("model");
/// let mut filesystem = LocalFileSystem::host()?;
/// filesystem.set_symlink_policy(qubit_local_files::policy::LocalSymlinkPolicy::Reject)?;
/// let mut writer = filesystem.open_conditional_writer::<Sha256>(
///     &target,
///     LocalWritePrecondition::Absent,
///     &LocalWriteOptions::new(LocalWriteMode::CreateNew),
/// )?;
/// writer.write_fully(b"model bytes")?;
/// let outcome = writer.commit()?;
/// assert_eq!(outcome.digest().as_slice(), Sha256::digest(b"model bytes").as_slice());
/// for error in outcome.cleanup_errors() {
///     eprintln!("published, but cleanup failed: {error}");
/// }
/// for retained in outcome.retained_files() {
///     eprintln!("inspect or transfer recovery file: {} ({:?})", retained.namespace_path().display(), retained.role());
/// }
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
#[must_use]
#[derive(Debug)]
pub struct LocalConditionalWriteOutcome<D: Digest> {
    digest: Output<D>,
    write_outcome: LocalWriteOutcome,
    retained_files: Vec<LocalRecoveryFile>,
    cleanup_errors: Vec<LocalFileError>,
}

impl<D: Digest> LocalConditionalWriteOutcome<D> {
    /// Creates a result after publication and required synchronization succeed.
    pub(crate) fn new(
        digest: Output<D>,
        write_outcome: LocalWriteOutcome,
        retained_files: Vec<LocalRecoveryFile>,
        cleanup_errors: Vec<LocalFileError>,
        current_directory: Option<std::path::PathBuf>,
    ) -> Self {
        let cleanup_errors = cleanup_errors
            .into_iter()
            .map(
                |error| match (error.current_directory(), current_directory.as_ref()) {
                    (None, Some(directory)) => error.with_current_directory(directory.clone()),
                    _ => error,
                },
            )
            .collect();
        Self {
            digest,
            write_outcome,
            retained_files,
            cleanup_errors,
        }
    }

    /// Returns the digest computed from the verified installed content.
    #[must_use]
    pub fn digest(&self) -> &Output<D> {
        &self.digest
    }

    /// Returns the byte count, publication method, and durability result.
    pub const fn write_outcome(&self) -> &LocalWriteOutcome {
        &self.write_outcome
    }

    /// Returns files that could not be removed during successful cleanup.
    pub fn retained_files(&self) -> &[LocalRecoveryFile] {
        &self.retained_files
    }

    /// Returns cleanup failures that did not invalidate the completed save.
    pub fn cleanup_errors(&self) -> &[LocalFileError] {
        &self.cleanup_errors
    }
}
