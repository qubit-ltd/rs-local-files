// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::io;
use std::io::IoSlice;
use std::io::Write;
use std::path::Path;
use std::path::PathBuf;

use super::internal::LocalFileWriterBackend;
use crate::LocalDurabilityRequirement;
use crate::LocalFileCommitError;
use crate::LocalFileError;
use crate::LocalFileErrorKind;
use crate::LocalFileOperation;
use crate::LocalResult;
use crate::LocalWriteFailureState;
use crate::LocalWriteOptions;
use crate::LocalWriteOutcome;
use crate::LocalWritePublicationMethod;
use crate::LocalWriterState;

/// Stateful native byte output and destination publication session.
///
/// # Examples
///
/// ```no_run
/// use std::io::Write;
/// use std::path::Path;
///
/// use qubit_local_files::LocalFileSystem;
///
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let filesystem = LocalFileSystem::host()?;
/// let mut writer = filesystem.open_writer(Path::new("output.txt"))?;
/// writer.write_all(b"hello")?;
/// let _outcome = writer.commit()?;
/// # Ok(())
/// # }
/// ```
#[must_use = "a local writer has no effect unless it is committed or aborted"]
#[derive(Debug)]
pub struct LocalFileWriter {
    /// Reusable namespace-absolute destination path.
    path: PathBuf,
    /// Non-authoritative destination path captured for diagnostics.
    diagnostic_path: Option<PathBuf>,
    /// Namespace-absolute PWD captured when the writer was opened.
    current_directory: Option<PathBuf>,
    /// Selected native write backend while the session is open.
    backend: Option<LocalFileWriterBackend>,
    /// Policy fixed when the writer is opened.
    options: LocalWriteOptions,
    /// Current observable session state.
    state: LocalWriterState,
    /// Bytes accepted by successful stream writes.
    bytes_written: usize,
    /// Failure state retained after an uncertain stream write.
    failure_state: Option<LocalWriteFailureState>,
}

impl LocalFileWriter {
    /// Creates a writer around a selected native backend.
    ///
    /// # Parameters
    ///
    /// - `diagnostic_path`: Destination path captured for diagnostics.
    /// - `backend`: Staged or append backend.
    /// - `options`: Writer policy.
    #[inline]
    pub(crate) fn new(diagnostic_path: PathBuf, backend: LocalFileWriterBackend, options: LocalWriteOptions) -> Self {
        Self {
            path: diagnostic_path.clone(),
            diagnostic_path: Some(diagnostic_path),
            current_directory: None,
            backend: Some(backend),
            options,
            state: LocalWriterState::Open,
            bytes_written: 0,
            failure_state: None,
        }
    }

    /// Replaces the public identity with its normalized namespace path.
    pub(crate) fn bind_namespace(mut self, path: PathBuf, current_directory: PathBuf) -> Self {
        self.path = path;
        self.current_directory = Some(current_directory);
        self
    }

    /// Returns the reusable namespace-absolute destination path.
    #[must_use]
    #[inline]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Returns the non-authoritative destination path captured for diagnostics.
    ///
    /// Rooted writers retain descriptor authority, so this path can refer to a
    /// replacement after the opened root is renamed.
    #[must_use]
    #[inline]
    pub fn diagnostic_path(&self) -> Option<&Path> {
        self.diagnostic_path.as_deref()
    }

    /// Returns the current writer state.
    #[inline]
    pub const fn state(&self) -> LocalWriterState {
        self.state
    }

    /// Returns an uncertainty retained from an earlier stream failure.
    #[must_use]
    #[inline]
    pub const fn failure_state(&self) -> Option<LocalWriteFailureState> {
        self.failure_state
    }

    /// Commits bytes and destination publication.
    ///
    /// # Returns
    ///
    /// Achieved atomicity, durability, and byte count.
    ///
    /// # Errors
    ///
    /// Returns `LocalFileCommitError` with a retryable writer only when staged
    /// publication has not started. A `Published` state means the destination
    /// changed before a later durability failure.
    #[allow(clippy::result_large_err)]
    pub fn commit(mut self) -> Result<LocalWriteOutcome, LocalFileCommitError> {
        if self.state != LocalWriterState::Open || self.failure_state == Some(LocalWriteFailureState::Indeterminate) {
            let failure_state = self.failure_state.unwrap_or(LocalWriteFailureState::NotPublished);
            return Err(LocalFileCommitError::new(
                self.contextualize_error(publication_error(
                    writer_state_error(&self.path, LocalFileOperation::Commit, self.state),
                    failure_state,
                )),
                failure_state,
                None,
            ));
        }
        let backend = self.backend.take().expect("open writer must retain one backend");
        match backend {
            backend @ (LocalFileWriterBackend::Staged(_) | LocalFileWriterBackend::Rooted(_)) => {
                self.commit_staged_backend(backend)
            }
            LocalFileWriterBackend::Append(mut file) => {
                #[cfg(feature = "internal-test-support")]
                let flush_result = if crate::local::test_support_enabled("writer-append-commit-flush") {
                    Err(crate::local::test_fault_error())
                } else {
                    file.flush()
                };
                #[cfg(not(feature = "internal-test-support"))]
                let flush_result = file.flush();
                if let Err(error) = flush_result {
                    return Err(LocalFileCommitError::new(
                        self.contextualize_error(publication_error(
                            writer_io_error(&self.path, LocalFileOperation::Commit, error),
                            LocalWriteFailureState::Indeterminate,
                        )),
                        LocalWriteFailureState::Indeterminate,
                        None,
                    ));
                }
                let durable = match self.options.durability() {
                    LocalDurabilityRequirement::NotRequired => false,
                    LocalDurabilityRequirement::Preferred => file.sync_all().is_ok(),
                    LocalDurabilityRequirement::Required => {
                        #[cfg(feature = "internal-test-support")]
                        let sync_result = if crate::local::test_support_enabled("writer-append-required-sync") {
                            Err(crate::local::test_fault_error())
                        } else {
                            file.sync_all()
                        };
                        #[cfg(not(feature = "internal-test-support"))]
                        let sync_result = file.sync_all();
                        if let Err(error) = sync_result {
                            return Err(LocalFileCommitError::new(
                                self.contextualize_error(publication_error(
                                    writer_io_error(&self.path, LocalFileOperation::Commit, error),
                                    LocalWriteFailureState::Published,
                                )),
                                LocalWriteFailureState::Published,
                                None,
                            ));
                        }
                        true
                    }
                };
                self.state = LocalWriterState::Committed;
                Ok(LocalWriteOutcome::new(
                    self.state,
                    false,
                    LocalWritePublicationMethod::DirectAppend,
                    durable,
                    self.bytes_written,
                    self.failure_state,
                ))
            }
        }
    }

    /// Aborts staged publication or closes direct append.
    ///
    /// # Returns
    ///
    /// An `Aborted` lifecycle outcome. Staged cleanup proves the destination
    /// unchanged; append with accepted bytes records `Published` because
    /// direct writes cannot be rolled back.
    ///
    /// # Errors
    ///
    /// Returns `LocalFileError` when the writer is terminal, staging cleanup
    /// fails, or append flushing fails. A cleanup failure retains the open
    /// writer so the caller can retry abort.
    pub fn abort(&mut self) -> LocalResult<LocalWriteOutcome> {
        if self.state != LocalWriterState::Open {
            return Err(self.contextualize_error(writer_state_error(
                &self.path,
                LocalFileOperation::Abort,
                self.state,
            )));
        }
        let previous_failure_state = self.failure_state;
        let backend = self.backend.as_mut().expect("open writer must retain one backend");
        match backend {
            backend @ (LocalFileWriterBackend::Staged(_) | LocalFileWriterBackend::Rooted(_)) => {
                if let Err(error) = backend.abort_staged() {
                    return Err(self.contextualize_error(atomic_write_error(
                        &self.path,
                        LocalFileOperation::Abort,
                        error,
                    )));
                }
                self.state = LocalWriterState::Aborted;
                Ok(LocalWriteOutcome::new(
                    self.state,
                    false,
                    LocalWritePublicationMethod::AtomicRename,
                    false,
                    self.bytes_written,
                    self.failure_state,
                ))
            }
            LocalFileWriterBackend::Append(file) => {
                #[cfg(feature = "internal-test-support")]
                let flush_result = if crate::local::test_support_enabled("writer-append-abort-flush") {
                    Err(crate::local::test_fault_error())
                } else {
                    file.flush()
                };
                #[cfg(not(feature = "internal-test-support"))]
                let flush_result = file.flush();
                if let Err(error) = flush_result {
                    return Err(self.contextualize_error(writer_io_error(
                        &self.path,
                        LocalFileOperation::Abort,
                        error,
                    )));
                }
                self.state = LocalWriterState::Aborted;
                self.failure_state = previous_failure_state
                    .or_else(|| (self.bytes_written > 0).then_some(LocalWriteFailureState::Published));
                Ok(LocalWriteOutcome::new(
                    self.state,
                    false,
                    LocalWritePublicationMethod::DirectAppend,
                    false,
                    self.bytes_written,
                    self.failure_state,
                ))
            }
        }
    }

    /// Commits either staged backend through the shared publication contract.
    #[allow(clippy::result_large_err)]
    fn commit_staged_backend(
        &mut self,
        backend: LocalFileWriterBackend,
    ) -> Result<LocalWriteOutcome, LocalFileCommitError> {
        match backend.commit_staged() {
            Ok(durable) => {
                self.state = LocalWriterState::Committed;
                Ok(LocalWriteOutcome::new(
                    self.state,
                    true,
                    LocalWritePublicationMethod::AtomicRename,
                    durable,
                    self.bytes_written,
                    self.failure_state,
                ))
            }
            Err(commit_error) => {
                let (error, retained) = commit_error.into_parts();
                let state = atomic_destination_state(error.destination_state());
                let retained = retained.map(|backend| self.retain_backend(backend));
                Err(LocalFileCommitError::new(
                    self.contextualize_error(publication_error(
                        atomic_write_error(&self.path, LocalFileOperation::Commit, error),
                        state,
                    )),
                    state,
                    retained,
                ))
            }
        }
    }

    /// Rebuilds a retryable writer without losing stream accounting.
    ///
    /// # Parameters
    ///
    /// - `backend`: Retained staged backend.
    ///
    /// # Returns
    ///
    /// A writer carrying the original path, options, state, and byte count.
    #[inline]
    fn retain_backend(&self, backend: LocalFileWriterBackend) -> Self {
        Self {
            path: self.path.clone(),
            diagnostic_path: self.diagnostic_path.clone(),
            current_directory: self.current_directory.clone(),
            backend: Some(backend),
            options: self.options,
            state: self.state,
            bytes_written: self.bytes_written,
            failure_state: self.failure_state,
        }
    }

    /// Records bytes accepted by a successful stream operation.
    ///
    /// # Parameters
    ///
    /// - `written`: Bytes accepted by the backend.
    #[inline]
    fn record_written(&mut self, written: usize) {
        self.bytes_written = self.bytes_written.saturating_add(written);
    }

    /// Marks an ordinary stream error as indeterminate.
    ///
    /// # Parameters
    ///
    /// - `result`: Backend stream result.
    ///
    /// # Returns
    ///
    /// The original result.
    #[inline]
    fn observe_stream_result<T>(&mut self, result: io::Result<T>) -> io::Result<T> {
        if result.is_err() {
            self.failure_state = Some(LocalWriteFailureState::Indeterminate);
        }
        result
    }

    /// Attaches the creation-time PWD to one structured session error.
    fn contextualize_error(&self, error: LocalFileError) -> LocalFileError {
        match &self.current_directory {
            Some(current_directory) => error.with_current_directory(current_directory.clone()),
            None => error,
        }
    }
}

impl Write for LocalFileWriter {
    /// Writes bytes to staging or directly appends to the destination.
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        if self.state != LocalWriterState::Open || self.failure_state == Some(LocalWriteFailureState::Indeterminate) {
            return Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "local file writer is not open",
            ));
        }
        let result = match self.backend.as_mut() {
            Some(LocalFileWriterBackend::Staged(writer)) => writer.write(buffer),
            Some(LocalFileWriterBackend::Rooted(writer)) => writer.write(buffer),
            Some(LocalFileWriterBackend::Append(file)) => file.write(buffer),
            None => unreachable!("open writer must retain one backend"),
        };
        let written = self.observe_stream_result(result)?;
        self.record_written(written);
        Ok(written)
    }

    /// Writes vectored bytes to staging or directly appends to the destination.
    fn write_vectored(&mut self, buffers: &[IoSlice<'_>]) -> io::Result<usize> {
        if self.state != LocalWriterState::Open || self.failure_state == Some(LocalWriteFailureState::Indeterminate) {
            return Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "local file writer is not open",
            ));
        }

        // Windows' standard file handle reports vectored writes as supported
        // while only consuming the first slice.  Preserve the writer's
        // cross-platform contract by completing the remaining slices through
        // the ordinary write path.
        #[cfg(windows)]
        {
            let mut written = 0;
            for buffer in buffers {
                let count = self.write(buffer)?;
                written += count;
                if count < buffer.len() {
                    break;
                }
            }
            Ok(written)
        }

        #[cfg(not(windows))]
        {
            let result = match self.backend.as_mut() {
                Some(LocalFileWriterBackend::Staged(writer)) => writer.write_vectored(buffers),
                Some(LocalFileWriterBackend::Rooted(writer)) => writer.write_vectored(buffers),
                Some(LocalFileWriterBackend::Append(file)) => file.write_vectored(buffers),
                None => unreachable!("open writer must retain one backend"),
            };
            let written = self.observe_stream_result(result)?;
            self.record_written(written);
            Ok(written)
        }
    }

    /// Flushes userspace buffers without publishing staged content.
    fn flush(&mut self) -> io::Result<()> {
        if self.state != LocalWriterState::Open || self.failure_state == Some(LocalWriteFailureState::Indeterminate) {
            return Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "local file writer is not open",
            ));
        }
        let result = match self.backend.as_mut() {
            Some(LocalFileWriterBackend::Staged(writer)) => writer.flush(),
            Some(LocalFileWriterBackend::Rooted(writer)) => writer.flush(),
            Some(LocalFileWriterBackend::Append(file)) => file.flush(),
            None => unreachable!("open writer must retain one backend"),
        };
        self.observe_stream_result(result)
    }
}

/// Maps the existing atomic destination state to the unified writer state.
///
/// # Parameters
///
/// - `state`: Atomic writer destination state.
///
/// # Returns
///
/// Unified publication state.
fn atomic_destination_state(state: crate::local::LocalAtomicDestinationState) -> LocalWriteFailureState {
    match state {
        crate::local::LocalAtomicDestinationState::Unchanged | crate::local::LocalAtomicDestinationState::Missing => {
            LocalWriteFailureState::NotPublished
        }
        crate::local::LocalAtomicDestinationState::Replaced => LocalWriteFailureState::Published,
        crate::local::LocalAtomicDestinationState::Indeterminate => LocalWriteFailureState::Indeterminate,
    }
}

/// Converts the existing atomic writer error into the unified error domain.
///
/// # Parameters
///
/// - `path`: Bound destination path.
/// - `error`: Existing structured atomic-write error.
///
/// # Returns
///
/// Unified local filesystem error retaining the atomic error as its source.
#[must_use]
#[inline]
fn atomic_write_error(
    path: &Path,
    operation: LocalFileOperation,
    error: crate::local::LocalAtomicWriteError,
) -> LocalFileError {
    let kind = error.kind();
    writer_io_error(path, operation, io::Error::new(kind, error))
}

/// Adds writer operation context to a native I/O failure.
///
/// # Parameters
///
/// - `path`: Bound destination path.
/// - `operation`: Commit or abort operation being performed.
/// - `error`: Native I/O failure.
///
/// # Returns
///
/// Structured writer error.
#[must_use]
#[inline(always)]
fn writer_io_error(path: &Path, operation: LocalFileOperation, error: io::Error) -> LocalFileError {
    LocalFileError::from_io(operation, Some(path.to_path_buf()), None, error)
}

/// Builds a structured error for a forbidden writer state transition.
///
/// # Parameters
///
/// - `path`: Bound destination path.
/// - `operation`: Requested terminal operation.
/// - `state`: Current non-open state.
///
/// # Returns
///
/// An invalid-state error retaining the operation and path.
#[inline]
fn writer_state_error(path: &Path, operation: LocalFileOperation, state: LocalWriterState) -> LocalFileError {
    LocalFileError::from_io(
        operation,
        Some(path.to_path_buf()),
        None,
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("local file writer cannot transition from {state:?}"),
        ),
    )
    .with_kind(LocalFileErrorKind::InvalidState)
}

/// Adds partial-publication classification to a terminal writer failure.
///
/// # Parameters
///
/// - `error`: Original structured I/O error.
/// - `state`: Publication state established by the failed operation.
///
/// # Returns
///
/// Error classified consistently with the observable publication state.
fn publication_error(error: LocalFileError, state: LocalWriteFailureState) -> LocalFileError {
    match state {
        LocalWriteFailureState::NotPublished => error,
        LocalWriteFailureState::Published => error.with_kind(LocalFileErrorKind::PublicationIncomplete),
        LocalWriteFailureState::Indeterminate => error.with_kind(LocalFileErrorKind::Indeterminate),
    }
}
