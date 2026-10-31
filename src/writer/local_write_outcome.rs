// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
// Covered by writer integration tests.

use super::LocalWriteFailureState;
use super::LocalWriterState;
use crate::LocalWritePublicationMethod;

/// Structured result of committing or aborting a local writer.
///
/// [`Self::publication_method`] reports the backend method directly; callers
/// do not need to infer it from [`Self::atomic`].
///
/// # Examples
///
/// ```
/// use qubit_io::Output;
/// use tempfile::tempdir;
///
/// use qubit_local_files::LocalFileSystem;
/// use qubit_local_files::outcome::LocalWriterState;
///
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let directory = tempdir()?;
/// let filesystem = LocalFileSystem::host()?;
/// let mut writer = filesystem.open_writer(&directory.path().join("output.txt"))?;
/// writer.write_fully(b"hello")?;
/// let outcome = writer.commit()?;
/// assert_eq!(outcome.state(), LocalWriterState::Committed);
/// # Ok(())
/// # }
/// ```
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[must_use]
pub struct LocalWriteOutcome {
    /// Terminal writer state.
    state: LocalWriterState,
    /// Whether destination publication was atomic.
    atomic: bool,
    /// Native method used by the writer backend.
    publication_method: LocalWritePublicationMethod,
    /// Whether requested durability synchronization completed.
    durable: bool,
    /// Whether file data and metadata synchronization completed.
    file_synced: bool,
    /// Bytes accepted by the writer stream.
    bytes_written: u64,
    /// Failure state retained when a stream failure preceded terminal cleanup.
    failure_state: Option<LocalWriteFailureState>,
}

impl LocalWriteOutcome {
    /// Creates a verified writer outcome.
    ///
    /// # Parameters
    ///
    /// - `state`: Terminal writer state.
    /// - `atomic`: Whether publication was atomic.
    /// - `publication_method`: Selected native backend method.
    /// - `durable`: Whether durability synchronization completed.
    /// - `bytes_written`: Bytes accepted by the stream.
    /// - `failure_state`: Certainty retained from an earlier failure or an
    ///   append abort with accepted bytes.
    pub(crate) const fn new(
        state: LocalWriterState,
        atomic: bool,
        publication_method: LocalWritePublicationMethod,
        durable: bool,
        bytes_written: u64,
        failure_state: Option<LocalWriteFailureState>,
    ) -> Self {
        Self {
            state,
            atomic,
            publication_method,
            durable,
            file_synced: false,
            bytes_written,
            failure_state,
        }
    }

    /// Returns the terminal writer state.
    #[must_use = "inspect the terminal writer state"]
    #[inline]
    pub const fn state(self) -> LocalWriterState {
        self.state
    }

    /// Reports whether destination publication was atomic.
    #[must_use]
    #[inline]
    pub const fn atomic(self) -> bool {
        self.atomic
    }

    /// Returns the native method used by this writer session.
    #[must_use = "the publication method must be inspected or stored"]
    #[inline]
    pub const fn publication_method(self) -> LocalWritePublicationMethod {
        self.publication_method
    }

    /// Reports whether durability synchronization completed.
    #[must_use]
    #[inline]
    pub const fn durable(self) -> bool {
        self.durable
    }

    /// Reports whether file synchronization completed.
    #[must_use]
    #[inline]
    pub const fn file_synced(self) -> bool {
        self.file_synced
    }

    /// Sets the achieved file synchronization state.
    #[inline]
    pub(crate) const fn with_file_synced(mut self, file_synced: bool) -> Self {
        self.file_synced = file_synced;
        self
    }

    /// Returns the number of bytes accepted by the writer stream as a `u64`.
    #[must_use]
    #[inline]
    pub const fn bytes_written(self) -> u64 {
        self.bytes_written
    }

    /// Returns a failure state retained from an earlier stream or publication
    /// error, or `None` when the terminal outcome is fully successful.
    #[inline]
    pub const fn failure_state(self) -> Option<LocalWriteFailureState> {
        self.failure_state
    }
}
