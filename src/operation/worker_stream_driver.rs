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
use super::start_stream_drain;
use super::worker_stream_command::WorkerCommand;
use super::worker_stream_inner::WorkerStreamInner;
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
                *reference.resource_id.lock().unwrap_or_else(|e| e.into_inner()) = Some(reference.id());
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
        let should_start = {
            let mut queue = self.0.queue.lock().unwrap_or_else(|e| e.into_inner());
            queue.push_back(request.clone());
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
    use super::LocalStreamCommand;
    use super::LocalStreamDriver;
    use super::LocalStreamRequest;
    use super::WorkerStreamDriver;
    use super::WorkerStreamInner;
    use super::WorkerStreamState;
    use crate::LocalExecutionConfig;
    use crate::LocalIoExecutor;

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
            active: AtomicBool::new(false),
            cleanup_permit: Mutex::new(Some(cleanup_permit)),
            _buffer_budget: buffer_budget,
            _handle_budget: handle_budget,
            observer: None,
            written_bytes: std::sync::atomic::AtomicU64::new(0),
            append_mode: false,
        }));
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
            active: AtomicBool::new(false),
            cleanup_permit: Mutex::new(Some(cleanup_permit)),
            _buffer_budget: buffer_budget,
            _handle_budget: handle_budget,
            observer: None,
            written_bytes: std::sync::atomic::AtomicU64::new(0),
            append_mode: false,
        }));
        let request = LocalStreamRequest {
            id: 9,
            command: LocalStreamCommand::Close,
        };
        let mut cx = Context::from_waker(Waker::noop());
        assert!(matches!(
            driver.poll_command(&mut cx, &request),
            std::task::Poll::Pending
        ));
        release_tx.send(()).unwrap();

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
        assert!(driver.0.native.lock().unwrap().is_none());
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
            active: std::sync::atomic::AtomicBool::new(false),
            cleanup_permit: Mutex::new(None),
            _buffer_budget: buffer_budget,
            _handle_budget: handle_budget,
            observer: None,
            written_bytes: std::sync::atomic::AtomicU64::new(0),
            append_mode: false,
        }));

        LocalStreamDriver::request_close(&driver, 11);
        release_tx.send(()).unwrap();

        let result_kind = {
            let state = driver.0.state.lock().unwrap();
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
