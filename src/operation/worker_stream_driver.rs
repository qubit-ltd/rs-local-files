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
    /// * `handle_budget` - Reservation retained while the native handle is open.
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
            let terminal = matches!(
                request.command,
                LocalStreamCommand::Close | LocalStreamCommand::Abort
            );
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
                    io::Error::new(
                        io::ErrorKind::WouldBlock,
                        "local stream cleanup scheduling failed",
                    )
                })));
                slot.waker.take()
            };
            if let Some(waker) = wake {
                let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| waker.wake()));
            }
        }
    }
}
