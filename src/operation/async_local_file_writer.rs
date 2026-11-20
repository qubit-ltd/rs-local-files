// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::io;
use std::pin::Pin;
use std::task::Context;
use std::task::Poll;

use qubit_io::AsyncOutput;
use qubit_operation::ResourceRef;

use super::LocalStreamCommand;
use super::LocalStreamCommitError;
use super::LocalStreamCompletion;
use super::LocalStreamRequest;
use super::LocalStreamResource;
use super::stream_state::Direction;
use super::stream_state::StreamState;

/// An exclusive asynchronous writer for a managed local file.
///
/// The filesystem operation that opens a writer supplies this handle. Staged
/// writes remain unpublished until `commit` confirms publication; append
/// writes publish as the worker accepts them. An empty append commit confirms
/// the session without recording a target publication or modification.
///
/// Polling `commit` for the first time establishes a barrier immediately and
/// starts settling writes already accepted by the worker. New writes and
/// flushes are rejected from that point. The native commit command is queued
/// only after settling succeeds. Dropping the future stops that caller's wait,
/// not the request: another `commit` resumes settling or observes the same
/// queued result. A retryable failure reopens the writer; success and
/// non-retryable failure make it terminal. `close` drives a started commit to
/// completion, even if its command was not queued yet, and retains its typed
/// result for one later `commit` call. A cached failure is non-retryable
/// because the writer has entered its terminal close path, even when the
/// original failure allowed retry while open. Once the first `close` poll
/// begins, new non-empty writes, flushes, and commits that have not already
/// started are rejected, even if that close future is later dropped. Only
/// closing a staged writer
/// before any commit starts discards the stage. Inspect publication state and
/// retryability before deciding whether to retry; `Published` and
/// `Indeterminate` do not authorize replay of the complete input.
///
/// # Examples
///
/// ```
/// use qubit_local_files::operation::stream::AsyncLocalFileWriter;
/// use qubit_local_files::operation::stream::LocalStreamCommitError;
/// use qubit_local_files::LocalWriteOutcome;
///
/// async fn commit(writer: &mut AsyncLocalFileWriter)
///     -> Result<LocalWriteOutcome, LocalStreamCommitError>
/// {
///     writer.commit().await
/// }
///
/// struct Diagnostics {
///     write: Option<std::io::Error>,
///     commit: Option<LocalStreamCommitError>,
///     close: Option<std::io::Error>,
/// }
///
/// async fn write_and_commit(
///     writer: &mut AsyncLocalFileWriter,
///     bytes: &[u8],
/// ) -> Result<LocalWriteOutcome, Diagnostics> {
///     use qubit_io::AsyncOutput;
///     let write_error = writer.write_fully_async(bytes).await.err();
///     let mut commit_result = None;
///     if write_error.is_none() {
///         let mut result = writer.commit().await;
///         if let Err(error) = &result {
///             let state = error.commit_error().state();
///             if error.retryable()
///                 && state == qubit_local_files::outcome::LocalWriteFailureState::NotPublished
///             {
///                 result = writer.commit().await;
///             }
///         }
///         commit_result = Some(result);
///     }
///     let close_error = writer.close().await.err();
///     let commit_error = commit_result.as_ref().and_then(|result| result.as_ref().err());
///     if write_error.is_some() || commit_error.is_some() || close_error.is_some() {
///         return Err(Diagnostics {
///             write: write_error,
///             commit: commit_result.and_then(Result::err),
///             close: close_error,
///         });
///     }
///     Ok(commit_result.expect("successful write is committed").expect("commit succeeded"))
/// }
/// ```
pub struct AsyncLocalFileWriter {
    /// Managed worker port retained until writer closure.
    resource: Option<ResourceRef<LocalStreamResource>>,
    /// Pending command and cancellation state for the writer.
    state: StreamState,
    /// Stable commit command retained across cancellation.
    request: Option<LocalStreamRequest>,
    /// Commit result captured by close but not yet observed by the caller.
    result: Option<Result<crate::LocalWriteOutcome, LocalStreamCommitError>>,
    /// Whether the commit barrier has started.
    started: bool,
    /// Whether the commit completed or failed terminally.
    terminal: bool,
    /// Whether polling close has begun, preventing new public work.
    close_started: bool,
}

impl AsyncLocalFileWriter {
    /// Creates a writer over a managed serial-worker resource.
    ///
    /// # Parameters
    ///
    /// * `resource` - Managed worker port with exclusive writer ownership.
    pub fn new(resource: ResourceRef<LocalStreamResource>) -> io::Result<Self> {
        resource.claim()?;
        Ok(Self {
            resource: Some(resource),
            state: StreamState::new(Direction::Write),
            request: None,
            result: None,
            started: false,
            terminal: false,
            close_started: false,
        })
    }

    /// Waits until all bytes accepted by this writer have been written.
    pub async fn settle(&mut self) -> io::Result<()> {
        let Some(resource) = self.resource.as_deref() else {
            return match self.state.completed_close() {
                Poll::Ready(result) => result,
                Poll::Pending => unreachable!("closed stream has a close result"),
            };
        };
        let state = &mut self.state;
        std::future::poll_fn(move |cx| state.poll_settle(resource, cx)).await
    }

    /// Starts or resumes the commit. On the first poll this establishes a
    /// write barrier, settles bytes already accepted by the worker, then
    /// queues the native commit command. Dropping this future only cancels the
    /// caller's wait. Call `commit` again to resume settling or observe the
    /// same queued command result while the writer remains open. Once `close`
    /// has been polled, use `close` to resume the operation; its cached result
    /// remains available to one later `commit` call. A retryable typed error
    /// leaves an open writer available for another call; inspect its typed
    /// state before retrying.
    pub async fn commit(&mut self) -> Result<crate::LocalWriteOutcome, LocalStreamCommitError> {
        if let Some(result) = self.result.take() {
            return result;
        }
        if self.close_started {
            if self.terminal {
                return self.commit_inner().await;
            }
            return Err(commit_stream_error(
                io::Error::new(io::ErrorKind::BrokenPipe, "local writer is closing"),
                crate::LocalWriteFailureState::Indeterminate,
                0,
            ));
        }
        self.commit_inner().await
    }

    /// Drives a commit already accepted before close began.
    async fn commit_inner(&mut self) -> Result<crate::LocalWriteOutcome, LocalStreamCommitError> {
        if self.terminal {
            return Err(commit_stream_error(
                io::Error::new(io::ErrorKind::BrokenPipe, "local writer commit is already terminal"),
                crate::LocalWriteFailureState::Indeterminate,
                0,
            ));
        }
        self.started = true;
        if self.request.is_none() {
            if let Err(error) = self.settle().await {
                let (state, bytes) = self
                    .resource
                    .as_deref()
                    .and_then(LocalStreamResource::write_facts)
                    .unwrap_or((crate::LocalWriteFailureState::Indeterminate, 0));
                self.terminal = true;
                return Err(commit_stream_error(error, state, bytes));
            }
            self.request = Some(LocalStreamRequest {
                id: 0x8000_0000_0000_0001,
                command: LocalStreamCommand::Commit,
            });
        }
        let Some(resource) = self.resource.as_deref() else {
            self.terminal = true;
            return Err(commit_stream_error(
                StreamState::closed_error(),
                crate::LocalWriteFailureState::Indeterminate,
                0,
            ));
        };
        let request = &mut self.request;
        let started = &mut self.started;
        let terminal = &mut self.terminal;
        std::future::poll_fn(move |cx| poll_commit(resource, request, started, terminal, cx)).await
    }

    /// Completes a started commit, then closes this writer port.
    ///
    /// If a commit was started, close keeps driving it even when it was still
    /// settling accepted writes and had not queued its native command. Its
    /// result remains available from one subsequent `commit` call, but a
    /// cached failure is non-retryable because this writer is closed. Closing a
    /// staged writer before the first commit poll discards the unpublished
    /// stage. This closes the writer port; an owning open operation must
    /// still be released before waiting for that operation's settlement. The
    /// first poll marks the writer as closing. Subsequent non-empty writes,
    /// flushes, and not-yet-started commits fail immediately, and the mark
    /// remains if this future is dropped. Errors returned by this method keep
    /// their original payload. To find a structured [`crate::LocalFileError`],
    /// inspect each `io::Error`'s direct payload with `get_ref()` before
    /// following `Error::source()`: an `io::Error` may expose the payload's
    /// source directly, skipping the payload itself.
    ///
    /// ```
    /// use std::error::Error;
    /// use std::io;
    ///
    /// fn local_file_error(error: &io::Error) -> Option<&qubit_local_files::LocalFileError> {
    ///     let mut pending: Vec<&(dyn Error + 'static)> = Vec::new();
    ///     if let Some(payload) = error.get_ref() {
    ///         pending.push(payload);
    ///     }
    ///     while let Some(cause) = pending.pop() {
    ///         if let Some(file_error) = cause.downcast_ref() {
    ///             return Some(file_error);
    ///         }
    ///         if let Some(io_error) = cause.downcast_ref::<io::Error>() {
    ///             if let Some(payload) = io_error.get_ref() {
    ///                 pending.push(payload);
    ///             }
    ///         }
    ///         if let Some(source) = cause.source() {
    ///             pending.push(source);
    ///         }
    ///     }
    ///     None
    /// }
    /// ```
    pub async fn close(&mut self) -> io::Result<()> {
        self.close_started = true;
        if self.request.is_some() || (self.started && !self.terminal) {
            self.result = Some(self.commit_inner().await.map_err(LocalStreamCommitError::without_retry));
            self.terminal = true;
        }
        let Some(resource) = self.resource.as_deref() else {
            return match self.state.completed_close() {
                Poll::Ready(result) => result,
                Poll::Pending => unreachable!("a completed close cannot be pending"),
            };
        };
        let result = {
            let state = &mut self.state;
            std::future::poll_fn(move |cx| state.poll_close(resource, cx)).await
        };
        if self.state.is_closed() {
            self.resource.take();
        }
        result
    }
}

impl AsyncOutput for AsyncLocalFileWriter {
    /// Byte unit accepted by this writer.
    type Item = u8;

    /// Reports that writes are staged by the serial worker.
    fn is_buffered(&self) -> bool {
        true
    }

    /// Polls a worker-backed write from the requested input range.
    ///
    /// The bytes are copied into an owned command before the caller may reuse
    /// its input buffer; publication remains deferred until `commit`.
    ///
    /// # Parameters
    ///
    /// * `self` - Pinned writer whose serial worker owns the native handle.
    /// * `cx` - Task context notified when the write completion is available.
    /// * `input` - Caller bytes to submit to the worker.
    /// * `index` - First input-buffer position selected for this write.
    /// * `count` - Number of bytes selected from the input buffer.
    ///
    /// # Safety
    ///
    /// The caller must provide a valid input range of `count` bytes beginning
    /// at `index`; this implementation checks the range before accessing it.
    unsafe fn poll_write_unchecked(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        input: &[u8],
        index: usize,
        count: usize,
    ) -> Poll<io::Result<usize>> {
        if count == 0 {
            return Poll::Ready(Ok(0));
        }
        let end = match index.checked_add(count) {
            Some(end) if end <= input.len() => end,
            _ => {
                return Poll::Ready(Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "write range out of bounds",
                )));
            }
        };
        let this = self.as_mut().get_mut();
        if this.resource.is_none() {
            return Poll::Ready(Err(StreamState::closed_error()));
        }
        if this.close_started {
            return Poll::Ready(Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "local writer is closing",
            )));
        }
        if this.started {
            return Poll::Ready(Err(commit_barrier_error(this.terminal)));
        }
        let resource = this.resource.as_deref().expect("resource retained until close");
        this.state.poll_write(resource, cx, &input[index..end])
    }

    /// Flushes all completed writes through the worker before returning.
    ///
    /// # Parameters
    ///
    /// * `self` - Pinned mutable writer whose pending command state is polled.
    /// * `cx` - Task context used to wake the caller when the flush completes.
    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let this = self.as_mut().get_mut();
        if this.resource.is_none() {
            return Poll::Ready(Err(StreamState::closed_error()));
        }
        if this.close_started {
            return Poll::Ready(Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "local writer is closing",
            )));
        }
        if this.started {
            return Poll::Ready(Err(commit_barrier_error(this.terminal)));
        }
        let resource = this.resource.as_deref().expect("resource retained until close");
        this.state.poll_flush(resource, cx)
    }
}

/// Polls the stable commit request and retains its terminal status.
///
/// # Parameters
///
/// * `resource` - Worker resource that owns the writer and pending request.
/// * `request` - Stable request retained across caller cancellation.
/// * `started` - Commit barrier state to update after polling.
/// * `terminal` - Terminal state to update after completion.
/// * `cx` - Task context used to register the caller for wakeup.
fn poll_commit(
    resource: &LocalStreamResource,
    request_slot: &mut Option<LocalStreamRequest>,
    started: &mut bool,
    terminal: &mut bool,
    cx: &mut Context<'_>,
) -> Poll<Result<crate::LocalWriteOutcome, LocalStreamCommitError>> {
    let Some(request) = request_slot.as_ref() else {
        return Poll::Ready(Err(commit_stream_error(
            io::Error::new(io::ErrorKind::InvalidInput, "no pending writer commit"),
            crate::LocalWriteFailureState::Indeterminate,
            0,
        )));
    };
    let result = match resource.poll_command(cx, request) {
        Poll::Pending => return Poll::Pending,
        Poll::Ready(Ok(LocalStreamCompletion::Committed(outcome))) => Ok(outcome),
        Poll::Ready(Ok(LocalStreamCompletion::CommitFailed { error, retryable })) => {
            *request_slot = None;
            *started = !retryable;
            *terminal = !retryable;
            return Poll::Ready(Err(LocalStreamCommitError::new(error, retryable)));
        }
        Poll::Ready(Ok(_)) => Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "unexpected commit completion",
        )),
        Poll::Ready(Err(error)) => Err(error),
    };
    *request_slot = None;
    *started = true;
    *terminal = true;
    Poll::Ready(result.map_err(|error| {
        let (state, bytes) = resource
            .write_facts()
            .unwrap_or((crate::LocalWriteFailureState::Indeterminate, 0));
        commit_stream_error(error, state, bytes)
    }))
}

/// Maps a worker I/O error into the public local commit error type.
///
/// # Parameters
///
/// * `error` - I/O error returned by the worker.
/// * `state` - Publication state observed after the failed command.
/// * `bytes` - Number of bytes the worker reports as modified.
fn commit_stream_error(error: io::Error, state: crate::LocalWriteFailureState, _bytes: u64) -> LocalStreamCommitError {
    LocalStreamCommitError::new(
        crate::LocalFileCommitError::new(
            crate::LocalFileError::from_io(crate::LocalFileOperation::Commit, None, None, error),
            state,
            None,
        ),
        false,
    )
}

/// Creates the I/O error used to reject operations behind a commit barrier.
///
/// # Parameters
///
/// * `terminal` - Whether the commit barrier has reached a terminal state.
fn commit_barrier_error(terminal: bool) -> io::Error {
    if terminal {
        io::Error::new(io::ErrorKind::BrokenPipe, "local writer commit is terminal")
    } else {
        io::Error::new(io::ErrorKind::WouldBlock, "local writer commit is pending")
    }
}

impl Drop for AsyncLocalFileWriter {
    /// Requests worker cleanup when the writer handle is abandoned.
    fn drop(&mut self) {
        if let Some(resource) = &self.resource {
            self.state.request_close_on_drop(resource);
        }
    }
}
