//! Cancellation-safe asynchronous local file streams.
//!
//! A [`LocalStreamResource`] is a control port. The native reader or writer
//! must be owned by its serial worker; implementations of [`LocalStreamDriver`]
//! only move owned requests to that worker and report their completion.

use std::io;
use std::io::SeekFrom;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::task::{Context, Poll};

use qubit_io::{AsyncInput, AsyncOutput};
use qubit_operation::ResourceRef;

#[path = "internal/stream_state.rs"]
mod stream_state;
use stream_state::{Direction, StreamState, WalkState, WalkerResourceLimit};

/// Default owned read and write buffer size for one stream.
pub const LOCAL_STREAM_BUFFER_SIZE: usize = 64 * 1024;
/// Maximum directory entries returned by one walker batch.
pub const LOCAL_WALK_BATCH_ENTRIES: usize = 128;
/// Maximum encoded path bytes returned by one walker batch.
pub const LOCAL_WALK_BATCH_BYTES: usize = 256 * 1024;

/// An owned command sent to a stream's serial worker.
#[derive(Debug, Clone)]
pub enum LocalStreamCommand {
    Read { max_bytes: usize },
    Write { bytes: Arc<[u8]> },
    Flush,
    Seek(SeekFrom),
    Close,
}

/// Stable identity and owned payload for one command.
#[derive(Debug, Clone)]
pub struct LocalStreamRequest {
    pub id: u64,
    pub command: LocalStreamCommand,
}

/// Result returned by the serial worker.
#[derive(Debug)]
pub enum LocalStreamCompletion {
    Read(Vec<u8>),
    Written(usize),
    Flushed,
    Seeked(u64),
    Closed,
    /// Worker/driver disappeared after command submission, so its side effect
    /// cannot be confirmed.
    Unconfirmed {
        operation: &'static str,
        detail: String,
    },
}

/// Non-blocking driver implemented by a local serial worker or a deterministic fake.
///
/// For a given request id, a driver must schedule at most one command and keep
/// its completion observable until it returns `Ready` from this method. Calls
/// for one resource are serialized by the stream handle. A driver must not
/// retain caller buffers: write payloads are shared owned allocations and read
/// storage is allocated by the driver/worker.
pub trait LocalStreamDriver: Send + Sync + 'static {
    fn poll_command(
        &self,
        cx: &mut Context<'_>,
        request: &LocalStreamRequest,
    ) -> Poll<io::Result<LocalStreamCompletion>>;

    /// Enqueues a close command without blocking when the owning handle drops.
    /// The command id must be idempotent with a later explicit `close` poll.
    fn request_close(&self, request_id: u64);
}

/// Thread-safe control port for one exclusively-owned native stream.
///
/// Cloning a `ResourceRef` shares this control port; it does not create a
/// second mutable reader or writer. `ResourceRef` keeps its owning operation
/// resource alive while an async stream handle exists.
pub struct LocalStreamResource {
    driver: Arc<dyn LocalStreamDriver>,
    claimed: AtomicBool,
}

impl LocalStreamResource {
    /// Wraps a serial-worker control port.
    pub fn new(driver: impl LocalStreamDriver) -> Self {
        Self {
            driver: Arc::new(driver),
            claimed: AtomicBool::new(false),
        }
    }

    fn claim(&self) -> io::Result<()> {
        self.claimed
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map(|_| ())
            .map_err(|_| {
                io::Error::new(
                    io::ErrorKind::AlreadyExists,
                    "local stream already has an exclusive handle",
                )
            })
    }

    fn poll_command(
        &self,
        cx: &mut Context<'_>,
        request: &LocalStreamRequest,
    ) -> Poll<io::Result<LocalStreamCompletion>> {
        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            self.driver.poll_command(cx, request)
        })) {
            Ok(result) => result,
            Err(_) => Poll::Ready(Err(io::Error::other(
                "Unconfirmed: local stream driver panicked",
            ))),
        }
    }

    fn request_close(&self, request_id: u64) {
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            self.driver.request_close(request_id)
        }));
    }
}

/// An exclusive asynchronous reader for a managed local file.
pub struct AsyncLocalFileReader {
    resource: Option<ResourceRef<LocalStreamResource>>,
    state: StreamState,
}

impl AsyncLocalFileReader {
    /// Creates a reader over a managed serial-worker resource.
    pub fn new(resource: ResourceRef<LocalStreamResource>) -> io::Result<Self> {
        resource.claim()?;
        Ok(Self {
            resource: Some(resource),
            state: StreamState::new(Direction::Read),
        })
    }

    /// Performs a cancellation-safe seek. A cancelled seek remains in the
    /// stream state and is settled before a later read is allowed to proceed.
    pub async fn seek(&mut self, position: SeekFrom) -> io::Result<u64> {
        let Some(resource) = self.resource.as_deref() else {
            return Err(StreamState::closed_error());
        };
        let state = &mut self.state;
        std::future::poll_fn(move |cx| state.poll_seek(resource, cx, position)).await
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
    type Item = u8;

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
    fn drop(&mut self) {
        if let Some(resource) = &self.resource {
            self.state.request_close_on_drop(resource);
        }
    }
}

/// An exclusive asynchronous writer for a managed local file.
pub struct AsyncLocalFileWriter {
    resource: Option<ResourceRef<LocalStreamResource>>,
    state: StreamState,
}

impl AsyncLocalFileWriter {
    /// Creates a writer over a managed serial-worker resource.
    pub fn new(resource: ResourceRef<LocalStreamResource>) -> io::Result<Self> {
        resource.claim()?;
        Ok(Self {
            resource: Some(resource),
            state: StreamState::new(Direction::Write),
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
    type Item = u8;

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
    fn drop(&mut self) {
        if let Some(resource) = &self.resource {
            self.state.request_close_on_drop(resource);
        }
    }
}

/// Serial worker port for a lazy directory walker.
pub trait LocalWalkDriver: Send + Sync + 'static {
    /// Polls one stable batch request. Re-polling the same id must observe the
    /// same result; a driver must not advance its native walker twice.
    fn poll_batch(
        &self,
        cx: &mut Context<'_>,
        request_id: u64,
        max_entries: usize,
        max_bytes: usize,
    ) -> Poll<io::Result<Option<Vec<PathBuf>>>>;

    /// Requests worker-side stop and native directory handle closure.
    /// Implementations must enqueue/merge this without blocking the caller.
    fn request_stop(&self);

    /// Confirms the native directory handle is closed.
    fn poll_closed(&self, cx: &mut Context<'_>) -> Poll<io::Result<()>>;
}

/// Managed control port for an exclusively-owned directory walker.
pub struct LocalWalkResource {
    driver: Arc<dyn LocalWalkDriver>,
    claimed: AtomicBool,
}

impl LocalWalkResource {
    /// Wraps a serial-worker walker port.
    pub fn new(driver: impl LocalWalkDriver) -> Self {
        Self {
            driver: Arc::new(driver),
            claimed: AtomicBool::new(false),
        }
    }

    fn claim(&self) -> io::Result<()> {
        self.claimed
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .map(|_| ())
            .map_err(|_| {
                io::Error::new(
                    io::ErrorKind::AlreadyExists,
                    "walker already has an exclusive handle",
                )
            })
    }

    fn poll_batch(
        &self,
        cx: &mut Context<'_>,
        request_id: u64,
    ) -> Poll<io::Result<Option<Vec<PathBuf>>>> {
        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            self.driver.poll_batch(
                cx,
                request_id,
                LOCAL_WALK_BATCH_ENTRIES,
                LOCAL_WALK_BATCH_BYTES,
            )
        })) {
            Ok(result) => result,
            Err(_) => Poll::Ready(Err(io::Error::other(
                "Unconfirmed: local walker driver panicked",
            ))),
        }
    }

    fn request_stop(&self) {
        let _ =
            std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| self.driver.request_stop()));
    }

    fn poll_closed(&self, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| self.driver.poll_closed(cx)))
        {
            Ok(result) => result,
            Err(_) => Poll::Ready(Err(io::Error::other(
                "Unconfirmed: local walker close driver panicked",
            ))),
        }
    }
}

/// Asynchronous directory walker that returns bounded path batches.
pub struct AsyncLocalDirectoryWalker {
    resource: Option<ResourceRef<LocalWalkResource>>,
    state: WalkState,
}

impl AsyncLocalDirectoryWalker {
    /// Creates an exclusive walker over a managed worker resource.
    pub fn new(resource: ResourceRef<LocalWalkResource>) -> io::Result<Self> {
        resource.claim()?;
        Ok(Self {
            resource: Some(resource),
            state: WalkState::default(),
        })
    }

    /// Returns the next bounded batch, or `None` after EOF and confirmed close.
    pub async fn next_batch(&mut self) -> crate::LocalResult<Option<Vec<PathBuf>>> {
        let resource = self
            .resource
            .as_deref()
            .expect("walker resource retained until close");
        let result = {
            let state = &mut self.state;
            std::future::poll_fn(move |cx| state.poll_batch(resource, cx)).await
        };
        if result.as_ref().is_ok_and(Option::is_none) && self.state.is_closed() {
            self.resource.take();
        }
        result.map_err(map_walk_error)
    }

    /// Stops traversal and waits for the worker to confirm handle closure.
    pub async fn close(&mut self) -> crate::LocalResult<()> {
        let Some(resource) = self.resource.as_deref() else {
            return match self.state.poll_closed_without_resource() {
                Poll::Ready(result) => result.map_err(map_walk_error),
                Poll::Pending => unreachable!("a released walker has completed closure"),
            };
        };
        self.state.request_stop(resource);
        let result = {
            let state = &mut self.state;
            std::future::poll_fn(move |cx| state.poll_close(resource, cx)).await
        };
        if self.state.is_closed() {
            self.resource.take();
        }
        result.map_err(map_walk_error)
    }

    /// Returns a close observer that survives dropping this walker handle.
    pub fn close_observer(&self) -> LocalWalkCloseObserver {
        LocalWalkCloseObserver {
            resource: self
                .resource
                .as_ref()
                .expect("walker resource retained")
                .clone(),
        }
    }
}

impl Drop for AsyncLocalDirectoryWalker {
    fn drop(&mut self) {
        if !self.state.is_closed() {
            if let Some(resource) = &self.resource {
                self.state.request_stop(resource);
            }
        }
    }
}

/// Observer for background walker stop and native-handle close confirmation.
pub struct LocalWalkCloseObserver {
    resource: ResourceRef<LocalWalkResource>,
}

impl LocalWalkCloseObserver {
    /// Waits for the serial worker's native handle close acknowledgement.
    pub async fn wait_closed(&self) -> crate::LocalResult<()> {
        std::future::poll_fn(|cx| self.resource.poll_closed(cx))
            .await
            .map_err(map_walk_error)
    }
}

fn map_walk_error(error: io::Error) -> crate::LocalFileError {
    if let Some(limit) = error
        .get_ref()
        .and_then(|source| source.downcast_ref::<WalkerResourceLimit>())
    {
        crate::error::LocalFileError::new(
            crate::error::LocalFileErrorKind::ResourceLimit,
            crate::error::LocalFileOperation::List,
        )
        .with_reason("asynchronous walker batch resource limit exceeded")
        .with_detail(limit.to_string())
    } else {
        crate::error::LocalFileError::from_io(
            crate::error::LocalFileOperation::List,
            None,
            None,
            error,
        )
    }
}
