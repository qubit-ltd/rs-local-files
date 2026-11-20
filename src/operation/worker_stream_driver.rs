// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::collections::HashMap;
use std::collections::VecDeque;
use std::io;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::task::Context;
use std::task::Poll;

use qubit_operation::ResourceRef;

use super::LocalNativeStream;
use super::LocalStreamCommand;
use super::LocalStreamCompletion;
use super::LocalStreamDriver;
use super::LocalStreamRequest;
use super::LocalStreamResource;
use super::WorkerCleanup;
use super::fail_stream_queue;
use super::native_stream_writer::NativeStreamWriter;
use super::start_stream_drain;
use super::worker_stream_command::WorkerCommand;
use super::worker_stream_inner::WorkerStreamInner;
use super::worker_stream_queued_request::WorkerStreamQueuedRequest;
use super::worker_stream_state::WorkerStreamState;

/// Worker-backed driver that serializes commands for one native stream.
pub(crate) struct WorkerStreamDriver(
    /// Shared queue and native state owned by this worker driver.
    Arc<WorkerStreamInner>,
);

impl WorkerStreamDriver {
    /// Registers a native stream worker as an operation-managed resource.
    ///
    /// # Parameters
    ///
    /// * `scope` - Operation scope that owns cleanup for the stream.
    /// * `executor` - Serial I/O executor used by this stream.
    /// * `native` - Exclusive native reader or writer handle.
    /// * `buffer_budget` - Reservation retained for stream buffers.
    /// * `handle_budget` - Reservation retained while the native handle is
    ///   open.
    /// * `observer` - Optional operation fact observer for append writes.
    /// * `append_mode` - Whether writes should update append-specific facts.
    pub(crate) fn managed(
        scope: &qubit_operation::OperationScope,
        executor: super::super::LocalIoExecutor,
        native: LocalNativeStream,
        buffer_budget: Arc<Mutex<super::super::internal::permits::Permit>>,
        handle_budget: Arc<super::super::internal::permits::Permit>,
        observer: Option<super::super::LocalFactObserver>,
        append_mode: bool,
    ) -> io::Result<ResourceRef<LocalStreamResource>> {
        let cleanup_permit = executor
            .reserve_cleanup()
            .ok_or_else(|| io::Error::new(io::ErrorKind::WouldBlock, "local stream cleanup capacity is full"))?;
        let cleanup = WorkerCleanup::<LocalStreamResource>::policy();
        let resource = LocalStreamResource::new_native(Self(Arc::new(WorkerStreamInner {
            executor,
            native: Mutex::new(Some(native)),
            state: Mutex::new(WorkerStreamState {
                commands: HashMap::new(),
            }),
            queue: Mutex::new(VecDeque::new()),
            native_write_ids: Mutex::new(HashMap::new()),
            next_internal_request_id: std::sync::atomic::AtomicU64::new(1 << 63),
            active: AtomicBool::new(false),
            cleanup_permit: Mutex::new(Some(cleanup_permit)),
            _buffer_budget: buffer_budget,
            _handle_budget: handle_budget,
            observer,
            written_bytes: std::sync::atomic::AtomicU64::new(0),
            append_mode,
        })));
        let resources = scope
            .resource_scope(qubit_operation::ResourceScopeOptions { capacity: 1 })
            .map_err(io::Error::other)?;
        match resources.manage_with(resource, Arc::clone(&cleanup)) {
            Ok(reference) => {
                set_stream_resource_id(&reference.resource_id, reference.id());
                Ok(reference)
            }
            Err(error) => {
                let (reason, resource, _) = error.into_parts();
                WorkerCleanup::<LocalStreamResource>::dispose(resource);
                Err(io::Error::other(format!("failed to register local stream: {reason:?}")))
            }
        }
    }

    /// Appends one request to the FIFO and starts a drain if it is idle.
    ///
    /// # Parameters
    ///
    /// * `request` - Owned request to append to this worker's queue.
    fn schedule(&self, request: &LocalStreamRequest) -> io::Result<()> {
        self.schedule_with_offset(request, 0)
    }

    /// Enqueues one request and records its original payload progress.
    ///
    /// # Parameters
    ///
    /// * `request` - Owned request to append to the serial worker queue.
    /// * `write_offset` - Number of bytes already written from its payload.
    fn schedule_with_offset(&self, request: &LocalStreamRequest, write_offset: usize) -> io::Result<()> {
        let should_start = {
            let mut queue = self.0.queue.lock().unwrap_or_else(|e| e.into_inner());
            queue.push_back(WorkerStreamQueuedRequest {
                request: request.clone(),
                write_offset,
            });
            !self.0.active.swap(true, Ordering::AcqRel)
        };
        if should_start {
            let terminal = matches!(request.command, LocalStreamCommand::Close | LocalStreamCommand::Abort);
            if let Err(error) = start_stream_drain(Arc::clone(&self.0), terminal) {
                fail_stream_queue(&self.0, io::Error::new(error.kind(), error.to_string()));
                return Err(error);
            }
        }
        Ok(())
    }
}

/// Stores the registered operation id while recovering a poisoned id lock.
///
/// # Type Parameters
///
/// * `T` - Copyable resource identifier stored in the lock.
///
/// # Parameters
///
/// * `resource_id` - Mutex containing the worker resource id.
/// * `id` - Operation resource id to record.
fn set_stream_resource_id<T: Copy>(resource_id: &Mutex<Option<T>>, id: T) {
    *resource_id.lock().unwrap_or_else(|error| error.into_inner()) = Some(id);
}

impl LocalStreamDriver for WorkerStreamDriver {
    /// Schedules one command once and returns its worker completion when ready.
    ///
    /// # Parameters
    ///
    /// * `cx` - Task context to wake when worker execution completes.
    /// * `request` - Stable command request submitted by the stream handle.
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
        if schedule && let Err(error) = self.schedule(request) {
            self.0
                .state
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .commands
                .remove(&request.id);
            return Poll::Ready(Err(error));
        }
        Poll::Pending
    }

    /// Reports the latest publication state confirmed by this worker.
    fn write_facts(&self) -> Option<(crate::LocalWriteFailureState, u64)> {
        let snapshot = self.0.observer.as_ref()?.snapshot();
        let state = match snapshot.publication {
            super::super::LocalPublicationState::NotPublished => crate::LocalWriteFailureState::NotPublished,
            super::super::LocalPublicationState::Published => crate::LocalWriteFailureState::Published,
            super::super::LocalPublicationState::Unknown => crate::LocalWriteFailureState::Indeterminate,
        };
        Some((
            state,
            snapshot
                .modified_bytes
                .max(self.0.written_bytes.load(Ordering::Acquire)),
        ))
    }

    /// Enqueues a terminal abort command and retries queue admission briefly.
    ///
    /// # Parameters
    ///
    /// * `request_id` - Stable identifier used for the terminal request.
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
        let mut scheduled = false;
        let mut last_error = None;
        for _ in 0..8 {
            match self.schedule(&request) {
                Ok(()) => {
                    scheduled = true;
                    break;
                }
                Err(error) => last_error = Some(error),
            }
        }
        if !scheduled {
            let wake = {
                let mut state = self.0.state.lock().unwrap_or_else(|e| e.into_inner());
                let Some(slot) = state.commands.get_mut(&request_id) else {
                    return;
                };
                slot.result = Some(Err(last_error.unwrap_or_else(|| {
                    io::Error::new(io::ErrorKind::WouldBlock, "local stream cleanup scheduling failed")
                })));
                slot.waker.take()
            };
            if let Some(waker) = wake {
                let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| waker.wake()));
            }
        }
    }
}

impl NativeStreamWriter for WorkerStreamDriver {
    /// Polls one native attempt while preserving the original parent payload.
    ///
    /// # Parameters
    ///
    /// * `cx` - Poll context used to register the task waker.
    /// * `request` - Original request being advanced.
    /// * `offset` - Number of bytes already written.
    fn poll_native_write(
        &self,
        cx: &mut Context<'_>,
        request: &LocalStreamRequest,
        offset: usize,
    ) -> Poll<io::Result<LocalStreamCompletion>> {
        let child_id = {
            let mut ids = self.0.native_write_ids.lock().unwrap_or_else(|e| e.into_inner());
            *ids.entry((request.id, offset))
                .or_insert_with(|| self.0.next_internal_request_id.fetch_add(1, Ordering::Relaxed).max(1))
        };
        let LocalStreamCommand::Write { bytes } = &request.command else {
            return Poll::Ready(Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "native write route requires a write command",
            )));
        };
        if offset > bytes.len() {
            return Poll::Ready(Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "native write offset exceeds payload",
            )));
        }
        let child = LocalStreamRequest {
            id: child_id,
            command: request.command.clone(),
        };
        let schedule = {
            let mut state = self.0.state.lock().unwrap_or_else(|e| e.into_inner());
            match state.commands.get_mut(&child_id) {
                Some(slot) => {
                    if let Some(result) = slot.result.take() {
                        state.commands.remove(&child_id);
                        self.0
                            .native_write_ids
                            .lock()
                            .unwrap_or_else(|e| e.into_inner())
                            .remove(&(request.id, offset));
                        return Poll::Ready(result);
                    }
                    slot.waker = Some(cx.waker().clone());
                    false
                }
                None => {
                    state.commands.insert(
                        child_id,
                        WorkerCommand {
                            result: None,
                            waker: Some(cx.waker().clone()),
                        },
                    );
                    true
                }
            }
        };
        if schedule && let Err(error) = self.schedule_with_offset(&child, offset) {
            self.0
                .state
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .commands
                .remove(&child_id);
            self.0
                .native_write_ids
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .remove(&(request.id, offset));
            return Poll::Ready(Err(error));
        }
        Poll::Pending
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::collections::VecDeque;
    use std::io;
    use std::sync::Arc;
    use std::sync::Mutex;
    use std::sync::atomic::AtomicBool;
    use std::sync::mpsc;
    use std::task::Context;
    use std::task::Waker;

    use qubit_operation::ManagerOptions;
    use qubit_operation::OperationManager;

    use super::super::LocalNativeStream;
    use super::super::native_stream_writer::NativeStreamWriter;
    use super::LocalStreamCommand;
    use super::LocalStreamDriver;
    use super::LocalStreamRequest;
    use super::WorkerStreamDriver;
    use super::WorkerStreamInner;
    use super::WorkerStreamState;
    use crate::LocalExecutionConfig;
    use crate::LocalIoExecutor;

    #[test]
    fn test_resource_id_registration_recovers_a_poisoned_lock() {
        let resource_id = Mutex::new(None);
        let lock = std::panic::catch_unwind(|| {
            let _guard = resource_id.lock().unwrap();
            panic!("poison the resource id lock");
        });
        assert!(lock.is_err());

        super::set_stream_resource_id(&resource_id, 23);
        assert_eq!(*resource_id.lock().unwrap_or_else(|error| error.into_inner()), Some(23));
    }

    #[test]
    fn test_write_facts_combines_observed_and_worker_confirmed_bytes() {
        let manager = OperationManager::new(ManagerOptions::default()).unwrap();
        let executor = LocalIoExecutor::new(manager, LocalExecutionConfig::default()).unwrap();
        let observer = crate::operation::LocalFactObserver::new();
        observer.update(crate::LocalFactSnapshot {
            publication: crate::LocalPublicationState::Published,
            modified_bytes: 3,
            ..Default::default()
        });
        let driver = WorkerStreamDriver(Arc::new(WorkerStreamInner {
            executor: executor.clone(),
            native: Mutex::new(None),
            state: Mutex::new(WorkerStreamState {
                commands: HashMap::new(),
            }),
            queue: Mutex::new(VecDeque::new()),
            native_write_ids: Mutex::new(HashMap::new()),
            next_internal_request_id: std::sync::atomic::AtomicU64::new(1 << 63),
            active: AtomicBool::new(false),
            cleanup_permit: Mutex::new(Some(executor.reserve_cleanup().unwrap())),
            _buffer_budget: Arc::new(Mutex::new(executor.reserve_bytes(0).unwrap())),
            _handle_budget: Arc::new(executor.reserve_handle().unwrap()),
            observer: Some(observer),
            written_bytes: std::sync::atomic::AtomicU64::new(5),
            append_mode: true,
        }));

        assert_eq!(
            driver.write_facts(),
            Some((crate::LocalWriteFailureState::Published, 5))
        );
    }

    #[test]
    fn test_native_write_offsets_use_fresh_stable_worker_request_ids() {
        let manager = OperationManager::new(ManagerOptions::default()).unwrap();
        let executor = LocalIoExecutor::new(manager, LocalExecutionConfig::default()).unwrap();
        let driver = WorkerStreamDriver(Arc::new(WorkerStreamInner {
            executor: executor.clone(),
            native: Mutex::new(None),
            state: Mutex::new(WorkerStreamState {
                commands: HashMap::new(),
            }),
            queue: Mutex::new(VecDeque::new()),
            native_write_ids: Mutex::new(HashMap::new()),
            next_internal_request_id: std::sync::atomic::AtomicU64::new(1 << 63),
            active: AtomicBool::new(true),
            cleanup_permit: Mutex::new(Some(executor.reserve_cleanup().unwrap())),
            _buffer_budget: Arc::new(Mutex::new(executor.reserve_bytes(0).unwrap())),
            _handle_budget: Arc::new(executor.reserve_handle().unwrap()),
            observer: None,
            written_bytes: std::sync::atomic::AtomicU64::new(0),
            append_mode: false,
        }));
        let request = LocalStreamRequest {
            id: 7,
            command: LocalStreamCommand::Write {
                bytes: Arc::from(&b"payload"[..]),
            },
        };
        let mut cx = Context::from_waker(Waker::noop());
        assert!(NativeStreamWriter::poll_native_write(&driver, &mut cx, &request, 0).is_pending());
        let first = driver.0.native_write_ids.lock().unwrap()[&(7, 0)];
        assert!(NativeStreamWriter::poll_native_write(&driver, &mut cx, &request, 0).is_pending());
        assert_eq!(driver.0.native_write_ids.lock().unwrap()[&(7, 0)], first);
        assert!(NativeStreamWriter::poll_native_write(&driver, &mut cx, &request, 2).is_pending());
        let second = driver.0.native_write_ids.lock().unwrap()[&(7, 2)];
        assert_ne!(first, second);
        let queue = driver.0.queue.lock().unwrap();
        assert_eq!(queue.len(), 2);
        assert_eq!(queue[0].request.id, first);
        assert_eq!(queue[1].request.id, second);
        assert_eq!(queue[0].write_offset, 0);
        assert_eq!(queue[1].write_offset, 2);
        let LocalStreamCommand::Write { bytes } = &queue[0].request.command else {
            panic!("expected write")
        };
        let LocalStreamCommand::Write { bytes: original } = &request.command else {
            unreachable!()
        };
        assert_eq!(bytes.as_ptr(), original.as_ptr());
    }

    #[test]
    fn test_poll_command_reports_full_worker_queue() {
        let manager = OperationManager::new(ManagerOptions::default()).unwrap();
        let executor = LocalIoExecutor::new(
            manager,
            LocalExecutionConfig {
                worker_count: 1,
                queue_capacity: 1,
                waiter_capacity: 1,
                max_open_handles: 2,
                ..Default::default()
            },
        )
        .unwrap();
        let (started_tx, started_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        executor
            .enqueue_task(move || {
                started_tx.send(()).unwrap();
                release_rx.recv().unwrap();
            })
            .unwrap();
        started_rx.recv().unwrap();
        executor.enqueue_task(|| {}).unwrap();
        executor.enqueue_task(|| {}).unwrap();

        let fixture = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(fixture.path(), b"stream").unwrap();
        let native = crate::LocalFileReader::from_file(fixture.reopen().unwrap()).unwrap();
        let buffer_budget = Arc::new(Mutex::new(executor.reserve_bytes(0).unwrap()));
        let handle_budget = Arc::new(executor.reserve_handle().unwrap());
        let cleanup_permit = executor.reserve_cleanup().unwrap();
        let driver = WorkerStreamDriver(Arc::new(WorkerStreamInner {
            executor,
            native: Mutex::new(Some(LocalNativeStream::Reader(native))),
            state: Mutex::new(WorkerStreamState {
                commands: HashMap::new(),
            }),
            queue: Mutex::new(VecDeque::new()),
            native_write_ids: Mutex::new(HashMap::new()),
            next_internal_request_id: std::sync::atomic::AtomicU64::new(1 << 63),
            active: AtomicBool::new(false),
            cleanup_permit: Mutex::new(Some(cleanup_permit)),
            _buffer_budget: buffer_budget,
            _handle_budget: handle_budget,
            observer: None,
            written_bytes: std::sync::atomic::AtomicU64::new(0),
            append_mode: false,
        }));
        #[cfg(panic = "unwind")]
        crate::operation::poison_test_mutex(&driver.0.state);
        #[cfg(panic = "unwind")]
        crate::operation::poison_test_mutex(&driver.0.queue);
        let request = LocalStreamRequest {
            id: 1,
            command: LocalStreamCommand::Read { max_bytes: 1 },
        };
        let mut cx = Context::from_waker(Waker::noop());
        let result = driver.poll_command(&mut cx, &request);
        release_tx.send(()).unwrap();
        assert!(matches!(result, std::task::Poll::Ready(Err(ref error)) if error.kind() == io::ErrorKind::WouldBlock));
    }

    #[test]
    fn test_terminal_close_uses_reserved_cleanup_when_worker_queue_is_full() {
        let manager = OperationManager::new(ManagerOptions::default()).unwrap();
        let executor = LocalIoExecutor::new(
            manager,
            LocalExecutionConfig {
                worker_count: 1,
                queue_capacity: 1,
                waiter_capacity: 1,
                max_open_handles: 2,
                ..Default::default()
            },
        )
        .unwrap();
        let (started_tx, started_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        executor
            .enqueue_task(move || {
                started_tx.send(()).unwrap();
                release_rx.recv().unwrap();
            })
            .unwrap();
        started_rx.recv().unwrap();
        executor.enqueue_task(|| {}).unwrap();
        executor.enqueue_task(|| {}).unwrap();

        let fixture = tempfile::NamedTempFile::new().unwrap();
        let native = crate::LocalFileReader::from_file(fixture.reopen().unwrap()).unwrap();
        let buffer_budget = Arc::new(Mutex::new(executor.reserve_bytes(0).unwrap()));
        let handle_budget = Arc::new(executor.reserve_handle().unwrap());
        let cleanup_permit = executor.reserve_cleanup().unwrap();
        let driver = WorkerStreamDriver(Arc::new(WorkerStreamInner {
            executor,
            native: Mutex::new(Some(LocalNativeStream::Reader(native))),
            state: Mutex::new(WorkerStreamState {
                commands: HashMap::new(),
            }),
            queue: Mutex::new(VecDeque::new()),
            native_write_ids: Mutex::new(HashMap::new()),
            next_internal_request_id: std::sync::atomic::AtomicU64::new(1 << 63),
            active: AtomicBool::new(false),
            cleanup_permit: Mutex::new(Some(cleanup_permit)),
            _buffer_budget: buffer_budget,
            _handle_budget: handle_budget,
            observer: None,
            written_bytes: std::sync::atomic::AtomicU64::new(0),
            append_mode: false,
        }));
        #[cfg(panic = "unwind")]
        crate::operation::poison_test_mutex(&driver.0.native);
        #[cfg(panic = "unwind")]
        crate::operation::poison_test_mutex(&driver.0.cleanup_permit);
        let request = LocalStreamRequest {
            id: 9,
            command: LocalStreamCommand::Close,
        };
        let mut cx = Context::from_waker(Waker::noop());
        assert!(matches!(
            driver.poll_command(&mut cx, &request),
            std::task::Poll::Pending
        ));
        let used_cleanup_lane = driver
            .0
            .cleanup_permit
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .is_none();
        release_tx.send(()).unwrap();
        assert!(used_cleanup_lane, "terminal close uses its reserved cleanup slot");

        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
        loop {
            match driver.poll_command(&mut cx, &request) {
                std::task::Poll::Ready(Ok(super::super::LocalStreamCompletion::Closed)) => break,
                std::task::Poll::Ready(Err(error)) => panic!("terminal close failed: {error}"),
                std::task::Poll::Ready(Ok(other)) => panic!("unexpected completion: {other:?}"),
                std::task::Poll::Pending => {
                    assert!(std::time::Instant::now() < deadline, "terminal close did not finish");
                    std::thread::sleep(std::time::Duration::from_millis(1));
                }
            }
        }
        assert!(
            driver
                .0
                .native
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .is_none()
        );
    }

    #[test]
    fn test_terminal_close_records_failure_when_all_worker_lanes_are_full() {
        let manager = OperationManager::new(ManagerOptions::default()).unwrap();
        let executor = LocalIoExecutor::new(
            manager,
            LocalExecutionConfig {
                worker_count: 1,
                queue_capacity: 1,
                waiter_capacity: 1,
                max_open_handles: 2,
                ..Default::default()
            },
        )
        .unwrap();
        let mut held_cleanup = Vec::new();
        while let Some(permit) = executor.reserve_cleanup() {
            held_cleanup.push(permit);
        }
        let (started_tx, started_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        executor
            .enqueue_task(move || {
                started_tx.send(()).unwrap();
                release_rx.recv().unwrap();
            })
            .unwrap();
        started_rx.recv().unwrap();
        executor.enqueue_task(|| {}).unwrap();
        executor.enqueue_task(|| {}).unwrap();
        let fixture = tempfile::NamedTempFile::new().unwrap();
        let native = crate::LocalFileReader::from_file(fixture.reopen().unwrap()).unwrap();
        let driver = WorkerStreamDriver(Arc::new(WorkerStreamInner {
            executor: executor.clone(),
            native: Mutex::new(Some(LocalNativeStream::Reader(native))),
            state: Mutex::new(WorkerStreamState {
                commands: HashMap::new(),
            }),
            queue: Mutex::new(VecDeque::new()),
            native_write_ids: Mutex::new(HashMap::new()),
            next_internal_request_id: std::sync::atomic::AtomicU64::new(1 << 63),
            active: AtomicBool::new(false),
            cleanup_permit: Mutex::new(None),
            _buffer_budget: Arc::new(Mutex::new(executor.reserve_bytes(0).unwrap())),
            _handle_budget: Arc::new(executor.reserve_handle().unwrap()),
            observer: None,
            written_bytes: std::sync::atomic::AtomicU64::new(0),
            append_mode: false,
        }));

        LocalStreamDriver::request_close(&driver, 17);
        let state = driver.0.state.lock().unwrap();
        let result = state.commands.get(&17).and_then(|command| command.result.as_ref());
        release_tx.send(()).unwrap();
        assert!(matches!(result, Some(Err(error)) if error.kind() == io::ErrorKind::WouldBlock));
        drop(held_cleanup);
    }

    #[test]
    fn test_managed_rejects_when_cleanup_capacity_is_reserved() {
        let manager = OperationManager::new(ManagerOptions::default()).unwrap();
        let executor = LocalIoExecutor::new(
            manager.clone(),
            LocalExecutionConfig {
                max_open_handles: 1,
                ..Default::default()
            },
        )
        .unwrap();
        let held_cleanup = executor.reserve_cleanup().unwrap();
        let buffer_budget = Arc::new(Mutex::new(executor.reserve_bytes(0).unwrap()));
        let handle_budget = Arc::new(executor.reserve_handle().unwrap());
        let fixture = tempfile::NamedTempFile::new().unwrap();
        std::fs::write(fixture.path(), b"stream").unwrap();
        let native = crate::LocalFileReader::from_file(fixture.reopen().unwrap()).unwrap();
        let executor_for_operation = executor.clone();
        let (result_tx, result_rx) = mpsc::channel();
        let mut operation = manager
            .try_submit(Default::default(), move |scope| {
                let result = WorkerStreamDriver::managed(
                    &scope,
                    executor_for_operation,
                    LocalNativeStream::Reader(native),
                    buffer_budget,
                    handle_budget,
                    None,
                    false,
                );
                result_tx
                    .send(result.err().expect("cleanup capacity must reject stream").kind())
                    .unwrap();
                Ok::<_, ()>(())
            })
            .unwrap();
        assert_eq!(result_rx.recv().unwrap(), io::ErrorKind::WouldBlock);
        assert!(matches!(
            operation.wait_outcome().unwrap(),
            qubit_operation::OperationOutcome::Succeeded(())
        ));
        drop(held_cleanup);
    }

    #[test]
    fn test_request_close_does_not_duplicate_a_pending_command() {
        let manager = OperationManager::new(ManagerOptions::default()).unwrap();
        let executor = LocalIoExecutor::new(manager, LocalExecutionConfig::default()).unwrap();

        let fixture = tempfile::NamedTempFile::new().unwrap();
        let native = crate::LocalFileReader::from_file(fixture.reopen().unwrap()).unwrap();
        let buffer_budget = Arc::new(Mutex::new(executor.reserve_bytes(0).unwrap()));
        let handle_budget = Arc::new(executor.reserve_handle().unwrap());
        let cleanup_permit = executor.reserve_cleanup().unwrap();
        let driver = WorkerStreamDriver(Arc::new(WorkerStreamInner {
            executor,
            native: Mutex::new(Some(LocalNativeStream::Reader(native))),
            state: Mutex::new(WorkerStreamState {
                commands: HashMap::from([(
                    7,
                    super::super::worker_stream_command::WorkerCommand {
                        result: None,
                        waker: None,
                    },
                )]),
            }),
            queue: Mutex::new(VecDeque::new()),
            native_write_ids: Mutex::new(HashMap::new()),
            next_internal_request_id: std::sync::atomic::AtomicU64::new(1 << 63),
            active: AtomicBool::new(false),
            cleanup_permit: Mutex::new(Some(cleanup_permit)),
            _buffer_budget: buffer_budget,
            _handle_budget: handle_budget,
            observer: None,
            written_bytes: std::sync::atomic::AtomicU64::new(0),
            append_mode: false,
        }));

        LocalStreamDriver::request_close(&driver, 7);

        let state = driver.0.state.lock().unwrap();
        assert_eq!(state.commands.len(), 1);
        assert!(state.commands.get(&7).unwrap().result.is_none());
        assert!(driver.0.queue.lock().unwrap().is_empty());
    }

    #[test]
    fn test_stopped_worker_returns_close_error_and_retains_native_stream() {
        let manager = OperationManager::new(ManagerOptions::default()).expect("manager");
        let executor = LocalIoExecutor::new(manager, LocalExecutionConfig::default()).expect("executor");
        let fixture = tempfile::NamedTempFile::new().expect("temporary file");
        let native =
            crate::LocalFileReader::from_file(fixture.reopen().expect("reopen fixture")).expect("native reader");
        let driver = WorkerStreamDriver(Arc::new(WorkerStreamInner {
            native: Mutex::new(Some(LocalNativeStream::Reader(native))),
            state: Mutex::new(WorkerStreamState {
                commands: HashMap::new(),
            }),
            queue: Mutex::new(VecDeque::new()),
            native_write_ids: Mutex::new(HashMap::new()),
            next_internal_request_id: std::sync::atomic::AtomicU64::new(1 << 63),
            active: AtomicBool::new(false),
            cleanup_permit: Mutex::new(Some(executor.reserve_cleanup().expect("cleanup permit"))),
            _buffer_budget: Arc::new(Mutex::new(executor.reserve_bytes(0).expect("buffer permit"))),
            _handle_budget: Arc::new(executor.reserve_handle().expect("handle permit")),
            observer: None,
            written_bytes: std::sync::atomic::AtomicU64::new(0),
            append_mode: false,
            executor: executor.clone(),
        }));
        executor.inner.queue.stop();
        #[cfg(panic = "unwind")]
        crate::operation::poison_test_mutex(&driver.0.cleanup_permit);

        let request = LocalStreamRequest {
            id: 1,
            command: LocalStreamCommand::Close,
        };
        let mut cx = Context::from_waker(Waker::noop());
        assert!(matches!(
            driver.poll_command(&mut cx, &request),
            std::task::Poll::Ready(Err(error)) if error.kind() == io::ErrorKind::WouldBlock
        ));
        assert!(driver.0.native.lock().expect("native lock").is_some());
        let permit = driver
            .0
            .cleanup_permit
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        assert!(permit.is_some());
    }

    #[test]
    fn test_request_close_records_error_when_all_scheduling_lanes_are_full() {
        let manager = OperationManager::new(ManagerOptions::default()).unwrap();
        let executor = LocalIoExecutor::new(
            manager,
            LocalExecutionConfig {
                worker_count: 1,
                queue_capacity: 1,
                waiter_capacity: 1,
                max_open_handles: 1,
                ..Default::default()
            },
        )
        .unwrap();
        let held_cleanup = executor.reserve_cleanup().unwrap();
        let (started_tx, started_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        executor
            .enqueue_task(move || {
                started_tx.send(()).unwrap();
                release_rx.recv().unwrap();
            })
            .unwrap();
        started_rx.recv().unwrap();
        executor.enqueue_task(|| {}).unwrap();
        executor.enqueue_task(|| {}).unwrap();

        let fixture = tempfile::NamedTempFile::new().unwrap();
        let native = crate::LocalFileReader::from_file(fixture.reopen().unwrap()).unwrap();
        let buffer_budget = Arc::new(Mutex::new(executor.reserve_bytes(0).unwrap()));
        let handle_budget = Arc::new(executor.reserve_handle().unwrap());
        let driver = WorkerStreamDriver(Arc::new(WorkerStreamInner {
            executor,
            native: Mutex::new(Some(LocalNativeStream::Reader(native))),
            state: Mutex::new(WorkerStreamState {
                commands: HashMap::new(),
            }),
            queue: Mutex::new(VecDeque::new()),
            native_write_ids: Mutex::new(HashMap::new()),
            next_internal_request_id: std::sync::atomic::AtomicU64::new(1 << 63),
            active: std::sync::atomic::AtomicBool::new(false),
            cleanup_permit: Mutex::new(None),
            _buffer_budget: buffer_budget,
            _handle_budget: handle_budget,
            observer: None,
            written_bytes: std::sync::atomic::AtomicU64::new(0),
            append_mode: false,
        }));

        #[cfg(panic = "unwind")]
        crate::operation::poison_test_mutex(&driver.0.state);
        LocalStreamDriver::request_close(&driver, 11);
        release_tx.send(()).unwrap();

        let result_kind = {
            let state = driver.0.state.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            match state
                .commands
                .get(&11)
                .unwrap()
                .result
                .as_ref()
                .expect("failed request_close must retain its scheduling error")
            {
                Err(error) => error.kind(),
                Ok(_) => panic!("failed request_close unexpectedly succeeded"),
            }
        };
        assert_eq!(result_kind, io::ErrorKind::WouldBlock);
        assert!(driver.0.queue.lock().unwrap().is_empty());
        assert!(!driver.0.active.load(std::sync::atomic::Ordering::Acquire));
        assert!(driver.0.native.lock().unwrap().is_some());

        drop(held_cleanup);
    }
}
