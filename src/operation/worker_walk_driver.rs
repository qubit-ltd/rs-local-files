// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::collections::HashMap;
use std::io;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::Ordering;
use std::task::Context;
use std::task::Poll;

use qubit_operation::ResourceRef;

use super::LocalWalkDriver;
use super::LocalWalkResource;
use super::WorkerCleanup;
use super::worker_walk_command::WorkerWalkCommand;
use super::worker_walk_cursor::WorkerWalkCursor;
use super::worker_walk_inner::WorkerWalkInner;

/// Worker-backed batch driver for one native directory iterator.
pub(crate) struct WorkerWalkDriver(
    /// Shared iterator, request table, and cleanup state for this driver.
    Arc<WorkerWalkInner>,
);

impl WorkerWalkDriver {
    /// Registers a directory iterator and its budgets with the operation scope.
    ///
    /// # Parameters
    ///
    /// * `scope` - Operation scope that owns iterator cleanup.
    /// * `executor` - Serial I/O executor used by this walker.
    /// * `walker` - Exclusive native iterator to be advanced by the worker.
    /// * `buffer_budget` - Reservation retained for path batches.
    /// * `handle_budget` - Reservation retained while the iterator is open.
    pub(crate) fn managed(
        scope: &qubit_operation::OperationScope,
        executor: super::super::LocalIoExecutor,
        walker: crate::LocalDirectoryWalker,
        buffer_budget: Arc<Mutex<super::super::internal::permits::Permit>>,
        handle_budget: Arc<super::super::internal::permits::Permit>,
    ) -> io::Result<ResourceRef<LocalWalkResource>> {
        let cleanup_permit = executor
            .reserve_cleanup()
            .ok_or_else(|| io::Error::new(io::ErrorKind::WouldBlock, "local cleanup capacity is full"))?;
        let cleanup = WorkerCleanup::<LocalWalkResource>::policy();
        let resource = LocalWalkResource::new(Self(Arc::new(WorkerWalkInner {
            executor,
            walker: Mutex::new(WorkerWalkCursor {
                walker: Some(walker),
                pending_entry: None,
            }),
            commands: Mutex::new(HashMap::new()),
            stop_requested: AtomicBool::new(false),
            closed: AtomicBool::new(false),
            close_error: Mutex::new(None),
            close_waker: Mutex::new(None),
            cleanup_permit: Mutex::new(Some(cleanup_permit)),
            _buffer_budget: buffer_budget,
            _handle_budget: handle_budget,
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
                WorkerCleanup::<LocalWalkResource>::dispose(resource);
                Err(io::Error::other(format!("failed to register local walker: {reason:?}")))
            }
        }
    }
}

impl LocalWalkDriver for WorkerWalkDriver {
    /// Advances the iterator once for the stable request and returns its batch.
    ///
    /// # Parameters
    ///
    /// * `cx` - Task context to wake when the worker produces a batch.
    /// * `request_id` - Stable identifier reused while polling the same batch.
    /// * `max_entries` - Maximum entries allowed in the returned batch.
    /// * `max_bytes` - Maximum encoded path bytes allowed in the batch.
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
                    let mut cursor = inner.walker.lock().unwrap_or_else(|e| e.into_inner());
                    if cursor.walker.is_none() {
                        return Ok(None);
                    }
                    let mut entries = Vec::new();
                    let mut total_bytes = 0usize;
                    for _ in 0..max_entries {
                        let next_entry = if let Some(path) = cursor.pending_entry.take() {
                            Some(Ok(path))
                        } else {
                            match cursor.walker.as_mut().expect("walker retained").next() {
                                Some(Ok(entry)) => Some(Ok(entry.path().to_path_buf())),
                                Some(Err(error)) => Some(Err(error)),
                                None => None,
                            }
                        };
                        match next_entry {
                            None => {
                                cursor.walker.take();
                                break;
                            }
                            Some(Err(error)) => return Err(io::Error::other(error)),
                            Some(Ok(path)) => {
                                let size = path.as_os_str().as_encoded_bytes().len();
                                if size > max_bytes {
                                    if entries.is_empty() {
                                        // Return the single oversized path so the public
                                        // bounded-batch validator reports the resource limit.
                                        entries.push(path);
                                    } else {
                                        cursor.pending_entry = Some(path);
                                    }
                                    break;
                                }
                                let Some(next_total) = total_bytes.checked_add(size) else {
                                    cursor.pending_entry = Some(path);
                                    break;
                                };
                                if next_total > max_bytes {
                                    cursor.pending_entry = Some(path);
                                    break;
                                }
                                total_bytes = next_total;
                                entries.push(path);
                            }
                        }
                    }
                    if entries.is_empty() && cursor.walker.is_none() {
                        Ok(None)
                    } else {
                        Ok(Some(entries))
                    }
                }))
                .unwrap_or_else(|_| Err(io::Error::other("Unconfirmed: local walker worker panicked")));
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

    /// Requests that the worker stop traversal and close the native iterator.
    fn request_stop(&self) {
        if self.0.stop_requested.swap(true, Ordering::AcqRel) {
            return;
        }
        let inner = Arc::clone(&self.0);
        let job: Box<dyn FnOnce() + Send> = Box::new(move || {
            inner.walker.lock().unwrap_or_else(|e| e.into_inner()).walker.take();
            inner.closed.store(true, Ordering::Release);
            if let Some(waker) = inner.close_waker.lock().unwrap_or_else(|e| e.into_inner()).take() {
                let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| waker.wake()));
            }
        });
        if let Some(permit) = self.0.cleanup_permit.lock().unwrap_or_else(|e| e.into_inner()).take() {
            let mut permit = Some(permit);
            let mut job = Some(job);
            let mut queued = false;
            for _ in 0..8 {
                match self.0.executor.enqueue_cleanup(
                    job.take().expect("cleanup job retained"),
                    permit.take().expect("cleanup permit retained"),
                ) {
                    Ok(()) => {
                        queued = true;
                        break;
                    }
                    Err((retry_job, retry_permit)) => {
                        job = Some(retry_job);
                        permit = Some(retry_permit);
                    }
                }
            }
            if !queued {
                *self.0.close_error.lock().unwrap_or_else(|e| e.into_inner()) = Some((
                    io::ErrorKind::WouldBlock,
                    "reserved local walker cleanup queue is unavailable".to_owned(),
                ));
                *self.0.cleanup_permit.lock().unwrap_or_else(|e| e.into_inner()) = permit;
            }
        }
    }

    /// Waits for confirmation that the native iterator has been closed.
    ///
    /// # Parameters
    ///
    /// * `cx` - Task context to wake when native closure completes.
    fn poll_closed(&self, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        if let Some((kind, message)) = self.0.close_error.lock().unwrap_or_else(|e| e.into_inner()).clone() {
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
