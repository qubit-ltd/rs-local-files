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
/// The filesystem operation that opens a writer supplies this handle. Writes
/// remain staged until `commit` confirms publication.
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
/// ```
pub struct AsyncLocalFileWriter {
    /// Managed worker port retained until writer closure.
    resource: Option<ResourceRef<LocalStreamResource>>,
    /// Pending command and cancellation state for the writer.
    state: StreamState,
    /// Stable commit command reused when the future is polled again.
    commit_request: Option<LocalStreamRequest>,
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
            commit_request: None,
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

    /// Commits the staged file and returns its publication and durability
    /// result. A retryable typed error leaves the writer available for
    /// another call.
    pub async fn commit(&mut self) -> Result<crate::LocalWriteOutcome, LocalStreamCommitError> {
        self.settle().await.map_err(|error| {
            LocalStreamCommitError::new(
                crate::LocalFileCommitError::new(
                    crate::LocalFileError::from_io(
                        crate::LocalFileOperation::Commit,
                        None,
                        None,
                        error,
                    ),
                    crate::LocalWriteFailureState::NotPublished,
                    None,
                ),
                false,
            )
        })?;
        let Some(resource) = self.resource.as_deref() else {
            return Err(LocalStreamCommitError::new(
                crate::LocalFileCommitError::new(
                    crate::LocalFileError::from_io(
                        crate::LocalFileOperation::Commit,
                        None,
                        None,
                        StreamState::closed_error(),
                    ),
                    crate::LocalWriteFailureState::Indeterminate,
                    None,
                ),
                false,
            ));
        };
        let id = self
            .commit_request
            .get_or_insert_with(|| LocalStreamRequest {
                id: 0x8000_0000_0000_0001,
                command: LocalStreamCommand::Commit,
            })
            .id;
        let request = self
            .commit_request
            .as_ref()
            .expect("commit request inserted");
        let result = std::future::poll_fn(|cx| match resource.poll_command(cx, request) {
            Poll::Pending => Poll::Pending,
            Poll::Ready(Ok(LocalStreamCompletion::Committed(outcome))) => Poll::Ready(Ok(outcome)),
            Poll::Ready(Ok(LocalStreamCompletion::CommitFailed { error, retryable })) => {
                Poll::Ready(Err(LocalStreamCommitError::new(error, retryable)))
            }
            Poll::Ready(Ok(_)) => Poll::Ready(Err(LocalStreamCommitError::new(
                crate::LocalFileCommitError::new(
                    crate::LocalFileError::from_io(
                        crate::LocalFileOperation::Commit,
                        None,
                        None,
                        io::Error::new(io::ErrorKind::InvalidData, "unexpected commit completion"),
                    ),
                    crate::LocalWriteFailureState::Indeterminate,
                    None,
                ),
                false,
            ))),
            Poll::Ready(Err(error)) => Poll::Ready(Err(LocalStreamCommitError::new(
                crate::LocalFileCommitError::new(
                    crate::LocalFileError::from_io(
                        crate::LocalFileOperation::Commit,
                        None,
                        None,
                        error,
                    ),
                    crate::LocalWriteFailureState::Indeterminate,
                    None,
                ),
                false,
            ))),
        })
        .await;
        if result.is_ok() || result.as_ref().is_err_and(|error| !error.retryable()) {
            self.commit_request = None;
        }
        let _ = id;
        result
    }

    /// Closes this port without waiting for operation settlement.
    pub async fn close(&mut self) -> io::Result<()> {
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
        let resource = this
            .resource
            .as_deref()
            .expect("resource retained until close");
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
        let resource = this
            .resource
            .as_deref()
            .expect("resource retained until close");
        this.state.poll_flush(resource, cx)
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
