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
                                        // Return the single oversized path so
                                        // the public bounded-batch validator
                                        // reports the resource limit.
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
        let cleanup_permit = self.0.cleanup_permit.lock().unwrap_or_else(|e| e.into_inner()).take();
        if let Some(permit) = cleanup_permit {
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

#[cfg(test)]
mod tests {
    use std::collections::HashMap;
    use std::io;
    use std::path::PathBuf;
    use std::sync::Arc;
    use std::sync::Mutex;
    use std::sync::atomic::AtomicBool;
    use std::task::Context;
    use std::task::Poll;
    use std::task::Wake;
    use std::task::Waker;

    use qubit_operation::ManagerOptions;
    use qubit_operation::OperationManager;
    use qubit_operation::OperationOutcome;

    use super::LocalWalkDriver;
    use super::WorkerWalkCursor;
    use super::WorkerWalkDriver;
    use super::WorkerWalkInner;
    use crate::LocalListOptions;
    use crate::operation::AsyncLocalFileSystem;
    use crate::operation::LocalExecutionConfig;
    use crate::operation::LocalIoExecutor;
    use crate::operation::internal::permits::PermitPool;

    fn direct_driver(directory: &tempfile::TempDir, executor: LocalIoExecutor) -> WorkerWalkDriver {
        let filesystem = crate::LocalFileSystem::rooted(directory.path()).unwrap();
        let walker = filesystem
            .list_with_options(&PathBuf::from("."), &LocalListOptions::default())
            .unwrap();
        let cleanup_permit = executor.reserve_cleanup().unwrap();
        let buffer_budget = Arc::new(Mutex::new(PermitPool::new(1).try_acquire(1).unwrap()));
        let handle_budget = Arc::new(PermitPool::new(1).try_acquire(1).unwrap());
        WorkerWalkDriver(Arc::new(WorkerWalkInner {
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
        }))
    }

    async fn poll_batch(
        driver: &WorkerWalkDriver,
        request_id: u64,
        max_entries: usize,
        max_bytes: usize,
    ) -> io::Result<Option<Vec<PathBuf>>> {
        std::future::poll_fn(|cx| driver.poll_batch(cx, request_id, max_entries, max_bytes)).await
    }

    async fn poll_closed(driver: &WorkerWalkDriver) -> io::Result<()> {
        std::future::poll_fn(|cx| driver.poll_closed(cx)).await
    }

    fn stop_driver(driver: &WorkerWalkDriver) {
        driver.request_stop();
        block_on(poll_closed(driver)).unwrap();
    }

    fn block_on<F: std::future::Future>(future: F) -> F::Output {
        struct ThreadWake(std::thread::Thread);
        impl Wake for ThreadWake {
            fn wake(self: Arc<Self>) {
                self.0.unpark();
            }
            fn wake_by_ref(self: &Arc<Self>) {
                self.0.unpark();
            }
        }

        let waker = Waker::from(Arc::new(ThreadWake(std::thread::current())));
        let mut context = Context::from_waker(&waker);
        let mut future = std::pin::pin!(future);
        loop {
            match future.as_mut().poll(&mut context) {
                Poll::Ready(value) => return value,
                Poll::Pending => std::thread::park(),
            }
        }
    }

    #[test]
    fn test_managed_walker_yields_all_entries_then_reports_end_of_stream() {
        let directory = tempfile::tempdir().unwrap();
        let expected: Vec<_> = (0..130).map(|index| format!("entry-{index:03}")).collect();
        for name in &expected {
            std::fs::write(directory.path().join(name), name).unwrap();
        }

        let manager = OperationManager::new(ManagerOptions::default()).unwrap();
        let executor = LocalIoExecutor::new(manager.clone(), LocalExecutionConfig::default()).unwrap();
        let filesystem = block_on(AsyncLocalFileSystem::rooted(executor, directory.path().to_path_buf())).unwrap();
        let mut operation = filesystem
            .submit_open_walker(
                &manager,
                Default::default(),
                PathBuf::from("."),
                LocalListOptions::default(),
            )
            .unwrap();
        let mut walker = match operation.wait_outcome().unwrap() {
            OperationOutcome::Succeeded(walker) => walker,
            _ => panic!("expected the managed walker to open"),
        };

        let mut paths = Vec::new();
        let mut batch_sizes = Vec::new();
        while let Some(batch) = block_on(walker.next_batch()).unwrap() {
            batch_sizes.push(batch.len());
            paths.extend(batch);
        }
        let mut names: Vec<_> = paths.iter().map(|path| path.file_name().unwrap().to_owned()).collect();
        names.sort();
        assert_eq!(
            names,
            expected.into_iter().map(std::ffi::OsString::from).collect::<Vec<_>>()
        );
        assert_eq!(
            batch_sizes,
            [128, 2],
            "the entry limit should split the final two paths into a second batch"
        );
        operation.wait_settlement().unwrap();
    }

    #[test]
    fn test_managed_walker_close_stops_iteration_and_confirms_native_cleanup() {
        let directory = tempfile::tempdir().unwrap();
        for index in 0..4 {
            std::fs::write(directory.path().join(format!("entry-{index}")), b"x").unwrap();
        }

        let manager = OperationManager::new(ManagerOptions::default()).unwrap();
        let executor = LocalIoExecutor::new(manager.clone(), LocalExecutionConfig::default()).unwrap();
        let filesystem = block_on(AsyncLocalFileSystem::rooted(executor, directory.path().to_path_buf())).unwrap();
        let mut operation = filesystem
            .submit_open_walker(
                &manager,
                Default::default(),
                PathBuf::from("."),
                LocalListOptions::default(),
            )
            .unwrap();
        let mut walker = match operation.wait_outcome().unwrap() {
            OperationOutcome::Succeeded(walker) => walker,
            _ => panic!("expected the managed walker to open"),
        };

        assert!(block_on(walker.next_batch()).unwrap().is_some());
        let observer = walker.close_observer();
        block_on(walker.close()).unwrap();
        block_on(observer.wait_closed()).unwrap();
        drop(observer);
        operation.wait_settlement().unwrap();
    }

    #[test]
    fn test_managed_walker_empty_directory_returns_end_of_stream() {
        let directory = tempfile::tempdir().unwrap();
        let manager = OperationManager::new(ManagerOptions::default()).unwrap();
        let executor = LocalIoExecutor::new(manager.clone(), LocalExecutionConfig::default()).unwrap();
        let filesystem = block_on(AsyncLocalFileSystem::rooted(executor, directory.path().to_path_buf())).unwrap();
        let mut operation = filesystem
            .submit_open_walker(
                &manager,
                Default::default(),
                PathBuf::from("."),
                LocalListOptions::default(),
            )
            .unwrap();
        let mut walker = match operation.wait_outcome().unwrap() {
            OperationOutcome::Succeeded(walker) => walker,
            _ => panic!("expected the managed walker to open"),
        };

        assert_eq!(block_on(walker.next_batch()).unwrap(), None);
        operation.wait_settlement().unwrap();
    }

    #[test]
    fn test_managed_walker_close_before_first_batch_confirms_cleanup() {
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(directory.path().join("unread-entry"), b"x").unwrap();

        let manager = OperationManager::new(ManagerOptions::default()).unwrap();
        let executor = LocalIoExecutor::new(manager.clone(), LocalExecutionConfig::default()).unwrap();
        let filesystem = block_on(AsyncLocalFileSystem::rooted(executor, directory.path().to_path_buf())).unwrap();
        let mut operation = filesystem
            .submit_open_walker(
                &manager,
                Default::default(),
                PathBuf::from("."),
                LocalListOptions::default(),
            )
            .unwrap();
        let mut walker = match operation.wait_outcome().unwrap() {
            OperationOutcome::Succeeded(walker) => walker,
            _ => panic!("expected the managed walker to open"),
        };

        let observer = walker.close_observer();
        block_on(walker.close()).unwrap();
        block_on(observer.wait_closed()).unwrap();
        drop(observer);
        operation.wait_settlement().unwrap();
    }

    #[test]
    fn test_managed_walker_drop_requests_cleanup_and_observer_confirms_close() {
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(directory.path().join("unread-entry"), b"x").unwrap();

        let manager = OperationManager::new(ManagerOptions::default()).unwrap();
        let executor = LocalIoExecutor::new(manager.clone(), LocalExecutionConfig::default()).unwrap();
        let filesystem = block_on(AsyncLocalFileSystem::rooted(executor, directory.path().to_path_buf())).unwrap();
        let mut operation = filesystem
            .submit_open_walker(
                &manager,
                Default::default(),
                PathBuf::from("."),
                LocalListOptions::default(),
            )
            .unwrap();
        let walker = match operation.wait_outcome().unwrap() {
            OperationOutcome::Succeeded(walker) => walker,
            _ => panic!("expected the managed walker to open"),
        };

        let observer = walker.close_observer();
        drop(walker);
        block_on(observer.wait_closed()).unwrap();
        drop(observer);
        operation.wait_settlement().unwrap();
    }

    #[test]
    fn test_worker_walk_driver_carries_entry_across_byte_limited_batches() {
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(directory.path().join("first"), b"x").unwrap();
        std::fs::write(directory.path().join("other"), b"x").unwrap();

        let manager = OperationManager::new(ManagerOptions::default()).unwrap();
        let executor = LocalIoExecutor::new(manager, LocalExecutionConfig::default()).unwrap();
        let driver = direct_driver(&directory, executor);
        #[cfg(panic = "unwind")]
        {
            use crate::operation::poison_test_mutex;

            poison_test_mutex(&driver.0.commands);
            poison_test_mutex(&driver.0.walker);
            poison_test_mutex(&driver.0.close_error);
            poison_test_mutex(&driver.0.close_waker);
            poison_test_mutex(&driver.0.cleanup_permit);
        }
        let byte_limit = PathBuf::from("first").as_os_str().as_encoded_bytes().len();
        let first = block_on(poll_batch(&driver, 41, 8, byte_limit)).unwrap().unwrap();
        assert_eq!(first.len(), 1, "the byte limit should admit one equal-sized path");
        let second = block_on(poll_batch(&driver, 42, 8, byte_limit)).unwrap().unwrap();
        assert_eq!(
            second.len(),
            1,
            "the carried path should be returned on the next request"
        );
        let names = [first[0].file_name().unwrap(), second[0].file_name().unwrap()];
        assert!(names.contains(&std::ffi::OsStr::new("first")));
        assert!(names.contains(&std::ffi::OsStr::new("other")));
        stop_driver(&driver);
    }

    #[test]
    fn test_worker_walk_driver_returns_one_path_larger_than_byte_budget() {
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(directory.path().join("oversized"), b"x").unwrap();

        let manager = OperationManager::new(ManagerOptions::default()).unwrap();
        let executor = LocalIoExecutor::new(manager, LocalExecutionConfig::default()).unwrap();
        let driver = direct_driver(&directory, executor);
        let batch = block_on(poll_batch(&driver, 43, 8, 0)).unwrap().unwrap();
        assert_eq!(batch.len(), 1, "a first path cannot be split across batches");
        assert!(!batch[0].as_os_str().as_encoded_bytes().is_empty());
        stop_driver(&driver);
    }

    #[test]
    fn test_walker_is_exclusive_and_cleanup_capacity_is_required_for_admission() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let manager = OperationManager::new(ManagerOptions::default()).expect("manager");
        let executor = LocalIoExecutor::new(
            manager.clone(),
            LocalExecutionConfig {
                max_open_handles: 1,
                ..Default::default()
            },
        )
        .expect("executor");
        let driver = direct_driver(&directory, executor.clone());
        let walker = crate::LocalFileSystem::rooted(directory.path())
            .expect("rooted filesystem")
            .list_with_options(std::path::Path::new("."), &LocalListOptions::new())
            .expect("native walker");
        let port = super::LocalWalkResource::new(driver);
        port.claim().expect("first handle claims the walker");
        assert_eq!(
            port.claim().expect_err("second claim must fail").kind(),
            io::ErrorKind::AlreadyExists
        );

        let outcome = manager
            .run(Default::default(), move |scope| {
                let buffer_budget = Arc::new(Mutex::new(executor.reserve_bytes(0).expect("buffer permit")));
                let handle_budget = Arc::new(executor.reserve_handle().expect("handle permit"));
                let result = WorkerWalkDriver::managed(&scope, executor, walker, buffer_budget, handle_budget);
                assert_eq!(
                    result.err().expect("cleanup permit already in use").kind(),
                    io::ErrorKind::WouldBlock
                );
                Ok::<(), ()>(())
            })
            .expect("run operation");
        assert!(matches!(outcome, OperationOutcome::Succeeded(())));
        port.request_stop();
        block_on(std::future::poll_fn(|cx| port.poll_closed(cx))).expect("first walker closes");
    }

    #[test]
    fn test_stopped_worker_rejects_batch_without_consuming_walker() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let manager = OperationManager::new(ManagerOptions::default()).expect("manager");
        let executor = LocalIoExecutor::new(manager, LocalExecutionConfig::default()).expect("executor");
        let driver = direct_driver(&directory, executor.clone());
        executor.inner.queue.stop();
        #[cfg(panic = "unwind")]
        crate::operation::poison_test_mutex(&driver.0.commands);

        let error = block_on(poll_batch(&driver, 1, 1, 1024)).expect_err("stopped queue rejects traversal");
        assert_eq!(error.kind(), io::ErrorKind::WouldBlock);
        let commands = driver
            .0
            .commands
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        assert!(commands.is_empty());
        assert!(driver.0.walker.lock().expect("walker lock").walker.is_some());
        assert!(driver.0.cleanup_permit.lock().expect("permit lock").is_some());
    }

    #[test]
    fn test_stopped_worker_reports_cleanup_rejection_without_losing_walker() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let manager = OperationManager::new(ManagerOptions::default()).expect("manager");
        let executor = LocalIoExecutor::new(manager, LocalExecutionConfig::default()).expect("executor");
        let driver = direct_driver(&directory, executor.clone());
        executor.inner.queue.stop();
        #[cfg(panic = "unwind")]
        {
            use crate::operation::poison_test_mutex;

            poison_test_mutex(&driver.0.close_error);
            poison_test_mutex(&driver.0.cleanup_permit);
        }

        let (finished, completion) = std::sync::mpsc::channel();
        let stopper = std::thread::spawn(move || {
            driver.request_stop();
            finished.send(driver).expect("test is waiting for stop to return");
        });
        let driver = completion
            .recv_timeout(std::time::Duration::from_secs(5))
            .expect("stop must return when the cleanup queue rejects every attempt");
        stopper.join().expect("stop thread should complete");

        let mut context = Context::from_waker(Waker::noop());
        let Poll::Ready(Err(error)) = driver.poll_closed(&mut context) else {
            panic!("close must report rejection immediately");
        };
        assert_eq!(error.kind(), io::ErrorKind::WouldBlock);
        assert_eq!(error.to_string(), "reserved local walker cleanup queue is unavailable");
        assert!(driver.0.walker.lock().expect("walker lock").walker.is_some());
        assert!(
            driver
                .0
                .cleanup_permit
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .is_some()
        );
    }

    #[test]
    fn test_worker_walk_driver_reports_close_error() {
        let directory = tempfile::tempdir().unwrap();
        let manager = OperationManager::new(ManagerOptions::default()).unwrap();
        let executor = LocalIoExecutor::new(manager, LocalExecutionConfig::default()).unwrap();
        let driver = direct_driver(&directory, executor);
        driver
            .0
            .close_error
            .lock()
            .unwrap()
            .replace((io::ErrorKind::WouldBlock, "cleanup unavailable".into()));
        let error = block_on(poll_closed(&driver)).unwrap_err();
        assert_eq!(error.kind(), io::ErrorKind::WouldBlock);
        assert_eq!(error.to_string(), "cleanup unavailable");
        driver.0.walker.lock().unwrap().walker.take();
    }

    #[test]
    fn test_managed_walker_operation_cancellation_closes_native_iterator() {
        let directory = tempfile::tempdir().unwrap();
        std::fs::write(directory.path().join("unread-entry"), b"x").unwrap();

        let manager = OperationManager::new(ManagerOptions::default()).unwrap();
        let executor = LocalIoExecutor::new(manager.clone(), LocalExecutionConfig::default()).unwrap();
        let filesystem = block_on(AsyncLocalFileSystem::rooted(executor, directory.path().to_path_buf())).unwrap();
        let mut operation = filesystem
            .submit_open_walker(
                &manager,
                Default::default(),
                PathBuf::from("."),
                LocalListOptions::default(),
            )
            .unwrap();
        let walker = match operation.wait_outcome().unwrap() {
            OperationOutcome::Succeeded(walker) => walker,
            _ => panic!("expected the managed walker to open"),
        };
        let observer = walker.close_observer();

        operation.request_stop(qubit_operation::StopReason::UserCancelled);
        drop(walker);
        block_on(observer.wait_closed()).unwrap();
        drop(observer);
        operation.wait_settlement().unwrap();
    }

    #[test]
    fn test_managed_walker_reports_full_worker_queue_without_blocking() {
        use std::sync::mpsc;
        use std::time::Duration;

        struct ReleaseOnDrop(Option<mpsc::Sender<()>>);
        impl Drop for ReleaseOnDrop {
            fn drop(&mut self) {
                if let Some(sender) = self.0.take() {
                    let _ = sender.send(());
                }
            }
        }

        let directory = tempfile::tempdir().unwrap();
        std::fs::write(directory.path().join("entry"), b"x").unwrap();
        let manager = OperationManager::new(ManagerOptions::default()).unwrap();
        let executor = LocalIoExecutor::new(
            manager.clone(),
            LocalExecutionConfig {
                worker_count: 1,
                queue_capacity: 1,
                waiter_capacity: 1,
                ..Default::default()
            },
        )
        .unwrap();
        let filesystem = block_on(AsyncLocalFileSystem::rooted(
            executor.clone(),
            directory.path().to_path_buf(),
        ))
        .unwrap();
        let mut operation = filesystem
            .submit_open_walker(
                &manager,
                Default::default(),
                PathBuf::from("."),
                LocalListOptions::default(),
            )
            .unwrap();
        let mut walker = match operation.wait_outcome().unwrap() {
            OperationOutcome::Succeeded(walker) => walker,
            _ => panic!("expected the managed walker to open"),
        };

        let (started_tx, started_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let release_guard = ReleaseOnDrop(Some(release_tx));
        executor
            .enqueue_task(move || {
                started_tx.send(()).unwrap();
                release_rx.recv().unwrap();
            })
            .unwrap();
        started_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        executor.enqueue_task(|| {}).unwrap();
        executor.enqueue_task(|| {}).unwrap();

        let error = block_on(walker.next_batch()).expect_err("a saturated worker queue must reject traversal");
        assert_eq!(error.kind(), crate::LocalFileErrorKind::Io);
        assert!(error.to_string().contains("local worker queue is full"));

        drop(release_guard);
        block_on(walker.close()).unwrap();
        operation.wait_settlement().unwrap();
    }
}
