// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::io;
use std::path::{Path, PathBuf};

use digest::{Digest, Output};

use crate::LocalWriteOutcome;
use crate::LocalWritePublicationMethod;
use crate::LocalWriterState;
use crate::error::LocalConditionalCommitError;
use crate::local::conditional::ConditionalWriterBackend;
use crate::options::{LocalWriteOptions, LocalWritePrecondition};
use crate::outcome::LocalConditionalWriteOutcome;

/// Stateful conditional file writer. Call [`commit`](Self::commit) to publish or
/// [`abort`](Self::abort) to discard its unpublished candidate.
#[must_use = "commit or abort the conditional writer"]
pub struct LocalConditionalWriter<D: Digest> {
    path: PathBuf,
    diagnostic_path: Option<PathBuf>,
    backend: Option<ConditionalWriterBackend>,
    precondition: Option<LocalWritePrecondition<Output<D>>>,
    options: LocalWriteOptions,
    state: LocalWriterState,
    bytes_written: usize,
    write_failed: bool,
    abort_failed: bool,
}

impl<D: Digest> std::fmt::Debug for LocalConditionalWriter<D> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LocalConditionalWriter")
            .field("path", &self.path)
            .field("state", &self.state)
            .field("bytes_written", &self.bytes_written)
            .finish_non_exhaustive()
    }
}

impl<D: Digest> LocalConditionalWriter<D> {
    pub(crate) fn new(
        path: PathBuf,
        diagnostic_path: Option<PathBuf>,
        backend: ConditionalWriterBackend,
        precondition: LocalWritePrecondition<Output<D>>,
        options: LocalWriteOptions,
    ) -> Self {
        Self {
            path,
            diagnostic_path,
            backend: Some(backend),
            precondition: Some(precondition),
            options,
            state: LocalWriterState::Open,
            bytes_written: 0,
            write_failed: false,
            abort_failed: false,
        }
    }
    /// Returns the namespace-absolute destination path.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }
    /// Returns the captured host path used for diagnostics, when available.
    #[must_use]
    pub fn diagnostic_path(&self) -> Option<&Path> {
        self.diagnostic_path.as_deref()
    }
    /// Returns this writer's lifecycle state.
    #[must_use]
    pub const fn state(&self) -> LocalWriterState {
        self.state
    }
    fn writable(&self) -> io::Result<()> {
        if self.state != LocalWriterState::Open || self.write_failed || self.abort_failed {
            Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "conditional writer is not writable",
            ))
        } else {
            Ok(())
        }
    }
    /// Publishes the candidate after checking the selected precondition.
    #[allow(clippy::result_large_err)]
    pub fn commit(
        mut self,
    ) -> Result<LocalConditionalWriteOutcome<D>, LocalConditionalCommitError> {
        if self.state != LocalWriterState::Open || self.write_failed || self.abort_failed {
            return Err(LocalConditionalCommitError::new(
                crate::LocalFileError::new(
                    crate::LocalFileErrorKind::InvalidState,
                    crate::LocalFileOperation::Commit,
                )
                .with_path(self.path.clone()),
                crate::error::LocalConditionalFailureReason::Io,
                crate::error::LocalConditionalWriteStage::Prepare,
                crate::outcome::LocalConditionalPublicationState::NotPublished,
                Vec::new(),
            ));
        }
        let backend = self
            .backend
            .take()
            .expect("open conditional writer retains backend");
        let condition = self
            .precondition
            .take()
            .expect("open conditional writer retains precondition");
        let result = match backend {
            #[cfg(any(unix, windows))]
            ConditionalWriterBackend::Native(backend) => {
                crate::local::conditional::commit_conditional::<D, _>(
                    backend,
                    condition,
                    self.options,
                    self.bytes_written,
                )
            }
            #[cfg(not(any(unix, windows)))]
            _ => match backend {},
        };
        let result = result.map_err(|error| error.with_fallback_path(self.path.clone()));
        if result.is_ok() {
            self.state = LocalWriterState::Committed;
        }
        result
    }
    /// Discards the unpublished candidate and reports its terminal byte count.
    /// On failure, the writer retains its backend so abort can be retried;
    /// writing and committing stay disabled.
    pub fn abort(&mut self) -> crate::LocalResult<LocalWriteOutcome> {
        if self.state != LocalWriterState::Open {
            return Err(crate::LocalFileError::new(
                crate::LocalFileErrorKind::InvalidState,
                crate::LocalFileOperation::Abort,
            )
            .with_path(self.path.clone()));
        }
        let result = self
            .backend
            .as_mut()
            .expect("open writer retains backend")
            .abort();
        match result {
            Ok(()) => {
                self.backend.take();
                self.precondition.take();
                self.state = LocalWriterState::Aborted;
                Ok(LocalWriteOutcome::new(
                    self.state,
                    false,
                    LocalWritePublicationMethod::AtomicRename,
                    false,
                    self.bytes_written,
                    None,
                ))
            }
            Err(error) => {
                self.abort_failed = true;
                Err(crate::LocalFileError::from_io(
                    crate::LocalFileOperation::Abort,
                    Some(self.path.clone()),
                    None,
                    error,
                ))
            }
        }
    }
    fn write_inner(&mut self, slices: &[&[u8]], vectored: bool) -> io::Result<usize> {
        self.writable()?;
        let backend = self.backend.as_mut().expect("open writer retains backend");
        let result = if vectored {
            qubit_io::Output::write_vectored(backend, slices)
        } else {
            qubit_io::Output::write(backend, slices.first().copied().unwrap_or(&[]))
        };
        match result {
            Ok(count) => {
                self.bytes_written = self.bytes_written.saturating_add(count);
                Ok(count)
            }
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::Interrupted | io::ErrorKind::WouldBlock
                ) =>
            {
                Err(error)
            }
            Err(error) => {
                self.write_failed = true;
                Err(error)
            }
        }
    }
}

impl<D: Digest> qubit_io::Output for LocalConditionalWriter<D> {
    type Item = u8;
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.write_inner(&[bytes], false)
    }
    fn write_vectored(&mut self, slices: &[&[u8]]) -> io::Result<usize> {
        self.write_inner(slices, true)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.writable()?;
        match qubit_io::Output::flush(self.backend.as_mut().expect("open writer retains backend")) {
            Ok(()) => Ok(()),
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::Interrupted | io::ErrorKind::WouldBlock
                ) =>
            {
                Err(error)
            }
            Err(error) => {
                self.write_failed = true;
                Err(error)
            }
        }
    }
    unsafe fn write_unchecked(
        &mut self,
        input: &[u8],
        index: usize,
        count: usize,
    ) -> io::Result<usize> {
        self.writable()?;
        // SAFETY: the Output caller guarantees the indexed range is valid.
        let backend = self.backend.as_mut().expect("open writer retains backend");
        let result = unsafe { qubit_io::Output::write_unchecked(backend, input, index, count) };
        match result {
            Ok(written) => {
                self.bytes_written = self.bytes_written.saturating_add(written);
                Ok(written)
            }
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::Interrupted | io::ErrorKind::WouldBlock
                ) =>
            {
                Err(error)
            }
            Err(error) => {
                self.write_failed = true;
                Err(error)
            }
        }
    }
}

impl<D: Digest> Drop for LocalConditionalWriter<D> {
    fn drop(&mut self) {
        if self.state == LocalWriterState::Open {
            if let Some(backend) = &mut self.backend {
                let _ = ConditionalWriterBackend::abort(backend);
            }
        }
    }
}
