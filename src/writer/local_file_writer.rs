// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
// qubit-style: allow coverage-cfg

use std::{
    io::{
        self,
        IoSlice,
        Write,
    },
    path::{
        Path,
        PathBuf,
    },
};

use crate::{
    LocalDurabilityRequirement,
    LocalFileCommitError,
    LocalFileError,
    LocalFileErrorKind,
    LocalFileOperation,
    LocalMutationState,
    LocalResult,
    LocalWriteOptions,
    LocalWriteOutcome,
    LocalWriterState,
};

use super::internal::LocalFileWriterBackend;

/// Stateful native byte output and destination publication session.
#[derive(Debug)]
pub struct LocalFileWriter {
    /// Bound destination path.
    path: PathBuf,
    /// Selected native write backend while the session is open.
    backend: Option<LocalFileWriterBackend>,
    /// Policy fixed when the writer is opened.
    options: LocalWriteOptions,
    /// Current observable session state.
    state: LocalWriterState,
    /// Bytes accepted by successful stream writes.
    bytes_written: u64,
}

impl LocalFileWriter {
    /// Creates a writer around a selected native backend.
    ///
    /// # Parameters
    ///
    /// - `path`: Bound destination path.
    /// - `backend`: Staged or append backend.
    /// - `options`: Writer policy.
    #[inline]
    pub(crate) const fn new(
        path: PathBuf,
        backend: LocalFileWriterBackend,
        options: LocalWriteOptions,
    ) -> Self {
        Self {
            path,
            backend: Some(backend),
            options,
            state: LocalWriterState::Open,
            bytes_written: 0,
        }
    }

    /// Returns the bound destination path.
    #[must_use]
    #[inline(always)]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Returns the current writer state.
    #[inline(always)]
    pub const fn state(&self) -> LocalWriterState {
        self.state
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
    pub fn commit(mut self) -> Result<LocalWriteOutcome, LocalFileCommitError> {
        if self.state != LocalWriterState::Open {
            return Err(LocalFileCommitError::new(
                writer_state_error(
                    &self.path,
                    LocalFileOperation::Commit,
                    self.state,
                ),
                self.state,
                None,
            ));
        }
        let backend = self
            .backend
            .take()
            .expect("open writer must retain one backend");
        match backend {
            LocalFileWriterBackend::Staged(writer) => {
                match writer.commit_recoverable_with_durability() {
                    Ok(durable) => {
                        self.state = LocalWriterState::Committed;
                        Ok(LocalWriteOutcome::new(
                            self.state,
                            true,
                            durable,
                            self.bytes_written,
                        ))
                    }
                    Err(commit_error) => {
                        let (error, retained) = commit_error.into_parts();
                        let state =
                            atomic_destination_state(error.destination_state());
                        let retained = retained.map(|writer| {
                            self.retain_backend(LocalFileWriterBackend::Staged(
                                writer,
                            ))
                        });
                        Err(LocalFileCommitError::new(
                            publication_error(
                                atomic_write_error(
                                    &self.path,
                                    LocalFileOperation::Commit,
                                    error,
                                ),
                                state,
                            ),
                            state,
                            retained,
                        ))
                    }
                }
            }
            LocalFileWriterBackend::Rooted(writer) => {
                match writer.commit_recoverable_with_durability() {
                    Ok(durable) => {
                        self.state = LocalWriterState::Committed;
                        Ok(LocalWriteOutcome::new(
                            self.state,
                            true,
                            durable,
                            self.bytes_written,
                        ))
                    }
                    Err(commit_error) => {
                        let (error, retained) = commit_error.into_parts();
                        let state =
                            atomic_destination_state(error.destination_state());
                        let retained = retained.map(|writer| {
                            self.retain_backend(LocalFileWriterBackend::Rooted(
                                writer,
                            ))
                        });
                        Err(LocalFileCommitError::new(
                            publication_error(
                                atomic_write_error(
                                    &self.path,
                                    LocalFileOperation::Commit,
                                    error,
                                ),
                                state,
                            ),
                            state,
                            retained,
                        ))
                    }
                }
            }
            LocalFileWriterBackend::Append(mut file) => {
                #[cfg(coverage)]
                let flush_result = if crate::local::coverage_fault_enabled(
                    "writer-append-commit-flush",
                ) {
                    Err(io::Error::from_raw_os_error(libc::EIO))
                } else {
                    file.flush()
                };
                #[cfg(not(coverage))]
                let flush_result = file.flush();
                if let Err(error) = flush_result {
                    return Err(LocalFileCommitError::new(
                        publication_error(
                            writer_io_error(
                                &self.path,
                                LocalFileOperation::Commit,
                                error,
                            ),
                            LocalWriterState::Indeterminate,
                        ),
                        LocalWriterState::Indeterminate,
                        None,
                    ));
                }
                let durable = match self.options.durability() {
                    LocalDurabilityRequirement::NotRequired => false,
                    LocalDurabilityRequirement::Preferred => {
                        file.sync_all().is_ok()
                    }
                    LocalDurabilityRequirement::Required => {
                        #[cfg(coverage)]
                        let sync_result =
                            if crate::local::coverage_fault_enabled(
                                "writer-append-required-sync",
                            ) {
                                Err(io::Error::from_raw_os_error(libc::EIO))
                            } else {
                                file.sync_all()
                            };
                        #[cfg(not(coverage))]
                        let sync_result = file.sync_all();
                        if let Err(error) = sync_result {
                            return Err(LocalFileCommitError::new(
                                publication_error(
                                    writer_io_error(
                                        &self.path,
                                        LocalFileOperation::Commit,
                                        error,
                                    ),
                                    LocalWriterState::Published,
                                ),
                                LocalWriterState::Published,
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
                    durable,
                    self.bytes_written,
                ))
            }
        }
    }

    /// Aborts staged publication or closes direct append.
    ///
    /// # Returns
    ///
    /// An `Aborted` outcome for staging. Append with accepted bytes returns
    /// `Published` because direct writes cannot be rolled back.
    ///
    /// # Errors
    ///
    /// Returns `LocalFileError` when staging cleanup or append flush fails.
    pub fn abort(mut self) -> LocalResult<LocalWriteOutcome> {
        let previous_state = self.state;
        let backend = self
            .backend
            .take()
            .expect("open writer must retain one backend");
        match backend {
            LocalFileWriterBackend::Staged(writer) => {
                if let Err(error) = writer.abort() {
                    return Err(atomic_write_error(
                        &self.path,
                        LocalFileOperation::Abort,
                        error,
                    ));
                }
                self.state = aborted_state(previous_state);
                Ok(LocalWriteOutcome::new(
                    self.state,
                    false,
                    false,
                    self.bytes_written,
                ))
            }
            LocalFileWriterBackend::Rooted(writer) => {
                if let Err(error) = writer.abort() {
                    return Err(atomic_write_error(
                        &self.path,
                        LocalFileOperation::Abort,
                        error,
                    ));
                }
                self.state = aborted_state(previous_state);
                Ok(LocalWriteOutcome::new(
                    self.state,
                    false,
                    false,
                    self.bytes_written,
                ))
            }
            LocalFileWriterBackend::Append(mut file) => {
                #[cfg(coverage)]
                let flush_result = if crate::local::coverage_fault_enabled(
                    "writer-append-abort-flush",
                ) {
                    Err(io::Error::from_raw_os_error(libc::EIO))
                } else {
                    file.flush()
                };
                #[cfg(not(coverage))]
                let flush_result = file.flush();
                if let Err(error) = flush_result {
                    return Err(writer_io_error(
                        &self.path,
                        LocalFileOperation::Abort,
                        error,
                    ));
                }
                self.state =
                    if previous_state == LocalWriterState::Indeterminate {
                        LocalWriterState::Indeterminate
                    } else if self.bytes_written == 0 {
                        LocalWriterState::Aborted
                    } else {
                        LocalWriterState::Published
                    };
                Ok(LocalWriteOutcome::new(
                    self.state,
                    false,
                    false,
                    self.bytes_written,
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
            backend: Some(backend),
            options: self.options,
            state: self.state,
            bytes_written: self.bytes_written,
        }
    }

    /// Records bytes accepted by a successful stream operation.
    ///
    /// # Parameters
    ///
    /// - `written`: Bytes accepted by the backend.
    #[inline]
    fn record_written(&mut self, written: usize) {
        self.bytes_written = self.bytes_written.saturating_add(written as u64);
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
    fn observe_stream_result<T>(
        &mut self,
        result: io::Result<T>,
    ) -> io::Result<T> {
        if result.is_err() {
            self.state = LocalWriterState::Indeterminate;
        }
        result
    }
}

impl Write for LocalFileWriter {
    /// Writes bytes to staging or directly appends to the destination.
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        if self.state != LocalWriterState::Open {
            return Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "local file writer is not open",
            ));
        }
        let result = match self.backend.as_mut() {
            Some(LocalFileWriterBackend::Staged(writer)) => {
                writer.write(buffer)
            }
            Some(LocalFileWriterBackend::Rooted(writer)) => {
                writer.write(buffer)
            }
            Some(LocalFileWriterBackend::Append(file)) => file.write(buffer),
            None => unreachable!("open writer must retain one backend"),
        };
        let written = self.observe_stream_result(result)?;
        self.record_written(written);
        Ok(written)
    }

    /// Writes vectored bytes to staging or directly appends to the destination.
    fn write_vectored(&mut self, buffers: &[IoSlice<'_>]) -> io::Result<usize> {
        if self.state != LocalWriterState::Open {
            return Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "local file writer is not open",
            ));
        }
        let result = match self.backend.as_mut() {
            Some(LocalFileWriterBackend::Staged(writer)) => {
                writer.write_vectored(buffers)
            }
            Some(LocalFileWriterBackend::Rooted(writer)) => {
                writer.write_vectored(buffers)
            }
            Some(LocalFileWriterBackend::Append(file)) => {
                file.write_vectored(buffers)
            }
            None => unreachable!("open writer must retain one backend"),
        };
        let written = self.observe_stream_result(result)?;
        self.record_written(written);
        Ok(written)
    }

    /// Flushes userspace buffers without publishing staged content.
    fn flush(&mut self) -> io::Result<()> {
        if self.state != LocalWriterState::Open {
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
fn atomic_destination_state(
    state: crate::local::LocalAtomicDestinationState,
) -> LocalWriterState {
    match state {
        crate::local::LocalAtomicDestinationState::Unchanged
        | crate::local::LocalAtomicDestinationState::Missing => {
            LocalWriterState::NotPublished
        }
        crate::local::LocalAtomicDestinationState::Replaced => {
            LocalWriterState::Published
        }
        crate::local::LocalAtomicDestinationState::Indeterminate => {
            LocalWriterState::Indeterminate
        }
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
fn writer_io_error(
    path: &Path,
    operation: LocalFileOperation,
    error: io::Error,
) -> LocalFileError {
    LocalFileError::from_io(operation, Some(path.to_path_buf()), None, error)
}

/// Returns the terminal state produced by successful staging cleanup.
///
/// # Parameters
///
/// - `previous_state`: State observed before abort began.
///
/// # Returns
///
/// `Indeterminate` when a prior stream failure made byte state uncertain;
/// otherwise `Aborted`.
#[inline]
const fn aborted_state(previous_state: LocalWriterState) -> LocalWriterState {
    if matches!(previous_state, LocalWriterState::Indeterminate) {
        LocalWriterState::Indeterminate
    } else {
        LocalWriterState::Aborted
    }
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
/// An invalid-input error retaining the operation and path.
#[inline]
fn writer_state_error(
    path: &Path,
    operation: LocalFileOperation,
    state: LocalWriterState,
) -> LocalFileError {
    LocalFileError::from_io(
        operation,
        Some(path.to_path_buf()),
        None,
        io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("local file writer cannot transition from {state:?}"),
        ),
    )
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
fn publication_error(
    error: LocalFileError,
    state: LocalWriterState,
) -> LocalFileError {
    match state {
        LocalWriterState::NotPublished => {
            error.with_mutation_state(LocalMutationState::NotPublished)
        }
        LocalWriterState::Published => error
            .with_kind(LocalFileErrorKind::PublicationIncomplete)
            .with_mutation_state(LocalMutationState::Published),
        LocalWriterState::Indeterminate => error
            .with_kind(LocalFileErrorKind::Indeterminate)
            .with_mutation_state(LocalMutationState::Indeterminate),
        LocalWriterState::Open
        | LocalWriterState::Committed
        | LocalWriterState::Aborted => error,
    }
}
