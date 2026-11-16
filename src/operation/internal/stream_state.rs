use std::collections::VecDeque;
use std::io;
use std::io::SeekFrom;
use std::path::PathBuf;
use std::sync::Arc;
use std::task::{Context, Poll};

use super::{
    LOCAL_STREAM_BUFFER_SIZE, LOCAL_WALK_BATCH_BYTES, LOCAL_WALK_BATCH_ENTRIES, LocalStreamCommand,
    LocalStreamCompletion, LocalStreamRequest, LocalStreamResource, LocalWalkResource,
};

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Direction {
    Read,
    Write,
}

struct PendingWrite {
    request: LocalStreamRequest,
    offset: usize,
}

struct PendingSeek {
    owner_id: u64,
    request: LocalStreamRequest,
}

#[derive(Clone)]
struct SavedError {
    kind: io::ErrorKind,
    message: String,
}

impl SavedError {
    fn capture(error: io::Error) -> Self {
        Self {
            kind: error.kind(),
            message: error.to_string(),
        }
    }

    fn make(&self) -> io::Error {
        io::Error::new(self.kind, self.message.clone())
    }
}

pub(super) struct StreamState {
    direction: Direction,
    next_id: u64,
    next_seek_owner_id: u64,
    read_request: Option<LocalStreamRequest>,
    read_buffer: VecDeque<u8>,
    eof: bool,
    writes: VecDeque<PendingWrite>,
    queued_write_bytes: usize,
    written_bytes: u64,
    write_error: Option<SavedError>,
    seek_request: Option<PendingSeek>,
    seek_result: Option<(u64, io::Result<u64>)>,
    flush_request: Option<LocalStreamRequest>,
    close_request: Option<LocalStreamRequest>,
    closed: bool,
    close_error: Option<SavedError>,
}

#[derive(Default)]
pub(super) struct WalkState {
    next_request_id: u64,
    pending_request: Option<u64>,
    stop_requested: bool,
    closed: bool,
    close_error: Option<SavedError>,
}

impl WalkState {
    pub(super) fn poll_batch(
        &mut self,
        resource: &LocalWalkResource,
        cx: &mut Context<'_>,
    ) -> Poll<io::Result<Option<Vec<PathBuf>>>> {
        if self.stop_requested {
            return match self.poll_close(resource, cx) {
                Poll::Pending => Poll::Pending,
                Poll::Ready(Err(error)) => Poll::Ready(Err(error)),
                Poll::Ready(Ok(())) => Poll::Ready(Ok(None)),
            };
        }
        let request_id = *self.pending_request.get_or_insert_with(|| {
            self.next_request_id = self.next_request_id.wrapping_add(1).max(1);
            self.next_request_id
        });
        match resource.poll_batch(cx, request_id) {
            Poll::Pending => Poll::Pending,
            Poll::Ready(Err(error)) => {
                self.pending_request = None;
                Poll::Ready(Err(error))
            }
            Poll::Ready(Ok(None)) => {
                self.pending_request = None;
                self.request_stop(resource);
                match self.poll_close(resource, cx) {
                    Poll::Pending => Poll::Pending,
                    Poll::Ready(Err(error)) => Poll::Ready(Err(error)),
                    Poll::Ready(Ok(())) => Poll::Ready(Ok(None)),
                }
            }
            Poll::Ready(Ok(Some(entries))) => {
                self.pending_request = None;
                let mut total = 0usize;
                for entry in &entries {
                    let bytes = entry.as_os_str().as_encoded_bytes().len();
                    if bytes > LOCAL_WALK_BATCH_BYTES {
                        self.request_stop(resource);
                        return Poll::Ready(Err(resource_limit(
                            "single walker path exceeds 256 KiB batch limit",
                        )));
                    }
                    total = match total.checked_add(bytes) {
                        Some(total) if total <= LOCAL_WALK_BATCH_BYTES => total,
                        _ => {
                            self.request_stop(resource);
                            return Poll::Ready(Err(resource_limit(
                                "walker batch exceeds 256 KiB path budget",
                            )));
                        }
                    };
                }
                if entries.len() > LOCAL_WALK_BATCH_ENTRIES {
                    self.request_stop(resource);
                    return Poll::Ready(Err(resource_limit(
                        "walker batch exceeds 128 entry limit",
                    )));
                }
                Poll::Ready(Ok(Some(entries)))
            }
        }
    }

    pub(super) fn request_stop(&mut self, resource: &LocalWalkResource) {
        if !self.stop_requested {
            self.stop_requested = true;
            resource.request_stop();
        }
    }

    pub(super) fn poll_close(
        &mut self,
        resource: &LocalWalkResource,
        cx: &mut Context<'_>,
    ) -> Poll<io::Result<()>> {
        if self.closed {
            return self.poll_closed_without_resource();
        }
        self.request_stop(resource);
        match resource.poll_closed(cx) {
            Poll::Pending => Poll::Pending,
            Poll::Ready(result) => {
                self.closed = result.is_ok();
                if let Err(error) = result {
                    self.close_error = Some(SavedError::capture(error));
                }
                self.poll_closed_without_resource()
            }
        }
    }

    pub(super) fn poll_closed_without_resource(&self) -> Poll<io::Result<()>> {
        if let Some(error) = &self.close_error {
            Poll::Ready(Err(error.make()))
        } else if self.closed {
            Poll::Ready(Ok(()))
        } else {
            Poll::Pending
        }
    }

    pub(super) fn is_closed(&self) -> bool {
        self.closed
    }
}

#[derive(Debug)]
pub(super) struct WalkerResourceLimit(&'static str);

impl std::fmt::Display for WalkerResourceLimit {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.0)
    }
}

impl std::error::Error for WalkerResourceLimit {}

fn resource_limit(message: &'static str) -> io::Error {
    io::Error::new(io::ErrorKind::OutOfMemory, WalkerResourceLimit(message))
}

impl StreamState {
    pub(super) fn new(direction: Direction) -> Self {
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

    fn request(&mut self, command: LocalStreamCommand) -> LocalStreamRequest {
        let id = self.next_id;
        self.next_id = self.next_id.wrapping_add(1).max(1);
        LocalStreamRequest { id, command }
    }

    pub(super) fn new_seek_owner_id(&mut self) -> u64 {
        let id = self.next_seek_owner_id;
        self.next_seek_owner_id = self.next_seek_owner_id.wrapping_add(1).max(1);
        id
    }

    fn error_closed() -> io::Error {
        io::Error::new(io::ErrorKind::BrokenPipe, "local stream is closed")
    }

    pub(super) fn closed_error() -> io::Error {
        Self::error_closed()
    }

    pub(super) fn poll_read(
        &mut self,
        resource: &LocalStreamResource,
        cx: &mut Context<'_>,
        output: &mut [u8],
    ) -> Poll<io::Result<usize>> {
        if self.direction != Direction::Read {
            return Poll::Ready(Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "not a reader",
            )));
        }
        if self.closed || self.close_request.is_some() {
            return Poll::Ready(Err(Self::error_closed()));
        }
        if let Some(error) = &self.write_error {
            return Poll::Ready(Err(error.make()));
        }
        if let Poll::Pending = self.poll_seek_barrier(resource, cx) {
            return Poll::Pending;
        }
        if let Some((_, result)) = self.seek_result.take() {
            if let Err(error) = result {
                return Poll::Ready(Err(error));
            }
        }
        if !self.read_buffer.is_empty() {
            let count = output.len().min(self.read_buffer.len());
            for slot in &mut output[..count] {
                *slot = self
                    .read_buffer
                    .pop_front()
                    .expect("count bounded by buffer length");
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
                            let error = io::Error::new(
                                io::ErrorKind::InvalidData,
                                "stream worker exceeded read buffer limit",
                            );
                            self.write_error = Some(SavedError::capture(io::Error::new(
                                error.kind(),
                                error.to_string(),
                            )));
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
                        self.write_error = Some(SavedError::capture(io::Error::new(
                            error.kind(),
                            error.to_string(),
                        )));
                        Poll::Ready(Err(error))
                    }
                    Ok(_) => Poll::Ready(Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "stream worker returned an unexpected read completion",
                    ))),
                    Err(error) => {
                        self.write_error = Some(SavedError::capture(io::Error::new(
                            error.kind(),
                            error.to_string(),
                        )));
                        Poll::Ready(Err(error))
                    }
                }
            }
        }
    }

    pub(super) fn poll_write(
        &mut self,
        resource: &LocalStreamResource,
        cx: &mut Context<'_>,
        input: &[u8],
    ) -> Poll<io::Result<usize>> {
        if self.direction != Direction::Write {
            return Poll::Ready(Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "not a writer",
            )));
        }
        if self.closed || self.close_request.is_some() {
            return Poll::Ready(Err(Self::error_closed()));
        }
        if let Some(error) = &self.write_error {
            return Poll::Ready(Err(error.make()));
        }
        if let Poll::Pending = self.poll_pending_flush(resource, cx) {
            return Poll::Pending;
        }
        if let Some(error) = &self.write_error {
            return Poll::Ready(Err(error.make()));
        }
        if let Poll::Pending = self.poll_seek_barrier(resource, cx) {
            return Poll::Pending;
        }
        if let Some((_, Err(error))) = self.seek_result.take() {
            return Poll::Ready(Err(error));
        }
        if let Poll::Pending = self.poll_writes(resource, cx) {
            // A queued command may still be running; newly accepted input is
            // independent because it is atomically moved into owned storage.
        }
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
                            format!(
                                "invalid partial write result after {} bytes",
                                self.written_bytes
                            ),
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
                            LocalStreamCommand::Write { bytes } => {
                                Arc::from(&bytes[offset + count..])
                            }
                            _ => unreachable!(),
                        };
                        let next = self.request(LocalStreamCommand::Write { bytes: rest });
                        let pending = self
                            .writes
                            .front_mut()
                            .expect("partial write remains queued");
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

    pub(super) fn poll_flush(
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
        if let Poll::Pending = self.poll_writes(resource, cx) {
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

    fn poll_pending_flush(
        &mut self,
        resource: &LocalStreamResource,
        cx: &mut Context<'_>,
    ) -> Poll<io::Result<()>> {
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
                self.write_error = Some(SavedError::capture(io::Error::new(
                    error.kind(),
                    error.to_string(),
                )));
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
                    format!(
                        "{} ({} bytes written before failure)",
                        error, self.written_bytes
                    ),
                )));
                Poll::Ready(Err(self.write_error.as_ref().expect("error stored").make()))
            }
        }
    }

    pub(super) fn poll_seek(
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
        if let Poll::Pending = self.poll_seek_barrier(resource, cx) {
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
        if let Poll::Pending = self.poll_pending_read(resource, cx) {
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
        if let Poll::Pending = self.poll_seek_barrier(resource, cx) {
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

    fn poll_seek_barrier(
        &mut self,
        resource: &LocalStreamResource,
        cx: &mut Context<'_>,
    ) -> Poll<()> {
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
                self.write_error = Some(SavedError::capture(io::Error::new(
                    error.kind(),
                    error.to_string(),
                )));
                self.seek_result = Some((owner_id, Err(error)));
                Poll::Ready(())
            }
            Poll::Ready(Ok(_)) => {
                self.seek_request = None;
                let error =
                    io::Error::other("stream position unknown after unexpected seek completion");
                self.write_error = Some(SavedError::capture(io::Error::new(
                    error.kind(),
                    error.to_string(),
                )));
                self.seek_result = Some((owner_id, Err(error)));
                Poll::Ready(())
            }
            Poll::Ready(Err(error)) => {
                self.seek_request = None;
                let unknown = io::Error::other(format!(
                    "stream position unknown after failed seek: {error}"
                ));
                self.write_error = Some(SavedError::capture(io::Error::new(
                    unknown.kind(),
                    unknown.to_string(),
                )));
                self.seek_result = Some((owner_id, Err(unknown)));
                Poll::Ready(())
            }
        }
    }

    fn poll_pending_read(
        &mut self,
        resource: &LocalStreamResource,
        cx: &mut Context<'_>,
    ) -> Poll<io::Result<()>> {
        let Some(request) = self.read_request.as_ref().cloned() else {
            return Poll::Ready(Ok(()));
        };
        match resource.poll_command(cx, &request) {
            Poll::Pending => Poll::Pending,
            Poll::Ready(Ok(LocalStreamCompletion::Read(bytes))) => {
                self.read_request = None;
                if bytes.len() > LOCAL_STREAM_BUFFER_SIZE {
                    let error = io::Error::new(
                        io::ErrorKind::InvalidData,
                        "stream worker exceeded read buffer limit",
                    );
                    self.write_error = Some(SavedError::capture(io::Error::new(
                        error.kind(),
                        error.to_string(),
                    )));
                    return Poll::Ready(Err(error));
                }
                self.eof = bytes.is_empty();
                self.read_buffer.extend(bytes);
                Poll::Ready(Ok(()))
            }
            Poll::Ready(Ok(LocalStreamCompletion::Unconfirmed { operation, detail })) => {
                self.read_request = None;
                let error = io::Error::other(format!("Unconfirmed {operation}: {detail}"));
                self.write_error = Some(SavedError::capture(io::Error::new(
                    error.kind(),
                    error.to_string(),
                )));
                Poll::Ready(Err(error))
            }
            Poll::Ready(Ok(_)) => {
                self.read_request = None;
                let error = io::Error::new(
                    io::ErrorKind::InvalidData,
                    "stream worker returned an unexpected read completion",
                );
                self.write_error = Some(SavedError::capture(io::Error::new(
                    error.kind(),
                    error.to_string(),
                )));
                Poll::Ready(Err(error))
            }
            Poll::Ready(Err(error)) => {
                self.read_request = None;
                self.write_error = Some(SavedError::capture(io::Error::new(
                    error.kind(),
                    error.to_string(),
                )));
                Poll::Ready(Err(error))
            }
        }
    }

    pub(super) fn poll_settle(
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
        if let Poll::Pending = self.poll_seek_barrier(resource, cx) {
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
            if let Poll::Pending = self.poll_writes(resource, cx) {
                return Poll::Pending;
            }
            if let Poll::Pending = self.poll_pending_flush(resource, cx) {
                return Poll::Pending;
            }
            if let Some(error) = &self.write_error {
                return Poll::Ready(Err(error.make()));
            }
        }
        Poll::Ready(Ok(()))
    }

    pub(super) fn poll_close(
        &mut self,
        resource: &LocalStreamResource,
        cx: &mut Context<'_>,
    ) -> Poll<io::Result<()>> {
        if self.closed {
            return self.completed_close();
        }
        if let Poll::Pending = self.poll_seek_barrier(resource, cx) {
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
            if let Poll::Pending = self.poll_writes(resource, cx) {
                return Poll::Pending;
            }
            if let Poll::Pending = self.poll_pending_flush(resource, cx) {
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
                self.close_error = Some(SavedError::capture(error));
                self.completed_close()
            }
        }
    }

    pub(super) fn completed_close(&self) -> Poll<io::Result<()>> {
        self.close_error
            .as_ref()
            .map_or(Poll::Ready(Ok(())), |error| Poll::Ready(Err(error.make())))
    }

    pub(super) fn request_close_on_drop(&mut self, resource: &LocalStreamResource) {
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

    pub(super) fn is_closed(&self) -> bool {
        self.closed
    }
}
