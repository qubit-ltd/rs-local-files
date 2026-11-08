// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::fmt;
use std::io;
use std::path::Path;
use std::path::PathBuf;

use digest::Digest;
use digest::Output;
use qubit_io::Output as IoOutput;

use crate::LocalWriteOutcome;
use crate::LocalWritePublicationMethod;
use crate::LocalWriterState;
use crate::error::LocalConditionalCommitError;
use crate::local::conditional::ConditionalWriterBackend;
use crate::options::LocalWriteOptions;
use crate::options::LocalWritePrecondition;
use crate::outcome::LocalConditionalWriteOutcome;
use crate::writer::internal::WriterByteCount;

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
/// # Type Parameters
///
/// * `D` implements [`digest::Digest`] and supplies the algorithm used by
///   digest-based preconditions.
///
/// # Examples
///
/// This example forces an `Absent` precondition failure, then reads the
/// independently verified candidate copy from the error that owns it.
///
/// ```
/// use qubit_io::Output;
/// use qubit_local_files::LocalConditionalWriter;
/// use qubit_local_files::LocalFileSystem;
/// use qubit_local_files::options::LocalWriteMode;
/// use qubit_local_files::options::LocalWriteOptions;
/// use qubit_local_files::options::LocalWritePrecondition;
/// use qubit_local_files::recovery::LocalRecoveryFileRole;
/// use sha2::Sha256;
/// use tempfile::tempdir;
///
/// let directory = tempdir()?;
/// let target = directory.path().canonicalize()?.join("model");
/// std::fs::write(&target, b"existing")?;
/// let mut filesystem = LocalFileSystem::host()?;
/// filesystem.set_symlink_policy(qubit_local_files::policy::LocalSymlinkPolicy::Reject)?;
/// let mut writer: LocalConditionalWriter<Sha256> = filesystem.open_conditional_writer::<Sha256>(
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
    /// Namespace-absolute destination path associated with this write.
    path: PathBuf,
    /// Optional host path retained for diagnostics only.
    diagnostic_path: Option<PathBuf>,
    /// PWD captured by the resolver when this writer was opened.
    current_directory: Option<PathBuf>,
    /// Backend that owns the unpublished candidate until commit or abort.
    backend: Option<ConditionalWriterBackend>,
    /// Condition evaluated before publishing the candidate.
    precondition: Option<LocalWritePrecondition<Output<D>>>,
    /// Publication and durability policy for this write.
    options: LocalWriteOptions,
    /// Current lifecycle state of the writer.
    state: LocalWriterState,
    /// Number of bytes accepted by the backend so far.
    bytes_written: WriterByteCount,
    /// Fatal stream errors disable writes and commits; `Interrupted` and
    /// `WouldBlock` remain retryable.
    ///
    /// This flag is set only after a non-retryable write or flush error.
    write_failed: bool,
    /// An abort error disables writes and commits, while a later explicit
    /// abort may retry cleanup as long as the writer remains open.
    ///
    /// This flag does not prevent a later explicit abort attempt.
    abort_failed: bool,
}

impl<D: Digest> fmt::Debug for LocalConditionalWriter<D> {
    /// Formats non-sensitive writer state without exposing filesystem paths.
    ///
    /// # Parameters
    ///
    /// * `f` is the formatter receiving the redacted writer state.
    ///
    /// # Returns
    ///
    /// `Ok(())` when formatting succeeds.
    ///
    /// # Errors
    ///
    /// Returns the formatter's error if output cannot be written.
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("LocalConditionalWriter")
            .field("path", &crate::local_file_system::redacted_path(&self.path))
            .field("state", &self.state)
            .field("bytes_written", &self.bytes_written)
            .finish_non_exhaustive()
    }
}

impl<D: Digest> LocalConditionalWriter<D> {
    /// Creates an open writer from its resolved paths, backend and condition.
    ///
    /// The caller provides the namespace authority and optional diagnostic
    /// context captured when opening the destination.
    ///
    /// # Parameters
    ///
    /// * `path` is the namespace-absolute destination.
    /// * `diagnostic_path` is the optional captured host path for diagnostics.
    /// * `current_directory` is the captured directory used for error context.
    /// * `backend` owns the unpublished candidate.
    /// * `precondition` must hold before publication.
    /// * `options` selects publication and durability behavior.
    ///
    /// # Returns
    ///
    /// An open writer retaining the supplied backend and write condition.
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
            bytes_written: WriterByteCount::default(),
            write_failed: false,
            abort_failed: false,
        }
    }
    /// Returns the namespace-absolute destination path.
    ///
    /// # Returns
    ///
    /// The destination path borrowed for the lifetime of this writer.
    #[must_use]
    #[inline]
    pub fn path(&self) -> &Path {
        &self.path
    }
    /// Returns the captured host path used for diagnostics, when available.
    ///
    /// `None` means that no host path was captured; it does not grant
    /// permission to reopen or modify the destination. Operations use the
    /// retained namespace authority instead.
    ///
    /// # Returns
    ///
    /// `Some` contains the captured host diagnostic path; `None` means no host
    /// path was available when the writer was opened.
    #[inline]
    pub fn diagnostic_path(&self) -> Option<&Path> {
        self.diagnostic_path.as_deref()
    }
    /// Returns this writer's lifecycle state.
    ///
    /// # Returns
    ///
    /// The current state, including whether publication or abort has completed.
    #[inline]
    pub const fn state(&self) -> LocalWriterState {
        self.state
    }
    /// Publishes the candidate after checking the selected precondition.
    ///
    /// This consumes the writer on both success and failure. On success,
    /// publication and required synchronization are complete, while cleanup
    /// errors and retained files are reported separately. On failure, the
    /// returned error owns any recovery files; inspect or transfer them before
    /// dropping the error. The digest precheck is not a strict
    /// compare-and-swap.
    ///
    /// # Returns
    ///
    /// The published outcome on success, or a commit error retaining recovery
    /// files on failure.
    ///
    /// # Errors
    ///
    /// Returns an error if the writer is not committable, the precondition
    /// fails, publication fails, or required synchronization fails.
    ///
    /// # Panics
    ///
    /// Panics if the open-writer invariant is broken and its backend or
    /// precondition is missing.
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
                    self.bytes_written.get(),
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
    ///
    /// # Returns
    ///
    /// The terminal write outcome after cleanup succeeds.
    ///
    /// # Errors
    ///
    /// Returns an error if abort is invalid in the current state or candidate
    /// cleanup fails.
    ///
    /// # Panics
    ///
    /// Panics if the open-writer invariant is broken and its backend is
    /// missing.
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
                    self.bytes_written.get(),
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
    /// Rejects writes after a terminal transition or a permanent prior error.
    ///
    /// # Returns
    ///
    /// `Ok(())` when writes are still permitted.
    ///
    /// # Errors
    ///
    /// Returns `BrokenPipe` after a terminal transition or a permanent prior
    /// write or abort error.
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
    ///
    /// # Parameters
    ///
    /// * `error` is the commit error to enrich with the retained directory
    ///   context.
    ///
    /// # Returns
    ///
    /// The same error enriched with the captured current directory when needed.
    fn contextualize_commit_error(&self, error: LocalConditionalCommitError) -> LocalConditionalCommitError {
        error.with_fallback_current_directory(self.current_directory.clone())
    }
    /// Validates and forwards a scalar or vectored write while tracking bytes.
    ///
    /// Permanent backend errors disable later writes and commits; interrupted
    /// and would-block errors remain retryable.
    ///
    /// # Parameters
    ///
    /// * `slices` contains the input buffers to write.
    /// * `vectored` selects vectored handling and aggregate length validation.
    ///
    /// # Returns
    ///
    /// The number of accepted bytes, or the backend error.
    ///
    /// # Errors
    ///
    /// Returns an error if the writer is disabled, the requested count
    /// overflows, or the backend fails.
    ///
    /// # Panics
    ///
    /// Panics if the open-writer invariant is broken and its backend is
    /// missing.
    fn write_inner(&mut self, slices: &[&[u8]], vectored: bool) -> io::Result<usize> {
        self.writable()?;
        if vectored {
            self.bytes_written.check_vectored_request(slices.iter().copied())?;
        } else {
            self.bytes_written
                .check_request(slices.first().map_or(0, |slice| slice.len()))?;
        }
        if vectored && slices.iter().all(|slice| slice.is_empty()) {
            return Ok(0);
        }
        let backend = self.backend.as_mut().expect("open writer retains backend");
        let result = if vectored {
            IoOutput::write_vectored(backend, slices)
        } else {
            IoOutput::write(backend, slices.first().copied().unwrap_or(&[]))
        };
        match self.bytes_written.record_result(result) {
            Ok(count) => Ok(count),
            Err(error) if matches!(error.kind(), io::ErrorKind::Interrupted | io::ErrorKind::WouldBlock) => Err(error),
            Err(error) => {
                self.write_failed = true;
                Err(error)
            }
        }
    }
}

impl<D: Digest> IoOutput for LocalConditionalWriter<D> {
    /// Byte type accepted by this conditional writer.
    type Item = u8;
    /// Writes a contiguous byte slice and records the accepted byte count.
    ///
    /// # Parameters
    ///
    /// * `bytes` is the contiguous input buffer.
    ///
    /// # Returns
    ///
    /// The number of bytes accepted, or an I/O error.
    ///
    /// # Errors
    ///
    /// Returns the backend error or an error indicating that the writer is no
    /// longer writable or the byte count would overflow.
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.write_inner(&[bytes], false)
    }
    /// Writes from multiple byte slices and records the accepted byte count.
    ///
    /// # Parameters
    ///
    /// * `slices` contains the input buffers in write order.
    ///
    /// # Returns
    ///
    /// The number of bytes accepted, or an I/O error.
    ///
    /// # Errors
    ///
    /// Returns the backend error or an error indicating that the writer is no
    /// longer writable or the total byte count would overflow.
    fn write_vectored(&mut self, slices: &[&[u8]]) -> io::Result<usize> {
        self.write_inner(slices, true)
    }
    /// Flushes the backend, disabling later writes after permanent errors.
    ///
    /// # Returns
    ///
    /// `Ok(())` when the backend flush completes.
    ///
    /// # Errors
    ///
    /// Returns the backend error or an error indicating that the writer is no
    /// longer writable.
    ///
    /// # Panics
    ///
    /// Panics if the open-writer invariant is broken and its backend is
    /// missing.
    fn flush(&mut self) -> io::Result<()> {
        self.writable()?;
        match IoOutput::flush(self.backend.as_mut().expect("open writer retains backend")) {
            Ok(()) => Ok(()),
            Err(error) if matches!(error.kind(), io::ErrorKind::Interrupted | io::ErrorKind::WouldBlock) => Err(error),
            Err(error) => {
                self.write_failed = true;
                Err(error)
            }
        }
    }
    /// Writes a caller-validated range and records the accepted byte count.
    ///
    /// # Parameters
    ///
    /// * `input` is the source buffer.
    /// * `index` is the first byte in the source range.
    /// * `count` is the number of bytes requested.
    ///
    /// # Returns
    ///
    /// The number of bytes accepted from the requested range; a successful
    /// write may accept a shorter prefix.
    ///
    /// # Errors
    ///
    /// Returns a backend error or rejects the write when the writer is no
    /// longer writable or the byte count would overflow.
    ///
    /// # Panics
    ///
    /// Panics if the open-writer invariant is broken and its backend is
    /// missing.
    ///
    /// # Safety
    ///
    /// `index..index + count` must be a valid range within `input`, and the
    /// addition must not overflow.
    unsafe fn write_unchecked(&mut self, input: &[u8], index: usize, count: usize) -> io::Result<usize> {
        self.writable()?;
        self.bytes_written.check_request(count)?;
        // SAFETY: the Output caller guarantees the indexed range is valid.
        let backend = self.backend.as_mut().expect("open writer retains backend");
        let result = unsafe { IoOutput::write_unchecked(backend, input, index, count) };
        match self.bytes_written.record_result(result) {
            Ok(written) => Ok(written),
            Err(error) if matches!(error.kind(), io::ErrorKind::Interrupted | io::ErrorKind::WouldBlock) => Err(error),
            Err(error) => {
                self.write_failed = true;
                Err(error)
            }
        }
    }
}

impl<D: Digest> Drop for LocalConditionalWriter<D> {
    /// Best-effort aborts the staging candidate if the writer is still open.
    fn drop(&mut self) {
        if self.state == LocalWriterState::Open
            && let Some(backend) = &mut self.backend
        {
            let _ = ConditionalWriterBackend::abort(backend);
        }
    }
}

#[cfg(test)]
mod tests {
    use qubit_io::Output;
    use sha2::Sha256;

    use crate::LocalFileSystem;
    use crate::options::LocalWritePrecondition;
    use crate::policy::LocalSymlinkPolicy;
    use crate::writer::internal::WriterByteCount;

    #[test]
    fn test_local_conditional_writer_count_crosses_u32_on_commit() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let mut filesystem = LocalFileSystem::host().expect("host filesystem");
        filesystem
            .set_symlink_policy(LocalSymlinkPolicy::Reject)
            .expect("set symlink policy");
        let target = directory
            .path()
            .canonicalize()
            .expect("temporary directory should resolve to its physical path")
            .join("target");
        let mut writer = filesystem
            .open_conditional_writer::<Sha256>(&target, LocalWritePrecondition::Absent, &Default::default())
            .expect("open conditional writer");
        writer.bytes_written = WriterByteCount::with_initial_value(u64::from(u32::MAX) - 1);
        assert_eq!(3, Output::write(&mut writer, b"abc").expect("write accepted bytes"));
        let outcome = writer.commit().expect("commit conditional writer");
        let accepted: u64 = outcome.write_outcome().bytes_written();
        assert_eq!(u64::from(u32::MAX) + 2, accepted);
    }

    #[test]
    fn test_local_conditional_writer_count_crosses_u32_on_abort() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let mut filesystem = LocalFileSystem::host().expect("host filesystem");
        filesystem
            .set_symlink_policy(LocalSymlinkPolicy::Reject)
            .expect("set symlink policy");
        let target = directory
            .path()
            .canonicalize()
            .expect("temporary directory should resolve to its physical path")
            .join("target");
        let mut writer = filesystem
            .open_conditional_writer::<Sha256>(&target, LocalWritePrecondition::Absent, &Default::default())
            .expect("open conditional writer");
        writer.bytes_written = WriterByteCount::with_initial_value(u64::from(u32::MAX) - 1);
        assert_eq!(3, Output::write(&mut writer, b"abc").expect("write accepted bytes"));
        let outcome = writer.abort().expect("abort conditional writer");
        assert_eq!(u64::from(u32::MAX) + 2, outcome.bytes_written());
    }
}
