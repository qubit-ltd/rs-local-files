// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::collections::VecDeque;
use std::io;
use std::io::SeekFrom;
use std::sync::Arc;
use std::task::Context;
use std::task::Poll;

use super::super::LOCAL_STREAM_BUFFER_SIZE;
use super::super::LocalStreamCommand;
use super::super::LocalStreamCompletion;
use super::super::LocalStreamRequest;
use super::super::LocalStreamResource;
use super::Direction;
use super::PendingSeek;
use super::PendingWrite;
use super::SavedError;

/// Coordinates queued reads, writes, seeks, flushes, and close completion.
/// Request ids bind worker responses to the active operation; buffered bytes
/// and sticky errors preserve stream ordering across asynchronous polls.
pub(crate) struct StreamState {
    /// Selects whether this instance enforces read-side or write-side
    /// operations.
    direction: Direction,
    /// Next request identifier, advanced with wrapping while avoiding zero.
    next_id: u64,
    /// Next identity used to associate a seek with its owner future.
    next_seek_owner_id: u64,
    /// Outstanding native read request, if one has been issued.
    read_request: Option<LocalStreamRequest>,
    /// Bytes returned by the worker but not yet copied to a caller buffer.
    read_buffer: VecDeque<u8>,
    /// Whether the worker has confirmed end-of-file with a sentinel read.
    eof: bool,
    /// FIFO of writes awaiting native completion, with per-request progress.
    writes: VecDeque<PendingWrite>,
    /// Total uncompleted bytes retained by queued write requests.
    queued_write_bytes: usize,
    /// Cumulative successfully written byte count used for seek barriers.
    written_bytes: u64,
    /// Sticky first write failure, replayed by later write-side operations.
    write_error: Option<SavedError>,
    /// Outstanding native seek and the writer identity that owns it.
    seek_request: Option<PendingSeek>,
    /// Completed seek position paired with its owner identity.
    seek_result: Option<(u64, io::Result<u64>)>,
    /// Outstanding native flush request awaiting completion.
    flush_request: Option<LocalStreamRequest>,
    /// Outstanding native close request awaiting completion.
    close_request: Option<LocalStreamRequest>,
    /// Whether the native close has completed successfully.
    closed: bool,
    /// Sticky close failure preserved for later polls.
    close_error: Option<SavedError>,
}

impl StreamState {
    /// Creates an empty state machine for one stream direction. Request
    /// counters start nonzero so worker replies can be matched after
    /// wraparound.
    ///
    /// # Parameters
    ///
    /// * `direction` — read or write mode whose request invariants apply.
    pub(in crate::operation::stream) fn new(direction: Direction) -> Self {
        Self {
            direction,
            next_id: 1,
            next_seek_owner_id: 1,
            read_request: None,
            read_buffer: VecDeque::new(),
            eof: false,
            writes: VecDeque::new(),
            queued_write_bytes: 0,
            written_bytes: 0,
            write_error: None,
            seek_request: None,
            seek_result: None,
            flush_request: None,
            close_request: None,
            closed: false,
            close_error: None,
        }
    }

    /// Assigns a fresh request id to a command before it is sent to the worker.
    ///
    /// # Parameters
    ///
    /// * `self` — state machine that owns the pending request and buffers .
    fn request(&mut self, command: LocalStreamCommand) -> LocalStreamRequest {
        let id = self.next_id;
        self.next_id = self.next_id.wrapping_add(1).max(1);
        LocalStreamRequest { id, command }
    }

    /// Operates on the stream state while preserving pending request ownership.
    ///
    /// # Parameters
    ///
    /// * `self` — state machine that owns the pending request and buffers .
    pub(in crate::operation::stream) fn new_seek_owner_id(&mut self) -> u64 {
        let id = self.next_seek_owner_id;
        self.next_seek_owner_id = self.next_seek_owner_id.wrapping_add(1).max(1);
        id
    }

    /// Creates the broken-pipe error used when a caller operates on a closed
    /// stream.
    fn error_closed() -> io::Error {
        io::Error::new(io::ErrorKind::BrokenPipe, "local stream is closed")
    }

    /// Returns the canonical error for an operation attempted after close.
    pub(in crate::operation::stream) fn closed_error() -> io::Error {
        Self::error_closed()
    }

    /// Copies buffered bytes first, then polls the native reader only when more
    /// data is needed. A sentinel read records EOF without exposing it as
    /// payload.
    ///
    /// # Parameters
    ///
    /// * `self` — state machine retaining buffered read and error state.
    /// * `resource` — worker resource used to request more native bytes.
    /// * `cx` — task context used while waiting for the worker.
    /// * `output` — destination for the next available stream bytes.
    pub(in crate::operation::stream) fn poll_read(
        &mut self,
        resource: &LocalStreamResource,
        cx: &mut Context<'_>,
        output: &mut [u8],
    ) -> Poll<io::Result<usize>> {
        if self.direction != Direction::Read {
            return Poll::Ready(Err(io::Error::new(io::ErrorKind::InvalidInput, "not a reader")));
        }
        if self.closed || self.close_request.is_some() {
            return Poll::Ready(Err(Self::error_closed()));
        }
        if let Some(error) = &self.write_error {
            return Poll::Ready(Err(error.make()));
        }
        if self.poll_seek_barrier(resource, cx).is_pending() {
            return Poll::Pending;
        }
        if let Some((_, Err(error))) = self.seek_result.take() {
            return Poll::Ready(Err(error));
        }
        if !self.read_buffer.is_empty() {
            let count = output.len().min(self.read_buffer.len());
            for slot in &mut output[..count] {
                *slot = self.read_buffer.pop_front().expect("count bounded by buffer length");
            }
            return Poll::Ready(Ok(count));
        }
        if self.eof {
            return Poll::Ready(Ok(0));
        }
        if self.read_request.is_none() {
            self.read_request = Some(self.request(LocalStreamCommand::Read {
                max_bytes: LOCAL_STREAM_BUFFER_SIZE,
            }));
        }
        let request = self.read_request.as_ref().expect("request inserted");
        match resource.poll_command(cx, request) {
            Poll::Pending => Poll::Pending,
            Poll::Ready(result) => {
                self.read_request = None;
                match result {
                    Ok(LocalStreamCompletion::Read(bytes)) => {
                        if bytes.len() > LOCAL_STREAM_BUFFER_SIZE {
                            let error =
                                io::Error::new(io::ErrorKind::InvalidData, "stream worker exceeded read buffer limit");
                            self.write_error =
                                Some(SavedError::capture(io::Error::new(error.kind(), error.to_string())));
                            return Poll::Ready(Err(error));
                        }
                        self.eof = bytes.is_empty();
                        self.read_buffer.extend(bytes);
                        if self.read_buffer.is_empty() {
                            Poll::Ready(Ok(0))
                        } else {
                            self.poll_read(resource, cx, output)
                        }
                    }
                    Ok(LocalStreamCompletion::Unconfirmed { operation, detail }) => {
                        let error = io::Error::other(format!("Unconfirmed {operation}: {detail}"));
                        self.write_error = Some(SavedError::capture(io::Error::new(error.kind(), error.to_string())));
                        Poll::Ready(Err(error))
                    }
                    Ok(_) => Poll::Ready(Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "stream worker returned an unexpected read completion",
                    ))),
                    Err(error) => {
                        self.write_error = Some(SavedError::capture(io::Error::new(error.kind(), error.to_string())));
                        Poll::Ready(Err(error))
                    }
                }
            }
        }
    }

    /// Queues the caller bytes and drives earlier writes in FIFO order,
    /// reporting only bytes accepted by the native worker.
    ///
    /// # Parameters
    ///
    /// * `self` — state machine maintaining write order and sticky errors.
    /// * `resource` — worker resource receiving the queued write.
    /// * `cx` — task context used while waiting for native progress.
    /// * `input` — bytes to append after any earlier queued write.
    pub(in crate::operation::stream) fn poll_write(
        &mut self,
        resource: &LocalStreamResource,
        cx: &mut Context<'_>,
        input: &[u8],
    ) -> Poll<io::Result<usize>> {
        if self.direction != Direction::Write {
            return Poll::Ready(Err(io::Error::new(io::ErrorKind::InvalidInput, "not a writer")));
        }
        if self.closed || self.close_request.is_some() {
            return Poll::Ready(Err(Self::error_closed()));
        }
        if let Some(error) = &self.write_error {
            return Poll::Ready(Err(error.make()));
        }
        if self.poll_pending_flush(resource, cx).is_pending() {
            return Poll::Pending;
        }
        if let Some(error) = &self.write_error {
            return Poll::Ready(Err(error.make()));
        }
        if self.poll_seek_barrier(resource, cx).is_pending() {
            return Poll::Pending;
        }
        if let Some((_, Err(error))) = self.seek_result.take() {
            return Poll::Ready(Err(error));
        }
        // A queued command may still be running; newly accepted input is
        // independent because it is atomically moved into owned storage.
        let _ = self.poll_writes(resource, cx);
        if let Some(error) = &self.write_error {
            return Poll::Ready(Err(error.make()));
        }
        let available = LOCAL_STREAM_BUFFER_SIZE.saturating_sub(self.queued_write_bytes);
        if available == 0 {
            return Poll::Pending;
        }
        let accepted = input.len().min(available);
        if accepted == 0 {
            return Poll::Ready(Ok(0));
        }
        let bytes: Arc<[u8]> = Arc::from(&input[..accepted]);
        let request = self.request(LocalStreamCommand::Write { bytes });
        self.queued_write_bytes += accepted;
        self.writes.push_back(PendingWrite { request, offset: 0 });
        Poll::Ready(Ok(accepted))
    }

    /// Advances the front queued write and stores any failure so subsequent
    /// operations observe the same error.
    ///
    /// # Parameters
    ///
    /// * `self` — state machine that owns the pending request and buffers .
    fn poll_writes(&mut self, resource: &LocalStreamResource, cx: &mut Context<'_>) -> Poll<()> {
        loop {
            let Some(pending) = self.writes.front() else {
                return Poll::Ready(());
            };
            let request = pending.request.clone();
            let offset = pending.offset;
            let expected = match &request.command {
                LocalStreamCommand::Write { bytes } => bytes.len(),
                _ => unreachable!("write queue only contains write commands"),
            };
            match resource.poll_command(cx, &request) {
                Poll::Pending => return Poll::Pending,
                Poll::Ready(Err(error)) => {
                    let actual = self.written_bytes;
                    self.write_error = Some(SavedError::capture(io::Error::new(
                        error.kind(),
                        format!("{} ({} bytes written before failure)", error, actual),
                    )));
                    self.writes.clear();
                    self.queued_write_bytes = 0;
                    return Poll::Ready(());
                }
                Poll::Ready(Ok(LocalStreamCompletion::Written(count))) => {
                    let remaining = expected - offset;
                    if count == 0 || count > remaining {
                        self.write_error = Some(SavedError::capture(io::Error::new(
                            io::ErrorKind::WriteZero,
                            format!("invalid partial write result after {} bytes", self.written_bytes),
                        )));
                        self.writes.clear();
                        self.queued_write_bytes = 0;
                        return Poll::Ready(());
                    }
                    self.written_bytes = self.written_bytes.saturating_add(count as u64);
                    self.queued_write_bytes = self.queued_write_bytes.saturating_sub(count);
                    if offset + count == expected {
                        self.writes.pop_front();
                    } else {
                        let rest = match &request.command {
                            LocalStreamCommand::Write { bytes } => Arc::from(&bytes[offset + count..]),
                            _ => unreachable!(),
                        };
                        let next = self.request(LocalStreamCommand::Write { bytes: rest });
                        let pending = self.writes.front_mut().expect("partial write remains queued");
                        pending.offset = 0;
                        pending.request = next;
                    }
                }
                Poll::Ready(Ok(LocalStreamCompletion::Unconfirmed { operation, detail })) => {
                    self.write_error = Some(SavedError::capture(io::Error::other(format!(
                        "Unconfirmed {operation} after {} bytes written: {detail}",
                        self.written_bytes
                    ))));
                    self.writes.clear();
                    self.queued_write_bytes = 0;
                    return Poll::Ready(());
                }
                Poll::Ready(Ok(_)) => {
                    self.write_error = Some(SavedError::capture(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "stream worker returned an unexpected write completion",
                    )));
                    self.writes.clear();
                    self.queued_write_bytes = 0;
                    return Poll::Ready(());
                }
            }
        }
    }

    /// Waits until all queued writes finish before issuing the native flush
    /// barrier.
    ///
    /// # Parameters
    ///
    /// * `self` — state machine whose writes must settle before flush.
    /// * `resource` — worker resource receiving the flush command.
    /// * `cx` — task context used while waiting for writes and flush.
    pub(in crate::operation::stream) fn poll_flush(
        &mut self,
        resource: &LocalStreamResource,
        cx: &mut Context<'_>,
    ) -> Poll<io::Result<()>> {
        if self.closed || self.close_request.is_some() {
            return Poll::Ready(Err(Self::error_closed()));
        }
        if let Some(error) = &self.write_error {
            return Poll::Ready(Err(error.make()));
        }
        if self.poll_writes(resource, cx).is_pending() {
            return Poll::Pending;
        }
        if let Some(error) = &self.write_error {
            return Poll::Ready(Err(error.make()));
        }
        if self.flush_request.is_none() {
            self.flush_request = Some(self.request(LocalStreamCommand::Flush));
        }
        self.poll_pending_flush(resource, cx)
    }

    /// Collects an already-issued flush request without issuing duplicate
    /// worker commands.
    ///
    /// # Parameters
    ///
    /// * `resource` — worker resource that performs the native operation .
    /// * `cx` — task context used to register a wakeup .
    fn poll_pending_flush(&mut self, resource: &LocalStreamResource, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let Some(request) = self.flush_request.as_ref() else {
            return Poll::Ready(Ok(()));
        };
        match resource.poll_command(cx, request) {
            Poll::Pending => Poll::Pending,
            Poll::Ready(Ok(LocalStreamCompletion::Flushed)) => {
                self.flush_request = None;
                Poll::Ready(Ok(()))
            }
            Poll::Ready(Ok(LocalStreamCompletion::Unconfirmed { operation, detail })) => {
                self.flush_request = None;
                let error = io::Error::other(format!(
                    "Unconfirmed {operation} after {} bytes written: {detail}",
                    self.written_bytes
                ));
                self.write_error = Some(SavedError::capture(io::Error::new(error.kind(), error.to_string())));
                Poll::Ready(Err(error))
            }
            Poll::Ready(Ok(_)) => {
                self.flush_request = None;
                Poll::Ready(Err(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "unexpected flush completion",
                )))
            }
            Poll::Ready(Err(error)) => {
                self.flush_request = None;
                self.write_error = Some(SavedError::capture(io::Error::new(
                    error.kind(),
                    format!("{} ({} bytes written before failure)", error, self.written_bytes),
                )));
                Poll::Ready(Err(self.write_error.as_ref().expect("error stored").make()))
            }
        }
    }

    /// Settles pending writes before issuing a seek and associates completion
    /// with the requesting writer.
    ///
    /// # Parameters
    ///
    /// * `self` — state machine retaining any write and seek state.
    /// * `resource` — worker resource receiving the seek request.
    /// * `cx` — task context used while waiting for the seek barrier.
    /// * `position` — requested position and origin.
    /// * `owner_id` — writer identity used to validate the seek result.
    pub(in crate::operation::stream) fn poll_seek(
        &mut self,
        resource: &LocalStreamResource,
        cx: &mut Context<'_>,
        position: SeekFrom,
        owner_id: u64,
    ) -> Poll<io::Result<u64>> {
        if self.direction != Direction::Read {
            return Poll::Ready(Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "seek is supported only by readers",
            )));
        }
        if let Some(error) = &self.write_error {
            return Poll::Ready(Err(error.make()));
        }
        if self.poll_seek_barrier(resource, cx).is_pending() {
            return Poll::Pending;
        }
        if let Some((result_owner, result)) = self.seek_result.take() {
            match result {
                Err(error) => return Poll::Ready(Err(error)),
                Ok(position) if result_owner == owner_id => {
                    return Poll::Ready(Ok(position));
                }
                Ok(_) => {}
            }
        }
        if self.poll_pending_read(resource, cx).is_pending() {
            return Poll::Pending;
        }
        if let Some(error) = &self.write_error {
            return Poll::Ready(Err(error.make()));
        }
        if self.seek_request.is_none() {
            let adjusted = match position {
                SeekFrom::Current(offset) => {
                    let buffered = i64::try_from(self.read_buffer.len()).unwrap_or(i64::MAX);
                    match offset.checked_sub(buffered) {
                        Some(value) => SeekFrom::Current(value),
                        None => {
                            return Poll::Ready(Err(io::Error::new(
                                io::ErrorKind::InvalidInput,
                                "seek offset overflow",
                            )));
                        }
                    }
                }
                other => other,
            };
            self.seek_request = Some(PendingSeek {
                owner_id,
                request: self.request(LocalStreamCommand::Seek(adjusted)),
            });
        }
        if self.poll_seek_barrier(resource, cx).is_pending() {
            return Poll::Pending;
        }
        match self.seek_result.take() {
            Some((result_owner, result)) if result_owner == owner_id => Poll::Ready(result),
            Some((_, Err(error))) => Poll::Ready(Err(error)),
            Some((_, Ok(_))) => Poll::Ready(Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "another seek completed while the current seek was pending",
            ))),
            None => Poll::Ready(Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "seek completed without a result",
            ))),
        }
    }

    /// Waits for the seek result owned by the current writer before proceeding.
    ///
    /// # Parameters
    ///
    /// * `resource` — worker resource that performs the native operation .
    /// * `cx` — task context used to register a wakeup .
    fn poll_seek_barrier(&mut self, resource: &LocalStreamResource, cx: &mut Context<'_>) -> Poll<()> {
        let Some(pending) = self.seek_request.as_ref() else {
            return Poll::Ready(());
        };
        let owner_id = pending.owner_id;
        let request = pending.request.clone();
        match resource.poll_command(cx, &request) {
            Poll::Pending => Poll::Pending,
            Poll::Ready(Ok(LocalStreamCompletion::Seeked(position))) => {
                self.seek_request = None;
                self.read_buffer.clear();
                self.eof = false;
                self.seek_result = Some((owner_id, Ok(position)));
                Poll::Ready(())
            }
            Poll::Ready(Ok(LocalStreamCompletion::Unconfirmed { operation, detail })) => {
                self.seek_request = None;
                let error = io::Error::other(format!("Unconfirmed {operation}: {detail}"));
                self.write_error = Some(SavedError::capture(io::Error::new(error.kind(), error.to_string())));
                self.seek_result = Some((owner_id, Err(error)));
                Poll::Ready(())
            }
            Poll::Ready(Ok(_)) => {
                self.seek_request = None;
                let error = io::Error::other("stream position unknown after unexpected seek completion");
                self.write_error = Some(SavedError::capture(io::Error::new(error.kind(), error.to_string())));
                self.seek_result = Some((owner_id, Err(error)));
                Poll::Ready(())
            }
            Poll::Ready(Err(error)) => {
                self.seek_request = None;
                let unknown = io::Error::other(format!("stream position unknown after failed seek: {error}"));
                self.write_error = Some(SavedError::capture(io::Error::new(unknown.kind(), unknown.to_string())));
                self.seek_result = Some((owner_id, Err(unknown)));
                Poll::Ready(())
            }
        }
    }

    /// Collects an outstanding read request and appends its bytes to the
    /// internal read buffer.
    ///
    /// # Parameters
    ///
    /// * `resource` — worker resource that performs the native operation .
    /// * `cx` — task context used to register a wakeup .
    fn poll_pending_read(&mut self, resource: &LocalStreamResource, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let Some(request) = self.read_request.as_ref().cloned() else {
            return Poll::Ready(Ok(()));
        };
        match resource.poll_command(cx, &request) {
            Poll::Pending => Poll::Pending,
            Poll::Ready(Ok(LocalStreamCompletion::Read(bytes))) => {
                self.read_request = None;
                if bytes.len() > LOCAL_STREAM_BUFFER_SIZE {
                    let error = io::Error::new(io::ErrorKind::InvalidData, "stream worker exceeded read buffer limit");
                    self.write_error = Some(SavedError::capture(io::Error::new(error.kind(), error.to_string())));
                    return Poll::Ready(Err(error));
                }
                self.eof = bytes.is_empty();
                self.read_buffer.extend(bytes);
                Poll::Ready(Ok(()))
            }
            Poll::Ready(Ok(LocalStreamCompletion::Unconfirmed { operation, detail })) => {
                self.read_request = None;
                let error = io::Error::other(format!("Unconfirmed {operation}: {detail}"));
                self.write_error = Some(SavedError::capture(io::Error::new(error.kind(), error.to_string())));
                Poll::Ready(Err(error))
            }
            Poll::Ready(Ok(_)) => {
                self.read_request = None;
                let error = io::Error::new(
                    io::ErrorKind::InvalidData,
                    "stream worker returned an unexpected read completion",
                );
                self.write_error = Some(SavedError::capture(io::Error::new(error.kind(), error.to_string())));
                Poll::Ready(Err(error))
            }
            Poll::Ready(Err(error)) => {
                self.read_request = None;
                self.write_error = Some(SavedError::capture(io::Error::new(error.kind(), error.to_string())));
                Poll::Ready(Err(error))
            }
        }
    }

    /// Drives outstanding work to a stable state before the stream is closed or
    /// dropped.
    ///
    /// # Parameters
    ///
    /// * `self` — state machine containing all pending stream operations.
    /// * `resource` — worker resource needed to settle pending commands.
    /// * `cx` — task context used while waiting for pending work.
    pub(in crate::operation::stream) fn poll_settle(
        &mut self,
        resource: &LocalStreamResource,
        cx: &mut Context<'_>,
    ) -> Poll<io::Result<()>> {
        if self.close_request.is_some() {
            return self.poll_close(resource, cx);
        }
        if let Some(error) = &self.write_error {
            return Poll::Ready(Err(error.make()));
        }
        if self.poll_seek_barrier(resource, cx).is_pending() {
            return Poll::Pending;
        }
        if let Some((_, Err(error))) = self.seek_result.take() {
            return Poll::Ready(Err(error));
        }
        if self.direction == Direction::Read {
            match self.poll_pending_read(resource, cx) {
                Poll::Pending => return Poll::Pending,
                Poll::Ready(Err(error)) => return Poll::Ready(Err(error)),
                Poll::Ready(Ok(())) => {}
            }
        } else {
            if self.poll_writes(resource, cx).is_pending() {
                return Poll::Pending;
            }
            if self.poll_pending_flush(resource, cx).is_pending() {
                return Poll::Pending;
            }
            if let Some(error) = &self.write_error {
                return Poll::Ready(Err(error.make()));
            }
        }
        Poll::Ready(Ok(()))
    }

    /// Requests native closure once and preserves its result for repeated
    /// polling.
    ///
    /// # Parameters
    ///
    /// * `self` — state machine retaining the close request and result.
    /// * `resource` — worker resource that owns native close.
    /// * `cx` — task context used while waiting for close completion.
    pub(in crate::operation::stream) fn poll_close(
        &mut self,
        resource: &LocalStreamResource,
        cx: &mut Context<'_>,
    ) -> Poll<io::Result<()>> {
        if self.closed {
            return self.completed_close();
        }
        if self.poll_seek_barrier(resource, cx).is_pending() {
            return Poll::Pending;
        }
        if let Some((_, Err(error))) = self.seek_result.take() {
            self.write_error = Some(SavedError::capture(error));
        }
        if self.direction == Direction::Read {
            match self.poll_pending_read(resource, cx) {
                Poll::Pending => return Poll::Pending,
                Poll::Ready(Err(_)) | Poll::Ready(Ok(())) => {}
            }
        } else {
            if self.poll_writes(resource, cx).is_pending() {
                return Poll::Pending;
            }
            if self.poll_pending_flush(resource, cx).is_pending() {
                return Poll::Pending;
            }
        }
        if self.close_request.is_none() {
            self.close_request = Some(self.request(LocalStreamCommand::Close));
        }
        let request = self.close_request.as_ref().expect("close request inserted");
        match resource.poll_command(cx, request) {
            Poll::Pending => Poll::Pending,
            Poll::Ready(Ok(LocalStreamCompletion::Closed)) => {
                self.close_request = None;
                self.closed = true;
                if let Some(error) = &self.write_error {
                    self.close_error = Some(error.clone());
                }
                self.completed_close()
            }
            Poll::Ready(Ok(LocalStreamCompletion::Unconfirmed { operation, detail })) => {
                self.close_request = None;
                self.closed = true;
                self.close_error = Some(SavedError::capture(io::Error::other(format!(
                    "Unconfirmed {operation}: {detail}"
                ))));
                self.completed_close()
            }
            Poll::Ready(Ok(_)) => {
                self.close_request = None;
                self.closed = true;
                self.close_error = Some(SavedError::capture(io::Error::new(
                    io::ErrorKind::InvalidData,
                    "unexpected close completion",
                )));
                self.completed_close()
            }
            Poll::Ready(Err(error)) => {
                self.close_request = None;
                self.closed = true;
                let error = if self.direction == Direction::Write
                    && let Some(write_error) = &self.write_error
                {
                    io::Error::new(
                        error.kind(),
                        format!("{error}; earlier write failed: {}", write_error.make()),
                    )
                } else {
                    error
                };
                self.close_error = Some(SavedError::capture(error));
                self.completed_close()
            }
        }
    }

    /// Returns the retained close result after the worker has completed the
    /// close request.
    ///
    /// # Parameters
    ///
    /// * `self` — state machine that owns the pending request and buffers .
    pub(in crate::operation::stream) fn completed_close(&self) -> Poll<io::Result<()>> {
        self.close_error
            .as_ref()
            .map_or(Poll::Ready(Ok(())), |error| Poll::Ready(Err(error.make())))
    }

    /// Requests worker-side close when the owning stream wrapper is dropped.
    ///
    /// # Parameters
    ///
    /// * `self` — state machine that owns the pending request and buffers .
    pub(in crate::operation::stream) fn request_close_on_drop(&mut self, resource: &LocalStreamResource) {
        if self.closed {
            return;
        }
        let request_id = if let Some(request) = &self.close_request {
            request.id
        } else {
            let request = self.request(LocalStreamCommand::Close);
            let id = request.id;
            self.close_request = Some(request);
            id
        };
        resource.request_close(request_id);
    }

    /// Reports whether the native resource has completed close successfully.
    ///
    /// # Parameters
    ///
    /// * `self` — state machine that owns the pending request and buffers .
    pub(in crate::operation::stream) fn is_closed(&self) -> bool {
        self.closed
    }
}

#[cfg(test)]
mod tests {
    use std::io;

    use super::StreamState;

    #[test]
    fn test_closed_stream_errors_are_broken_pipe() {
        let errors = [StreamState::error_closed(), StreamState::closed_error()];

        for error in errors {
            assert_eq!(error.kind(), io::ErrorKind::BrokenPipe);
            assert_eq!(error.to_string(), "local stream is closed");
        }
    }
}
