// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::io;
use std::path::Path;
use std::path::PathBuf;

use digest::Digest;
use digest::Output;

use crate::LocalWriteOutcome;
use crate::LocalWritePublicationMethod;
use crate::LocalWriterState;
use crate::error::LocalConditionalCommitError;
use crate::local::conditional::ConditionalWriterBackend;
use crate::options::LocalWriteOptions;
use crate::options::LocalWritePrecondition;
use crate::outcome::LocalConditionalWriteOutcome;

/// Stateful conditional file writer. Call [`commit`](Self::commit) to publish
/// or [`abort`](Self::abort) to discard its unpublished candidate.
///
/// `commit` consumes the writer. On failure, the returned
/// [`LocalConditionalCommitError`] owns retained recovery files; the writer
/// cannot be committed again. A successful result means the candidate was
/// published and required synchronization completed. Cleanup failures are
/// reported in `cleanup_errors()` and do not undo publication; any files that
/// could not be removed remain owned by `retained_files()`.
///
/// Recovery handles carry a role describing each retained file's place in the
/// attempt. Their read permission is retained independently of the optional
/// diagnostic path, and dropping a recovery handle does not delete its file.
/// `abort` may be retried after a cleanup failure while the writer is open;
/// writing and committing are disabled after a permanent stream or abort
/// failure. Dropping an open writer makes a best-effort abort of its staging
/// candidate.
///
/// # Example
///
/// This example forces an `Absent` precondition failure, then reads the
/// independently verified candidate copy from the error that owns it.
///
/// ```
/// use qubit_io::Output;
/// use qubit_local_files::LocalFileSystem;
/// use qubit_local_files::options::LocalWriteMode;
/// use qubit_local_files::options::LocalWriteOptions;
/// use qubit_local_files::options::LocalWritePrecondition;
/// use qubit_local_files::recovery::LocalRecoveryFileRole;
/// use sha2::Sha256;
/// use tempfile::tempdir;
///
/// let directory = tempdir()?;
/// let target = directory.path().join("model");
/// std::fs::write(&target, b"existing")?;
/// let mut filesystem = LocalFileSystem::host()?;
/// filesystem.set_symlink_policy(qubit_local_files::policy::LocalSymlinkPolicy::Reject)?;
/// let mut writer = filesystem.open_conditional_writer::<Sha256>(
///     &target,
///     LocalWritePrecondition::Absent,
///     &LocalWriteOptions::new(LocalWriteMode::CreateOrReplace),
/// )?;
/// writer.write_fully(b"candidate")?;
/// let error = writer.commit().expect_err("the destination already exists");
/// let candidate = error.candidate().expect("verified candidate copy is retained");
/// assert_eq!(candidate.role(), LocalRecoveryFileRole::CandidateCopy);
/// let mut reader = candidate.open_reader()?;
/// let mut bytes = Vec::new();
/// qubit_io::Streams::copy_input_to_output(&mut reader, &mut bytes)?;
/// assert_eq!(bytes, b"candidate");
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
///
/// [`LocalConditionalCommitError`]: crate::error::LocalConditionalCommitError
#[must_use = "commit or abort the conditional writer"]
pub struct LocalConditionalWriter<D: Digest> {
    path: PathBuf,
    diagnostic_path: Option<PathBuf>,
    /// PWD captured by the resolver when this writer was opened.
    current_directory: Option<PathBuf>,
    backend: Option<ConditionalWriterBackend>,
    precondition: Option<LocalWritePrecondition<Output<D>>>,
    options: LocalWriteOptions,
    state: LocalWriterState,
    bytes_written: usize,
    /// Fatal stream errors disable writes and commits; `Interrupted` and
    /// `WouldBlock` remain retryable.
    write_failed: bool,
    /// An abort error disables writes and commits, while a later explicit
    /// abort may retry cleanup as long as the writer remains open.
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
        current_directory: Option<PathBuf>,
        backend: ConditionalWriterBackend,
        precondition: LocalWritePrecondition<Output<D>>,
        options: LocalWriteOptions,
    ) -> Self {
        Self {
            path,
            diagnostic_path,
            current_directory,
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
    ///
    /// `None` means that no host path was captured; it does not grant
    /// permission to reopen or modify the destination. Operations use the
    /// retained namespace authority instead.
    #[must_use]
    pub fn diagnostic_path(&self) -> Option<&Path> {
        self.diagnostic_path.as_deref()
    }
    /// Returns this writer's lifecycle state.
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

    /// Completes an unsuccessful commit with the PWD captured at open time.
    ///
    /// The commit is terminal after either success or failure, and a failed
    /// commit transfers recovery ownership through its error.
    fn contextualize_commit_error(&self, error: LocalConditionalCommitError) -> LocalConditionalCommitError {
        error.with_fallback_current_directory(self.current_directory.clone())
    }
    /// Publishes the candidate after checking the selected precondition.
    ///
    /// This consumes the writer on both success and failure. On success,
    /// publication and required synchronization are complete, while cleanup
    /// errors and retained files are reported separately. On failure, the
    /// returned error owns any recovery files; inspect or transfer them before
    /// dropping the error. The digest precheck is not a strict
    /// compare-and-swap.
    #[allow(clippy::result_large_err)]
    pub fn commit(mut self) -> Result<LocalConditionalWriteOutcome<D>, LocalConditionalCommitError> {
        if self.state != LocalWriterState::Open || self.write_failed || self.abort_failed {
            return Err(LocalConditionalCommitError::new(
                crate::LocalFileError::new(
                    crate::LocalFileErrorKind::InvalidState,
                    crate::LocalFileOperation::Commit,
                )
                .with_path(self.path.clone()),
                crate::error::LocalConditionalFailureReason::InvalidState,
                crate::error::LocalConditionalWriteStage::Prepare,
                crate::outcome::LocalConditionalPublicationState::NotPublished,
                Vec::new(),
            )
            .with_fallback_current_directory(self.current_directory.clone()));
        }
        let backend = self.backend.take().expect("open conditional writer retains backend");
        let condition = self
            .precondition
            .take()
            .expect("open conditional writer retains precondition");
        let result = match backend {
            #[cfg(any(unix, windows))]
            ConditionalWriterBackend::Native(backend) => {
                crate::local::conditional::commit_conditional_with_directory::<D, _>(
                    backend,
                    condition,
                    self.options,
                    self.bytes_written,
                    self.current_directory.clone(),
                )
            }
            #[cfg(not(any(unix, windows)))]
            _ => match backend {},
        };
        let result =
            result.map_err(|error| self.contextualize_commit_error(error.with_fallback_path(self.path.clone())));
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
            let error = crate::LocalFileError::new(
                crate::LocalFileErrorKind::InvalidState,
                crate::LocalFileOperation::Abort,
            )
            .with_path(self.path.clone());
            return Err(match self.current_directory.clone() {
                Some(directory) => error.with_current_directory(directory),
                None => error,
            });
        }
        let result = self.backend.as_mut().expect("open writer retains backend").abort();
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
                let error = crate::LocalFileError::from_io(
                    crate::LocalFileOperation::Abort,
                    Some(self.path.clone()),
                    None,
                    error,
                );
                Err(match self.current_directory.clone() {
                    Some(directory) => error.with_current_directory(directory),
                    None => error,
                })
            }
        }
    }
    fn write_inner(&mut self, slices: &[&[u8]], vectored: bool) -> io::Result<usize> {
        self.writable()?;
        if vectored && slices.iter().all(|slice| slice.is_empty()) {
            return Ok(0);
        }
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
            Err(error) if matches!(error.kind(), io::ErrorKind::Interrupted | io::ErrorKind::WouldBlock) => Err(error),
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
            Err(error) if matches!(error.kind(), io::ErrorKind::Interrupted | io::ErrorKind::WouldBlock) => Err(error),
            Err(error) => {
                self.write_failed = true;
                Err(error)
            }
        }
    }
    unsafe fn write_unchecked(&mut self, input: &[u8], index: usize, count: usize) -> io::Result<usize> {
        self.writable()?;
        // SAFETY: the Output caller guarantees the indexed range is valid.
        let backend = self.backend.as_mut().expect("open writer retains backend");
        let result = unsafe { qubit_io::Output::write_unchecked(backend, input, index, count) };
        match result {
            Ok(written) => {
                self.bytes_written = self.bytes_written.saturating_add(written);
                Ok(written)
            }
            Err(error) if matches!(error.kind(), io::ErrorKind::Interrupted | io::ErrorKind::WouldBlock) => Err(error),
            Err(error) => {
                self.write_failed = true;
                Err(error)
            }
        }
    }
}

impl<D: Digest> Drop for LocalConditionalWriter<D> {
    fn drop(&mut self) {
        if self.state == LocalWriterState::Open
            && let Some(backend) = &mut self.backend
        {
            let _ = ConditionalWriterBackend::abort(backend);
        }
    }
}
