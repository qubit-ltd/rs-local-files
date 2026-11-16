// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

mod executor_inner;
mod local_io_executor;
mod worker_owner;

pub use local_io_executor::LocalIoExecutor;
pub(super) use local_io_executor::spawn_workers;

use super::LocalExecutionConfig;

#[cfg(test)]
mod tests {
    use std::io;
    use std::sync::mpsc;
    use std::time::Duration;

    use qubit_operation::ManagerOptions;
    use qubit_operation::OperationManager;
    use qubit_operation::OperationOutcome;

    use super::LocalExecutionConfig;
    use super::LocalIoExecutor;
    use super::spawn_workers;
    use crate::operation::AsyncLocalFileSystem;
    use crate::operation::LocalFactObserver;
    use crate::operation::internal::prepared_local_job::PreparedLocalJob;
    use crate::operation::internal::worker::WorkerQueue;

    /// Runs the make job behavior while preserving the operation lifecycle
    /// invariants.

    /// # Type Parameters
    /// - `F` — type used by this operation.

    /// # Parameters
    /// - `executor` — supplies data or policy for this operation.
    /// - `task` — supplies data or policy for this operation.
    fn make_job<F>(executor: &LocalIoExecutor, task: F) -> std::sync::Arc<PreparedLocalJob<(), (), ()>>
    where
        F: FnOnce(()) -> OperationOutcome<(), ()> + Send + 'static,
    {
        let slot = executor.reserve_slot().unwrap();
        make_job_with_slot(executor, slot, task)
    }

    /// Runs the make job with slot behavior while preserving the operation
    /// lifecycle invariants.

    /// # Type Parameters
    /// - `F` — type used by this operation.

    /// # Parameters
    /// - `executor` — supplies data or policy for this operation.
    /// - `slot` — supplies data or policy for this operation.
    /// - `task` — supplies data or policy for this operation.
    fn make_job_with_slot<F>(
        executor: &LocalIoExecutor,
        slot: crate::operation::internal::permits::Permit,
        task: F,
    ) -> std::sync::Arc<PreparedLocalJob<(), (), ()>>
    where
        F: FnOnce(()) -> OperationOutcome<(), ()> + Send + 'static,
    {
        PreparedLocalJob::new(
            std::sync::Arc::clone(&executor.inner),
            (),
            slot,
            std::sync::Arc::new(std::sync::Mutex::new(executor.reserve_bytes(0).unwrap())),
            executor.reserve_handle().unwrap(),
            LocalFactObserver::new(),
            task,
        )
    }

    /// Runs the test partial worker start failure stops and joins started
    /// workers behavior while preserving the operation lifecycle invariants.
    #[test]
    fn test_partial_worker_start_failure_stops_and_joins_started_workers() {
        let queue = std::sync::Arc::new(WorkerQueue::default());
        let (joined_tx, joined_rx) = mpsc::channel();
        let mut calls = 0;
        let result = spawn_workers(&queue, 3, |_, worker_queue| {
            calls += 1;
            if calls == 1 {
                let joined_tx = joined_tx.clone();
                std::thread::Builder::new().spawn(move || {
                    worker_queue.run();
                    joined_tx.send(()).unwrap();
                })
            } else {
                Err(io::Error::other("injected thread start failure"))
            }
        });

        assert!(result.is_err());
        assert_eq!(calls, 2);
        joined_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("started worker must exit before start failure is returned");
    }

    /// Runs the test activate queue failure leaves typed outcome to manager and
    /// releases permits behavior while preserving the operation lifecycle
    /// invariants.
    #[test]
    fn test_activate_queue_failure_leaves_typed_outcome_to_manager_and_releases_permits() {
        use std::task::Context;
        use std::task::Wake;
        use std::task::Waker;

        /// Represents noop within the local operation subsystem.
        struct Noop;
        impl Wake for Noop {
            /// Runs the wake behavior while preserving the operation lifecycle
            /// invariants.
            fn wake(self: std::sync::Arc<Self>) {}
        }

        let manager = OperationManager::new(ManagerOptions::default()).unwrap();
        let executor = LocalIoExecutor::new(manager.clone(), LocalExecutionConfig::default()).unwrap();
        let owned = std::sync::Arc::new(std::sync::Mutex::new(executor.reserve_bytes(7).unwrap()));
        let owned_for_task = std::sync::Arc::clone(&owned);
        let fact = std::sync::Arc::new(executor.reserve_fact().unwrap());
        let prepared = PreparedLocalJob::new(
            std::sync::Arc::clone(&executor.inner),
            (),
            executor.reserve_slot().unwrap(),
            owned,
            executor.reserve_handle().unwrap(),
            LocalFactObserver::with_permit(fact),
            move |_| {
                drop(owned_for_task);
                OperationOutcome::<(), ()>::Succeeded(())
            },
        );
        executor.inner.queue.stop();
        let mut handle = manager
            .try_submit_external(Default::default(), std::sync::Arc::clone(&prepared))
            .unwrap();
        assert!(matches!(
            handle.wait_outcome().unwrap(),
            OperationOutcome::ExecutionFailed(_)
        ));
        handle.wait_settlement().unwrap();
        let waker = Waker::from(std::sync::Arc::new(Noop));
        let mut context = Context::from_waker(&waker);
        assert!(
            matches!(
                qubit_operation::PreparedExternalOperation::poll_take_outcome(&*prepared, &mut context,),
                std::task::Poll::Pending
            ),
            "manager owns the sole typed ExecutionFailed outcome"
        );
        assert!(qubit_operation::OperationControl::poll_release(&*prepared, &mut context).is_ready());
        assert!(qubit_operation::OperationControl::poll_settlement(&*prepared, &mut context).is_ready());
        assert_eq!(executor.inner.queue_slots.used(), 0);
        assert_eq!(executor.inner.byte_slots.used(), 0);
        assert_eq!(executor.inner.handle_slots.used(), 0);
        assert_eq!(executor.inner.fact_slots.used(), 0);
        assert!(qubit_operation::OperationControl::snapshot(&*prepared).input_released);
    }

    /// Runs the test reentrant waker can observe control without deadlocking
    /// behavior while preserving the operation lifecycle invariants.
    #[test]
    fn test_reentrant_waker_can_observe_control_without_deadlocking() {
        use std::task::Context;
        use std::task::Wake;
        use std::task::Waker;

        /// Represents reentrant within the local operation subsystem.
        struct Reentrant {
            /// Stores the prepared associated with this value.
            prepared: std::sync::Arc<PreparedLocalJob<(), (), ()>>,
            /// Stores the notified associated with this value.
            notified: mpsc::Sender<()>,
        }
        impl Wake for Reentrant {
            /// Runs the wake behavior while preserving the operation lifecycle
            /// invariants.
            fn wake(self: std::sync::Arc<Self>) {
                self.wake_by_ref();
            }
            /// Runs the wake by ref behavior while preserving the operation
            /// lifecycle invariants.
            fn wake_by_ref(self: &std::sync::Arc<Self>) {
                let _ = qubit_operation::OperationControl::snapshot(&*self.prepared);
                let _ = self.notified.send(());
            }
        }

        let manager = OperationManager::new(ManagerOptions::default()).unwrap();
        let executor = LocalIoExecutor::new(manager.clone(), LocalExecutionConfig::default()).unwrap();
        let prepared = make_job(&executor, |_| OperationOutcome::Succeeded(()));
        let prepared_for_parent = std::sync::Arc::clone(&prepared);
        let manager_for_thread = manager.clone();
        let (done_tx, done_rx) = mpsc::channel();
        let thread = std::thread::spawn(move || {
            let mut parent = manager_for_thread
                .try_submit(Default::default(), move |scope| {
                    qubit_operation::PreparedExternalOperation::bind(&*prepared_for_parent, scope.id()).unwrap();
                    let (wake_tx, wake_rx) = mpsc::channel();
                    let waker = Waker::from(std::sync::Arc::new(Reentrant {
                        prepared: std::sync::Arc::clone(&prepared_for_parent),
                        notified: wake_tx,
                    }));
                    let mut context = Context::from_waker(&waker);
                    assert!(
                        qubit_operation::OperationControl::poll_outcome(&*prepared_for_parent, &mut context,)
                            .is_pending()
                    );
                    qubit_operation::OperationControl::request_stop(
                        &*prepared_for_parent,
                        qubit_operation::StopReason::UserCancelled,
                    );
                    wake_rx.recv_timeout(Duration::from_secs(5)).unwrap();
                    Ok::<_, ()>(())
                })
                .unwrap();
            assert!(matches!(
                parent.wait_outcome().unwrap(),
                OperationOutcome::Succeeded(())
            ));
            done_tx.send(()).unwrap();
        });
        done_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("reentrant Waker callback should complete outside the control lock");
        thread.join().unwrap();
    }

    /// Runs the test panicking waker does not unwind or corrupt control state
    /// behavior while preserving the operation lifecycle invariants.
    #[test]
    fn test_panicking_waker_does_not_unwind_or_corrupt_control_state() {
        use std::task::Context;
        use std::task::Wake;
        use std::task::Waker;

        /// Represents panics within the local operation subsystem.
        struct Panics;
        impl Wake for Panics {
            /// Runs the wake behavior while preserving the operation lifecycle
            /// invariants.
            fn wake(self: std::sync::Arc<Self>) {
                panic!("injected Waker panic");
            }
            /// Runs the wake by ref behavior while preserving the operation
            /// lifecycle invariants.
            fn wake_by_ref(self: &std::sync::Arc<Self>) {
                panic!("injected Waker panic");
            }
        }

        let manager = OperationManager::new(ManagerOptions::default()).unwrap();
        let executor = LocalIoExecutor::new(manager.clone(), LocalExecutionConfig::default()).unwrap();
        let prepared = make_job(&executor, |_| OperationOutcome::Succeeded(()));
        let prepared_for_parent = std::sync::Arc::clone(&prepared);
        let mut parent = manager
            .try_submit(Default::default(), move |scope| {
                qubit_operation::PreparedExternalOperation::bind(&*prepared_for_parent, scope.id()).unwrap();
                let waker = Waker::from(std::sync::Arc::new(Panics));
                let mut context = Context::from_waker(&waker);
                assert!(
                    qubit_operation::OperationControl::poll_outcome(&*prepared_for_parent, &mut context,).is_pending()
                );
                qubit_operation::OperationControl::request_stop(
                    &*prepared_for_parent,
                    qubit_operation::StopReason::UserCancelled,
                );
                assert_eq!(
                    qubit_operation::OperationControl::snapshot(&*prepared_for_parent)
                        .stop
                        .map(|report| report.reason),
                    Some(qubit_operation::StopReason::UserCancelled)
                );
                Ok::<_, ()>(())
            })
            .unwrap();
        assert!(matches!(
            parent.wait_outcome().unwrap(),
            OperationOutcome::Succeeded(())
        ));
    }

    /// Runs the test abandon releases all unstarted permits even with retained
    /// arc behavior while preserving the operation lifecycle invariants.
    #[test]
    fn test_abandon_releases_all_unstarted_permits_even_with_retained_arc() {
        let manager = OperationManager::new(ManagerOptions::default()).unwrap();
        let executor = LocalIoExecutor::new(manager.clone(), LocalExecutionConfig::default()).unwrap();
        let owned = std::sync::Arc::new(std::sync::Mutex::new(executor.reserve_bytes(9).unwrap()));
        let owned_for_task = std::sync::Arc::clone(&owned);
        let observer_permit = std::sync::Arc::new(executor.reserve_fact().unwrap());
        let prepared = PreparedLocalJob::new(
            std::sync::Arc::clone(&executor.inner),
            (),
            executor.reserve_slot().unwrap(),
            owned,
            executor.reserve_handle().unwrap(),
            LocalFactObserver::with_permit(observer_permit),
            move |_| {
                drop(owned_for_task);
                OperationOutcome::<(), ()>::Succeeded(())
            },
        );
        let prepared_for_parent = std::sync::Arc::clone(&prepared);
        let executor_for_parent = executor.clone();
        let mut parent = manager
            .try_submit(Default::default(), move |scope| {
                qubit_operation::PreparedExternalOperation::bind(&*prepared_for_parent, scope.id()).unwrap();
                qubit_operation::PreparedExternalOperation::abandon(std::sync::Arc::clone(&prepared_for_parent));
                assert_eq!(executor_for_parent.inner.queue_slots.used(), 0);
                assert_eq!(executor_for_parent.inner.byte_slots.used(), 0);
                assert_eq!(executor_for_parent.inner.handle_slots.used(), 0);
                assert_eq!(executor_for_parent.inner.fact_slots.used(), 0);
                Ok::<_, ()>(())
            })
            .unwrap();
        assert!(matches!(
            parent.wait_outcome().unwrap(),
            OperationOutcome::Succeeded(())
        ));
        assert!(qubit_operation::OperationControl::snapshot(&*prepared).input_released);
    }

    /// Runs the test local operation keeps result budget until user takes
    /// outcome behavior while preserving the operation lifecycle invariants.
    #[test]
    fn test_local_operation_keeps_result_budget_until_user_takes_outcome() {
        let manager = OperationManager::new(ManagerOptions::default()).unwrap();
        let executor = LocalIoExecutor::new(manager.clone(), LocalExecutionConfig::default()).unwrap();
        let permit = std::sync::Arc::new(std::sync::Mutex::new(executor.reserve_bytes(8).unwrap()));
        let handle = manager
            .try_submit(Default::default(), |_| Ok::<_, ()>(vec![1_u8; 4]))
            .unwrap();
        let mut local = crate::operation::LocalOperation::new(handle, permit.clone());
        drop(permit);
        local.wait_settlement().unwrap();
        assert_eq!(executor.inner.byte_slots.used(), 8);
        let outcome = local.wait_outcome().unwrap();
        assert!(matches!(outcome, OperationOutcome::Succeeded(bytes) if bytes == [1, 1, 1, 1]));
        assert_eq!(executor.inner.byte_slots.used(), 0);
    }

    /// Runs the test small read budgets actual capacity until take or drop
    /// behavior while preserving the operation lifecycle invariants.
    #[test]
    fn test_small_read_budgets_actual_capacity_until_take_or_drop() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("small");
        std::fs::write(&path, b"tiny").unwrap();
        let manager = OperationManager::new(ManagerOptions::default()).unwrap();
        let executor = LocalIoExecutor::new(
            manager.clone(),
            LocalExecutionConfig {
                max_owned_bytes: 64,
                ..LocalExecutionConfig::default()
            },
        )
        .unwrap();
        let filesystem = crate::LocalFileSystem::host()
            .unwrap()
            .into_async(executor.clone())
            .unwrap();
        let (mut operation, observer) = filesystem
            .submit_read_all_with_observer(
                &manager,
                Default::default(),
                crate::operation::LocalReadRequest {
                    path: path.clone(),
                    read: Default::default(),
                },
                Default::default(),
            )
            .unwrap();
        operation.wait_settlement().unwrap();
        let used_before_take = executor.inner.byte_slots.used();
        assert!(
            used_before_take > 0 && used_before_take < 64,
            "small result must not reserve the whole executor pool"
        );
        assert_eq!(executor.inner.fact_slots.used(), 1);
        let outcome = operation.wait_outcome().unwrap();
        let actual_capacity = match outcome {
            OperationOutcome::Succeeded(bytes) => bytes.capacity(),
            other => panic!("unexpected read outcome: {other:?}"),
        };
        assert_eq!(used_before_take, actual_capacity);
        assert_eq!(executor.inner.byte_slots.used(), 0);
        drop(observer);
        assert_eq!(executor.inner.fact_slots.used(), 0);

        let (operation, observer) = filesystem
            .submit_read_all_with_observer(
                &manager,
                Default::default(),
                crate::operation::LocalReadRequest {
                    path,
                    read: Default::default(),
                },
                Default::default(),
            )
            .unwrap();
        operation.wait_settlement().unwrap();
        assert!(executor.inner.byte_slots.used() > 0);
        drop(operation);
        assert_eq!(
            executor.inner.byte_slots.used(),
            0,
            "drop discards the unclaimed upstream result before returning its budget"
        );
        assert_eq!(executor.inner.fact_slots.used(), 1);
        drop(observer);
        assert_eq!(executor.inner.fact_slots.used(), 0);
    }

    /// Runs the test async constructor poll never waits for blocked worker io
    /// behavior while preserving the operation lifecycle invariants.
    #[test]
    fn test_async_constructor_poll_never_waits_for_blocked_worker_io() {
        use std::future::Future;
        use std::task::Context;
        use std::task::Poll;
        use std::task::Wake;
        use std::task::Waker;
        /// Represents noop within the local operation subsystem.
        struct Noop;
        impl Wake for Noop {
            /// Runs the wake behavior while preserving the operation lifecycle
            /// invariants.
            fn wake(self: std::sync::Arc<Self>) {}
        }

        let manager = OperationManager::new(ManagerOptions::default()).unwrap();
        let executor = LocalIoExecutor::new(
            manager,
            LocalExecutionConfig {
                worker_count: 1,
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
        started_rx.recv_timeout(Duration::from_secs(5)).unwrap();

        let scratch = tempfile::tempdir().unwrap();
        let root = scratch.path().to_path_buf();
        let mut rooted = Box::pin(AsyncLocalFileSystem::rooted(executor.clone(), root));
        let waker = Waker::from(std::sync::Arc::new(Noop));
        let mut context = Context::from_waker(&waker);
        assert!(matches!(rooted.as_mut().poll(&mut context), Poll::Pending));
        drop(rooted);
        let mut host = Box::pin(AsyncLocalFileSystem::host(executor.clone()));
        assert!(matches!(host.as_mut().poll(&mut context), Poll::Pending));
        drop(host);

        let (complete_tx, complete_rx) = mpsc::channel();
        executor.enqueue_task(move || complete_tx.send(()).unwrap()).unwrap();
        release_tx.send(()).unwrap();
        complete_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    }

    /// Runs the test cleanup and ordinary queues make round robin progress
    /// behavior while preserving the operation lifecycle invariants.
    #[test]
    fn test_cleanup_and_ordinary_queues_make_round_robin_progress() {
        let queue = std::sync::Arc::new(WorkerQueue::default());
        let worker_queue = std::sync::Arc::clone(&queue);
        let worker = std::thread::spawn(move || worker_queue.run());
        let (started_tx, started_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let (order_tx, order_rx) = mpsc::channel();
        assert!(
            queue
                .push(
                    Box::new(move || {
                        started_tx.send(()).unwrap();
                        release_rx.recv().unwrap();
                    }),
                    true,
                    4,
                )
                .is_ok()
        );
        started_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        let cleanup_order = order_tx.clone();
        assert!(
            queue
                .push(Box::new(move || cleanup_order.send("cleanup").unwrap()), true, 4,)
                .is_ok()
        );
        assert!(
            queue
                .push(Box::new(move || order_tx.send("ordinary").unwrap()), false, 4,)
                .is_ok()
        );
        release_tx.send(()).unwrap();
        assert_eq!(order_rx.recv_timeout(Duration::from_secs(5)).unwrap(), "ordinary");
        assert_eq!(order_rx.recv_timeout(Duration::from_secs(5)).unwrap(), "cleanup");
        queue.stop();
        worker.join().unwrap();
    }

    /// Runs the test queue and waiter permits bound blocked jobs and return on
    /// release behavior while preserving the operation lifecycle invariants.
    #[test]
    fn test_queue_and_waiter_permits_bound_blocked_jobs_and_return_on_release() {
        let manager = OperationManager::new(ManagerOptions::default()).unwrap();
        let executor = LocalIoExecutor::new(
            manager.clone(),
            LocalExecutionConfig {
                worker_count: 1,
                queue_capacity: 1,
                waiter_capacity: 1,
                ..LocalExecutionConfig::default()
            },
        )
        .unwrap();
        let (started_tx, started_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let first = make_job(&executor, move |_| {
            started_tx.send(()).unwrap();
            release_rx.recv().unwrap();
            OperationOutcome::Succeeded(())
        });
        let mut first_handle = manager
            .try_submit_external(Default::default(), std::sync::Arc::clone(&first))
            .unwrap();
        started_rx.recv_timeout(Duration::from_secs(5)).unwrap();

        let queued_slot = executor
            .reserve_slot()
            .expect("queue capacity available while first job runs");
        let waiter_slot = executor.reserve_slot().expect("bounded waiter capacity is available");
        assert!(
            executor.reserve_slot().is_none(),
            "all queue and waiter reservations are held"
        );
        let queued = make_job_with_slot(&executor, queued_slot, |_| OperationOutcome::Succeeded(()));
        let waiting = make_job_with_slot(&executor, waiter_slot, |_| OperationOutcome::Succeeded(()));
        let mut queued_handle = manager.try_submit_external(Default::default(), queued).unwrap();
        let mut waiting_handle = manager.try_submit_external(Default::default(), waiting).unwrap();

        release_tx.send(()).unwrap();
        assert!(matches!(
            first_handle.wait_outcome().unwrap(),
            OperationOutcome::Succeeded(())
        ));
        assert!(matches!(
            queued_handle.wait_outcome().unwrap(),
            OperationOutcome::Succeeded(())
        ));
        assert!(matches!(
            waiting_handle.wait_outcome().unwrap(),
            OperationOutcome::Succeeded(())
        ));
        first_handle.wait_settlement().unwrap();
        queued_handle.wait_settlement().unwrap();
        waiting_handle.wait_settlement().unwrap();
        assert!(executor.reserve_slot().is_some());
        assert!(executor.reserve_slot().is_some());
    }

    /// Runs the test stop after bind before activate prevents task from running
    /// behavior while preserving the operation lifecycle invariants.
    #[test]
    fn test_stop_after_bind_before_activate_prevents_task_from_running() {
        use std::sync::atomic::AtomicUsize;
        use std::sync::atomic::Ordering;
        use std::task::Context;
        use std::task::Wake;
        use std::task::Waker;

        /// Represents notify within the local operation subsystem.
        struct Notify(std::sync::mpsc::Sender<()>);
        impl Wake for Notify {
            /// Runs the wake behavior while preserving the operation lifecycle
            /// invariants.
            fn wake(self: std::sync::Arc<Self>) {
                let _ = self.0.send(());
            }
            /// Runs the wake by ref behavior while preserving the operation
            /// lifecycle invariants.
            fn wake_by_ref(self: &std::sync::Arc<Self>) {
                let _ = self.0.send(());
            }
        }

        let manager = OperationManager::new(ManagerOptions::default()).unwrap();
        let executor = LocalIoExecutor::new(manager.clone(), LocalExecutionConfig::default()).unwrap();
        let executions = std::sync::Arc::new(AtomicUsize::new(0));
        let executions_for_parent = std::sync::Arc::clone(&executions);
        let mut parent = manager
            .try_submit(Default::default(), move |scope| {
                let count = std::sync::Arc::clone(&executions_for_parent);
                let prepared = make_job(&executor, move |_| {
                    count.fetch_add(1, Ordering::SeqCst);
                    OperationOutcome::Succeeded(())
                });
                let id = scope.id();
                qubit_operation::PreparedExternalOperation::bind(&*prepared, id).unwrap();
                qubit_operation::OperationControl::request_stop(&*prepared, qubit_operation::StopReason::UserCancelled);
                qubit_operation::PreparedExternalOperation::activate(std::sync::Arc::clone(&prepared), scope).unwrap();

                let (wake_tx, wake_rx) = std::sync::mpsc::channel();
                let waker = Waker::from(std::sync::Arc::new(Notify(wake_tx)));
                let mut context = Context::from_waker(&waker);
                loop {
                    match qubit_operation::PreparedExternalOperation::poll_take_outcome(&*prepared, &mut context) {
                        std::task::Poll::Ready(qubit_operation::OperationOutcome::Stopped(_)) => {
                            break;
                        }
                        std::task::Poll::Ready(_) => {
                            panic!("stopped prepared task produced a non-stopped outcome")
                        }
                        std::task::Poll::Pending => wake_rx.recv_timeout(Duration::from_secs(5)).unwrap(),
                    }
                }
                Ok::<_, ()>(())
            })
            .unwrap();
        let _ = parent.wait_outcome().unwrap();
        assert_eq!(executions.load(Ordering::SeqCst), 0);
    }

    /// Runs the test disk permits are finite and return when released behavior
    /// while preserving the operation lifecycle invariants.
    #[test]
    fn test_disk_permits_are_finite_and_return_when_released() {
        let manager = OperationManager::new(ManagerOptions::default()).unwrap();
        let executor = LocalIoExecutor::new(
            manager,
            LocalExecutionConfig {
                max_disk_bytes: 8,
                ..Default::default()
            },
        )
        .unwrap();
        let permit = executor.reserve_disk(8).unwrap();
        assert!(executor.reserve_disk(1).is_none());
        drop(permit);
        assert_eq!(executor.reserve_disk(8).unwrap().amount(), 8);
    }

    /// Runs the test cleanup permit quota and rejected enqueue preserve job and
    /// permit behavior while preserving the operation lifecycle invariants.
    #[test]
    fn test_cleanup_permit_quota_and_rejected_enqueue_preserve_job_and_permit() {
        use std::sync::atomic::AtomicUsize;
        use std::sync::atomic::Ordering;

        let manager = OperationManager::new(ManagerOptions::default()).unwrap();
        let executor = LocalIoExecutor::new(
            manager,
            LocalExecutionConfig {
                max_open_handles: 1,
                ..Default::default()
            },
        )
        .unwrap();
        let permit = executor.reserve_cleanup().unwrap();
        assert!(executor.reserve_cleanup().is_none());

        executor.inner.queue.stop();
        let calls = std::sync::Arc::new(AtomicUsize::new(0));
        let calls_for_job = std::sync::Arc::clone(&calls);
        let result = executor.enqueue_cleanup(
            Box::new(move || {
                calls_for_job.fetch_add(1, Ordering::SeqCst);
            }),
            permit,
        );
        let (job, permit) = result.expect_err("stopped queue must reject cleanup");
        assert_eq!(executor.inner.cleanup_slots.used(), 1);
        job();
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        drop(permit);
        assert_eq!(executor.inner.cleanup_slots.used(), 0);
    }

    /// Runs the test successful cleanup runs and releases reserved slot
    /// behavior while preserving the operation lifecycle invariants.
    #[test]
    fn test_successful_cleanup_runs_and_releases_reserved_slot() {
        let manager = OperationManager::new(ManagerOptions::default()).unwrap();
        let executor = LocalIoExecutor::new(manager, LocalExecutionConfig::default()).unwrap();
        let permit = executor.reserve_cleanup().unwrap();
        let (done_tx, done_rx) = mpsc::channel();
        if executor
            .enqueue_cleanup(Box::new(move || done_tx.send(()).unwrap()), permit)
            .is_err()
        {
            panic!("cleanup enqueue should succeed with a reserved permit");
        }
        executor.inner.queue.stop();
        for worker in executor
            ._owner
            .workers
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .drain(..)
        {
            worker.join().unwrap();
        }
        done_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(executor.inner.cleanup_slots.used(), 0);
        assert!(executor.reserve_cleanup().is_some());
    }

    /// Runs the test dropping executor drains queued work and stops queue
    /// behavior while preserving the operation lifecycle invariants.
    #[test]
    fn test_dropping_executor_drains_queued_work_and_stops_queue() {
        let manager = OperationManager::new(ManagerOptions::default()).unwrap();
        let executor = LocalIoExecutor::new(
            manager,
            LocalExecutionConfig {
                worker_count: 1,
                queue_capacity: 2,
                ..Default::default()
            },
        )
        .unwrap();
        let queue = std::sync::Arc::clone(&executor.inner.queue);
        let (started_tx, started_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        executor
            .enqueue_task(move || {
                started_tx.send(()).unwrap();
                release_rx.recv().unwrap();
            })
            .unwrap();
        started_rx.recv_timeout(Duration::from_secs(5)).unwrap();

        let (events_tx, events_rx) = mpsc::channel();
        executor
            .enqueue_task({
                let events_tx = events_tx.clone();
                move || events_tx.send("queued work drained").unwrap()
            })
            .unwrap();
        let drop_thread = std::thread::spawn(move || {
            drop(executor);
            events_tx.send("executor dropped").unwrap();
        });
        release_tx.send(()).unwrap();
        assert_eq!(
            events_rx.recv_timeout(Duration::from_secs(5)).unwrap(),
            "queued work drained"
        );
        assert_eq!(
            events_rx.recv_timeout(Duration::from_secs(5)).unwrap(),
            "executor dropped"
        );
        drop_thread.join().unwrap();
        assert!(queue.push(Box::new(|| {}), false, 1).is_err());
    }

    /// Runs the test duplicate arc submission is rejected without abandoning
    /// claimed job behavior while preserving the operation lifecycle
    /// invariants.
    #[test]
    fn test_duplicate_arc_submission_is_rejected_without_abandoning_claimed_job() {
        let manager = OperationManager::new(ManagerOptions::default()).unwrap();
        let executor = LocalIoExecutor::new(manager.clone(), LocalExecutionConfig::default()).unwrap();
        let (started_tx, started_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let prepared = make_job(&executor, move |_| {
            started_tx.send(()).unwrap();
            release_rx.recv().unwrap();
            OperationOutcome::Succeeded(())
        });
        let mut accepted = manager
            .try_submit_external(Default::default(), std::sync::Arc::clone(&prepared))
            .unwrap();
        started_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        let rejected = manager.try_submit_external(Default::default(), std::sync::Arc::clone(&prepared));
        let error = match rejected {
            Ok(_) => panic!("duplicate prepared Arc was accepted"),
            Err(error) => error,
        };
        assert_eq!(error.reason(), qubit_operation::AdmissionError::AlreadyBound);
        release_tx.send(()).unwrap();
        assert!(matches!(
            accepted.wait_outcome().unwrap(),
            OperationOutcome::Succeeded(())
        ));
        accepted.wait_settlement().unwrap();
    }
}
