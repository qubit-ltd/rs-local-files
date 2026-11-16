// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::io;
use std::io::SeekFrom;
use std::pin::Pin;
use std::task::Context;
use std::task::Poll;

use qubit_io::AsyncInput;
use qubit_operation::ResourceRef;

use super::LocalStreamResource;
use super::stream_state::Direction;
use super::stream_state::StreamState;

/// An exclusive asynchronous reader for a managed local file.
///
/// The filesystem operation that opens a reader supplies this handle. Reads,
/// seeks, and closes share one cancellation-safe command state.
///
/// # Examples
///
/// ```
/// use qubit_local_files::operation::stream::AsyncLocalFileReader;
/// use std::io;
///
/// async fn settle(reader: &mut AsyncLocalFileReader) -> io::Result<()> {
///     reader.settle().await
/// }
/// ```
pub struct AsyncLocalFileReader {
    /// Managed worker port retained until the reader closes.
    resource: Option<ResourceRef<LocalStreamResource>>,
    /// Pending command and cancellation state for the reader.
    state: StreamState,
}

impl AsyncLocalFileReader {
    /// Creates a reader over a managed serial-worker resource.
    ///
    /// # Parameters
    ///
    /// * `resource` - Managed worker port with exclusive reader ownership.
    pub fn new(resource: ResourceRef<LocalStreamResource>) -> io::Result<Self> {
        resource.claim()?;
        Ok(Self {
            resource: Some(resource),
            state: StreamState::new(Direction::Read),
        })
    }

    /// Performs a cancellation-safe seek. A cancelled seek remains in the
    /// stream state and is settled before a later read is allowed to proceed.
    ///
    /// # Parameters
    ///
    /// * `position` - Absolute or relative target for the next read.
    pub async fn seek(&mut self, position: SeekFrom) -> io::Result<u64> {
        let owner_id = self.state.new_seek_owner_id();
        let Some(resource) = self.resource.as_deref() else {
            return Err(StreamState::closed_error());
        };
        let state = &mut self.state;
        std::future::poll_fn(move |cx| state.poll_seek(resource, cx, position, owner_id)).await
    }

    /// Waits for the stream's in-flight command and reports a latched failure.
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

    /// Closes this port without waiting for the containing operation scope.
    /// If this future is cancelled, calling `close` again observes the same
    /// close command and its eventual result.
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

impl AsyncInput for AsyncLocalFileReader {
    /// Byte unit produced by this reader.
    type Item = u8;

    /// Reports that reads use a bounded worker-owned buffer.
    fn is_buffered(&self) -> bool {
        true
    }

    unsafe fn poll_read_unchecked(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        output: &mut [u8],
        index: usize,
        count: usize,
    ) -> Poll<io::Result<usize>> {
        if count == 0 {
            return Poll::Ready(Ok(0));
        }
        let end = match index.checked_add(count) {
            Some(end) if end <= output.len() => end,
            _ => {
                return Poll::Ready(Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "read range out of bounds",
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
        this.state.poll_read(resource, cx, &mut output[index..end])
    }
}

impl Drop for AsyncLocalFileReader {
    /// Requests worker cleanup when the reader handle is abandoned.
    fn drop(&mut self) {
        if let Some(resource) = &self.resource {
            self.state.request_close_on_drop(resource);
        }
    }
}
