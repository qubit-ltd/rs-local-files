//! Cancellation-safe asynchronous local file streams.
//!
//! A [`LocalStreamResource`] is a control port. The native reader or writer
//! must be owned by its serial worker; implementations of [`LocalStreamDriver`]
//! only move owned requests to that worker and report their completion.

use std::collections::{HashMap, VecDeque};
use std::io;
use std::io::SeekFrom;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::task::Waker;
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
    Read {
        max_bytes: usize,
    },
    Write {
        bytes: Arc<[u8]>,
    },
    Flush,
    Seek(SeekFrom),
    Close,
    /// Aborts and releases the native owner when the public port is dropped.
    Abort,
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
    resource_id: Arc<Mutex<Option<qubit_operation::ResourceId>>>,
}

impl LocalStreamResource {
    /// Wraps a serial-worker control port.
    pub fn new(driver: impl LocalStreamDriver) -> Self {
        Self {
            driver: Arc::new(driver),
            claimed: AtomicBool::new(false),
            resource_id: Arc::new(Mutex::new(None)),
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

/// Native stream state owned by the local I/O worker. This concrete driver is
/// used by the filesystem facade; the public trait remains useful for adapters
/// and deterministic contract tests.
pub(crate) enum LocalNativeStream {
    Reader(crate::LocalFileReader),
    Writer(crate::LocalFileWriter),
}

trait WorkerClosable: Send + Sync + 'static {
    fn request_cleanup(&self);
    fn poll_cleanup(&self, cx: &mut Context<'_>) -> Poll<io::Result<()>>;
    fn resource_id(&self) -> Arc<Mutex<Option<qubit_operation::ResourceId>>>;
}

impl WorkerClosable for LocalStreamResource {
    fn request_cleanup(&self) {
        self.request_close(0);
    }

    fn poll_cleanup(&self, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        let request = LocalStreamRequest {
            id: 0,
            command: LocalStreamCommand::Abort,
        };
        match self.poll_command(cx, &request) {
            Poll::Ready(Ok(LocalStreamCompletion::Closed)) => Poll::Ready(Ok(())),
            Poll::Ready(Ok(LocalStreamCompletion::Unconfirmed { operation, detail })) => {
                Poll::Ready(Err(io::Error::other(format!(
                    "Unconfirmed {operation}: {detail}"
                ))))
            }
            Poll::Ready(Ok(_)) => Poll::Ready(Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "unexpected stream cleanup completion",
            ))),
            Poll::Ready(Err(error)) => Poll::Ready(Err(error)),
            Poll::Pending => Poll::Pending,
        }
    }

    fn resource_id(&self) -> Arc<Mutex<Option<qubit_operation::ResourceId>>> {
        Arc::clone(&self.resource_id)
    }
}

impl WorkerClosable for LocalWalkResource {
    fn request_cleanup(&self) {
        self.request_stop();
    }
    fn poll_cleanup(&self, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        self.poll_closed(cx)
    }
    fn resource_id(&self) -> Arc<Mutex<Option<qubit_operation::ResourceId>>> {
        Arc::clone(&self.resource_id)
    }
}

struct WorkerCleanup<T>(std::marker::PhantomData<fn(T)>);

impl<T: WorkerClosable> WorkerCleanup<T> {
    fn policy() -> Arc<Self> {
        Arc::new(Self(std::marker::PhantomData))
    }

    fn dispose(resource: T) {
        resource.request_cleanup();
        // The queued terminal command retains the driver and native owner.
        drop(resource);
    }
}

impl<T: WorkerClosable> qubit_operation::AsyncCleanup<T> for WorkerCleanup<T> {
    fn cleanup(
        self: Arc<Self>,
        resource: T,
        _context: qubit_operation::CleanupContext,
    ) -> qubit_operation::CleanupFuture<T> {
        let resource_id = resource.resource_id();
        resource.request_cleanup();
        let mut resource = Some(resource);
        Box::pin(std::future::poll_fn(move |cx| {
            match resource
                .as_ref()
                .expect("cleanup resource retained")
                .poll_cleanup(cx)
            {
                Poll::Pending => Poll::Pending,
                Poll::Ready(Ok(())) => {
                    drop(resource.take());
                    Poll::Ready(qubit_operation::CleanupOutcome::Released)
                }
                Poll::Ready(Err(error)) => {
                    let id = resource_id
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .expect("managed worker resource has a ResourceId");
                    let failure = qubit_operation::CleanupFailure {
                        resource_id: id,
                        stage: qubit_operation::CleanupStage::AsyncRelease,
                        summary: qubit_operation::DiagnosticSummary::new(&error.to_string()),
                        source: Some(Arc::new(error)),
                    };
                    Poll::Ready(qubit_operation::CleanupOutcome::Failed {
                        failure,
                        resource: resource.take().expect("cleanup resource retained"),
                    })
                }
            }
        }))
    }
}

struct WorkerWalkCommand {
    result: Option<io::Result<Option<Vec<PathBuf>>>>,
    waker: Option<Waker>,
}

struct WorkerWalkInner {
    executor: super::LocalIoExecutor,
    walker: Mutex<Option<crate::LocalDirectoryWalker>>,
    commands: Mutex<HashMap<u64, WorkerWalkCommand>>,
    stop_requested: AtomicBool,
    closed: AtomicBool,
    close_error: Mutex<Option<(io::ErrorKind, String)>>,
    close_waker: Mutex<Option<Waker>>,
    cleanup_permit: Mutex<Option<super::internal::permits::Permit>>,
    _buffer_budget: Arc<Mutex<super::internal::permits::Permit>>,
}

pub(crate) struct WorkerWalkDriver(Arc<WorkerWalkInner>);

impl WorkerWalkDriver {
    pub(crate) fn managed(
        scope: &qubit_operation::OperationScope,
        executor: super::LocalIoExecutor,
        walker: crate::LocalDirectoryWalker,
        buffer_budget: Arc<Mutex<super::internal::permits::Permit>>,
    ) -> io::Result<ResourceRef<LocalWalkResource>> {
        let cleanup_permit = executor.reserve_cleanup().ok_or_else(|| {
            io::Error::new(io::ErrorKind::WouldBlock, "local cleanup capacity is full")
        })?;
        let cleanup = WorkerCleanup::<LocalWalkResource>::policy();
        let resource = LocalWalkResource::new(Self(Arc::new(WorkerWalkInner {
            executor,
            walker: Mutex::new(Some(walker)),
            commands: Mutex::new(HashMap::new()),
            stop_requested: AtomicBool::new(false),
            closed: AtomicBool::new(false),
            close_error: Mutex::new(None),
            close_waker: Mutex::new(None),
            cleanup_permit: Mutex::new(Some(cleanup_permit)),
            _buffer_budget: buffer_budget,
        })));
        let resources = scope
            .resource_scope(qubit_operation::ResourceScopeOptions { capacity: 1 })
            .map_err(io::Error::other)?;
        match resources.manage_with(resource, Arc::clone(&cleanup)) {
            Ok(reference) => {
                *reference
                    .resource_id
                    .lock()
                    .unwrap_or_else(|e| e.into_inner()) = Some(reference.id());
                Ok(reference)
            }
            Err(error) => {
                let (reason, resource, _) = error.into_parts();
                WorkerCleanup::<LocalWalkResource>::dispose(resource);
                Err(io::Error::other(format!(
                    "failed to register local walker: {reason:?}"
                )))
            }
        }
    }
}

impl LocalWalkDriver for WorkerWalkDriver {
    fn poll_batch(
        &self,
        cx: &mut Context<'_>,
        request_id: u64,
        max_entries: usize,
        max_bytes: usize,
    ) -> Poll<io::Result<Option<Vec<PathBuf>>>> {
        let mut schedule = false;
        {
            let mut commands = self.0.commands.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(command) = commands.get_mut(&request_id) {
                if let Some(result) = command.result.take() {
                    commands.remove(&request_id);
                    return Poll::Ready(result);
                }
                command.waker = Some(cx.waker().clone());
            } else {
                commands.insert(
                    request_id,
                    WorkerWalkCommand {
                        result: None,
                        waker: Some(cx.waker().clone()),
                    },
                );
                schedule = true;
            }
        }
        if schedule {
            let inner = Arc::clone(&self.0);
            let job = move || {
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    if inner.stop_requested.load(Ordering::Acquire) {
                        return Ok(None);
                    }
                    let mut walker_slot = inner.walker.lock().unwrap_or_else(|e| e.into_inner());
                    let Some(walker) = walker_slot.as_mut() else {
                        return Ok(None);
                    };
                    let mut entries = Vec::new();
                    let mut total_bytes = 0usize;
                    for _ in 0..max_entries {
                        match walker.next() {
                            None => {
                                *walker_slot = None;
                                break;
                            }
                            Some(Err(error)) => return Err(io::Error::other(error)),
                            Some(Ok(entry)) => {
                                let path = entry.path().to_path_buf();
                                let size = path.as_os_str().as_encoded_bytes().len();
                                if size > max_bytes || total_bytes.saturating_add(size) > max_bytes
                                {
                                    return Err(io::Error::new(
                                        io::ErrorKind::OutOfMemory,
                                        "walker batch path budget exceeded",
                                    ));
                                }
                                total_bytes += size;
                                entries.push(path);
                            }
                        }
                    }
                    if entries.is_empty() && walker_slot.is_none() {
                        Ok(None)
                    } else {
                        Ok(Some(entries))
                    }
                }))
                .unwrap_or_else(|_| {
                    Err(io::Error::other(
                        "Unconfirmed: local walker worker panicked",
                    ))
                });
                let wake = {
                    let mut commands = inner.commands.lock().unwrap_or_else(|e| e.into_inner());
                    let Some(command) = commands.get_mut(&request_id) else {
                        return;
                    };
                    command.result = Some(result);
                    command.waker.take()
                };
                if let Some(waker) = wake {
                    let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| waker.wake()));
                }
            };
            if self.0.executor.enqueue_task(job).is_err() {
                self.0
                    .commands
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .remove(&request_id);
                return Poll::Ready(Err(io::Error::new(
                    io::ErrorKind::WouldBlock,
                    "local worker queue is full",
                )));
            }
        }
        Poll::Pending
    }

    fn request_stop(&self) {
        if self.0.stop_requested.swap(true, Ordering::AcqRel) {
            return;
        }
        let inner = Arc::clone(&self.0);
        let job = Box::new(move || {
            inner
                .walker
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .take();
            inner.closed.store(true, Ordering::Release);
            if let Some(waker) = inner
                .close_waker
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .take()
            {
                let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| waker.wake()));
            }
        });
        if let Some(permit) = self
            .0
            .cleanup_permit
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take()
        {
            if let Err((job, _permit)) = self.0.executor.enqueue_cleanup(job, permit) {
                if self.0.executor.enqueue_task(job).is_err() {
                    *self.0.close_error.lock().unwrap_or_else(|e| e.into_inner()) = Some((
                        io::ErrorKind::WouldBlock,
                        "local cleanup queues are unavailable".to_owned(),
                    ));
                    let retained = Arc::clone(&self.0);
                    std::mem::forget(retained);
                }
            }
        }
    }

    fn poll_closed(&self, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        if let Some((kind, message)) = self
            .0
            .close_error
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
        {
            return Poll::Ready(Err(io::Error::new(kind, message)));
        }
        if self.0.closed.load(Ordering::Acquire) {
            return Poll::Ready(Ok(()));
        }
        *self.0.close_waker.lock().unwrap_or_else(|e| e.into_inner()) = Some(cx.waker().clone());
        if self.0.closed.load(Ordering::Acquire) {
            Poll::Ready(Ok(()))
        } else {
            Poll::Pending
        }
    }
}

struct WorkerCommand {
    result: Option<io::Result<LocalStreamCompletion>>,
    waker: Option<Waker>,
}

struct WorkerStreamState {
    commands: HashMap<u64, WorkerCommand>,
}

struct WorkerStreamInner {
    executor: super::LocalIoExecutor,
    native: Mutex<Option<LocalNativeStream>>,
    state: Mutex<WorkerStreamState>,
    queue: Mutex<VecDeque<LocalStreamRequest>>,
    active: AtomicBool,
    cleanup_permit: Mutex<Option<super::internal::permits::Permit>>,
    _buffer_budget: Arc<Mutex<super::internal::permits::Permit>>,
}

pub(crate) struct WorkerStreamDriver(Arc<WorkerStreamInner>);

impl WorkerStreamDriver {
    pub(crate) fn managed(
        scope: &qubit_operation::OperationScope,
        executor: super::LocalIoExecutor,
        native: LocalNativeStream,
        buffer_budget: Arc<Mutex<super::internal::permits::Permit>>,
    ) -> io::Result<ResourceRef<LocalStreamResource>> {
        let cleanup_permit = executor.reserve_cleanup().ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::WouldBlock,
                "local stream cleanup capacity is full",
            )
        })?;
        let cleanup = WorkerCleanup::<LocalStreamResource>::policy();
        let resource = LocalStreamResource::new(Self(Arc::new(WorkerStreamInner {
            executor,
            native: Mutex::new(Some(native)),
            state: Mutex::new(WorkerStreamState {
                commands: HashMap::new(),
            }),
            queue: Mutex::new(VecDeque::new()),
            active: AtomicBool::new(false),
            cleanup_permit: Mutex::new(Some(cleanup_permit)),
            _buffer_budget: buffer_budget,
        })));
        let resources = scope
            .resource_scope(qubit_operation::ResourceScopeOptions { capacity: 1 })
            .map_err(io::Error::other)?;
        match resources.manage_with(resource, Arc::clone(&cleanup)) {
            Ok(reference) => {
                *reference
                    .resource_id
                    .lock()
                    .unwrap_or_else(|e| e.into_inner()) = Some(reference.id());
                Ok(reference)
            }
            Err(error) => {
                let (reason, resource, _) = error.into_parts();
                WorkerCleanup::<LocalStreamResource>::dispose(resource);
                Err(io::Error::other(format!(
                    "failed to register local stream: {reason:?}"
                )))
            }
        }
    }

    fn schedule(&self, request: &LocalStreamRequest) -> io::Result<()> {
        let should_start = {
            let mut queue = self.0.queue.lock().unwrap_or_else(|e| e.into_inner());
            queue.push_back(request.clone());
            !self.0.active.swap(true, Ordering::AcqRel)
        };
        if should_start {
            start_stream_drain(Arc::clone(&self.0))?;
        }
        Ok(())
    }
}

fn start_stream_drain(inner: Arc<WorkerStreamInner>) -> io::Result<()> {
    let task_inner = Arc::clone(&inner);
    let task = move || drain_stream_commands(task_inner);
    if inner.executor.enqueue_task(task).is_ok() {
        return Ok(());
    }

    // Every opened stream reserves one cleanup slot before it can escape its
    // operation. Use it when normal queue admission is exhausted (especially
    // for Drop-triggered Abort).
    let task_inner = Arc::clone(&inner);
    let fallback: Box<dyn FnOnce() + Send> = Box::new(move || drain_stream_commands(task_inner));
    if let Some(permit) = inner
        .cleanup_permit
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .take()
    {
        match inner.executor.enqueue_cleanup(fallback, permit) {
            Ok(()) => return Ok(()),
            Err((job, permit)) => {
                *inner
                    .cleanup_permit
                    .lock()
                    .unwrap_or_else(|e| e.into_inner()) = Some(permit);
                // Preserve the native owner if the executor is already stopping.
                std::mem::forget(job);
            }
        }
    } else {
        std::mem::forget(fallback);
    }
    inner.active.store(false, Ordering::Release);
    Err(io::Error::new(
        io::ErrorKind::WouldBlock,
        "local worker queue and cleanup lane are unavailable",
    ))
}

fn drain_stream_commands(inner: Arc<WorkerStreamInner>) {
    loop {
        let request = {
            let mut queue = inner.queue.lock().unwrap_or_else(|e| e.into_inner());
            match queue.pop_front() {
                Some(request) => request,
                None => {
                    inner.active.store(false, Ordering::Release);
                    return;
                }
            }
        };
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let mut native = inner.native.lock().unwrap_or_else(|e| e.into_inner());
            execute_stream_command(&mut native, &request.command)
        }))
        .unwrap_or_else(|_| {
            Err(io::Error::other(
                "Unconfirmed: local stream worker panicked",
            ))
        });
        let waker = {
            let mut state = inner.state.lock().unwrap_or_else(|e| e.into_inner());
            let Some(slot) = state.commands.get_mut(&request.id) else {
                continue;
            };
            slot.result = Some(result);
            slot.waker.take()
        };
        if let Some(waker) = waker {
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| waker.wake()));
        }
    }
}

impl LocalStreamDriver for WorkerStreamDriver {
    fn poll_command(
        &self,
        cx: &mut Context<'_>,
        request: &LocalStreamRequest,
    ) -> Poll<io::Result<LocalStreamCompletion>> {
        let schedule = {
            let mut state = self.0.state.lock().unwrap_or_else(|e| e.into_inner());
            match state.commands.get_mut(&request.id) {
                Some(slot) => {
                    if let Some(result) = slot.result.take() {
                        state.commands.remove(&request.id);
                        return Poll::Ready(result);
                    }
                    slot.waker = Some(cx.waker().clone());
                    false
                }
                None => {
                    state.commands.insert(
                        request.id,
                        WorkerCommand {
                            result: None,
                            waker: Some(cx.waker().clone()),
                        },
                    );
                    true
                }
            }
        };
        if schedule {
            if let Err(error) = self.schedule(request) {
                self.0
                    .state
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .commands
                    .remove(&request.id);
                return Poll::Ready(Err(error));
            }
        }
        Poll::Pending
    }

    fn request_close(&self, request_id: u64) {
        let request = LocalStreamRequest {
            id: request_id,
            command: LocalStreamCommand::Abort,
        };
        {
            let mut state = self.0.state.lock().unwrap_or_else(|e| e.into_inner());
            if state.commands.contains_key(&request_id) {
                return;
            }
            state.commands.insert(
                request_id,
                WorkerCommand {
                    result: None,
                    waker: None,
                },
            );
        }
        if let Err(error) = self.schedule(&request) {
            let wake = {
                let mut state = self.0.state.lock().unwrap_or_else(|e| e.into_inner());
                let Some(slot) = state.commands.get_mut(&request_id) else {
                    return;
                };
                slot.result = Some(Err(error));
                slot.waker.take()
            };
            if let Some(waker) = wake {
                let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| waker.wake()));
            }
        }
    }
}

fn execute_stream_command(
    native: &mut Option<LocalNativeStream>,
    command: &LocalStreamCommand,
) -> io::Result<LocalStreamCompletion> {
    if matches!(
        command,
        LocalStreamCommand::Close | LocalStreamCommand::Abort
    ) {
        let Some(previous) = native.take() else {
            return Ok(LocalStreamCompletion::Closed);
        };
        if let LocalNativeStream::Writer(writer) = previous {
            if matches!(command, LocalStreamCommand::Abort) {
                drop(writer);
                return Ok(LocalStreamCompletion::Closed);
            }
            let _ = writer.commit().map_err(io::Error::other)?;
        }
        return Ok(LocalStreamCompletion::Closed);
    }
    let Some(native) = native.as_mut() else {
        return Err(io::Error::new(
            io::ErrorKind::BrokenPipe,
            "local stream is closed",
        ));
    };
    match (native, command) {
        (LocalNativeStream::Reader(reader), LocalStreamCommand::Read { max_bytes }) => {
            let mut bytes = vec![0; (*max_bytes).min(LOCAL_STREAM_BUFFER_SIZE)];
            let count = qubit_io::Input::read(reader, &mut bytes)?;
            bytes.truncate(count);
            Ok(LocalStreamCompletion::Read(bytes))
        }
        (LocalNativeStream::Writer(writer), LocalStreamCommand::Write { bytes }) => {
            let count = qubit_io::Output::write(writer, bytes)?;
            Ok(LocalStreamCompletion::Written(count))
        }
        (LocalNativeStream::Writer(writer), LocalStreamCommand::Flush) => {
            qubit_io::Output::flush(writer)?;
            Ok(LocalStreamCompletion::Flushed)
        }
        (LocalNativeStream::Reader(reader), LocalStreamCommand::Seek(position)) => Ok(
            LocalStreamCompletion::Seeked(std::io::Seek::seek(reader, *position)?),
        ),
        (LocalNativeStream::Writer(_), LocalStreamCommand::Seek(_)) => Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "seeking a local file writer is unsupported",
        )),
        (LocalNativeStream::Reader(_), LocalStreamCommand::Write { .. })
        | (LocalNativeStream::Reader(_), LocalStreamCommand::Flush)
        | (LocalNativeStream::Writer(_), LocalStreamCommand::Read { .. }) => Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "stream command has the wrong direction",
        )),
        (_, LocalStreamCommand::Close | LocalStreamCommand::Abort) => {
            unreachable!("terminal command handled above")
        }
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
    resource_id: Arc<Mutex<Option<qubit_operation::ResourceId>>>,
}

impl LocalWalkResource {
    /// Wraps a serial-worker walker port.
    pub fn new(driver: impl LocalWalkDriver) -> Self {
        Self {
            driver: Arc::new(driver),
            claimed: AtomicBool::new(false),
            resource_id: Arc::new(Mutex::new(None)),
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
