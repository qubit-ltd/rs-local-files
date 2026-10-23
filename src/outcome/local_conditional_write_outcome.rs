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
    ) -> Self {
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
    #[must_use]
    pub const fn write_outcome(&self) -> &LocalWriteOutcome {
        &self.write_outcome
    }

    /// Returns files that could not be removed during successful cleanup.
    #[must_use]
    pub fn retained_files(&self) -> &[LocalRecoveryFile] {
        &self.retained_files
    }

    /// Returns cleanup failures that did not invalidate the completed save.
    #[must_use]
    pub fn cleanup_errors(&self) -> &[LocalFileError] {
        &self.cleanup_errors
    }
}
