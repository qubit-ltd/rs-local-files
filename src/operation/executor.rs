// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

mod executor_inner;
mod local_cleanup_executor;
mod local_io_executor;
mod worker_owner;

pub(crate) use executor_inner::ExecutorInner;
pub(crate) use local_cleanup_executor::LocalCleanupExecutor;
pub use local_io_executor::LocalIoExecutor;

#[cfg(test)]
mod tests {
    use std::io;
    use std::sync::mpsc;
    use std::time::Duration;

    use qubit_operation::ManagerOptions;
    use qubit_operation::OperationControl;
    use qubit_operation::OperationManager;
    use qubit_operation::OperationOutcome;
    use qubit_operation::ShutdownMode;

    use super::LocalIoExecutor;
    use super::local_io_executor::spawn_workers;
    use crate::operation::AsyncLocalFileSystem;
    use crate::operation::LocalExecutionConfig;
    use crate::operation::LocalExecutorConfigError;
    use crate::operation::LocalFactObserver;
    use crate::operation::internal::prepared_local_job::PreparedLocalJob;
    use crate::operation::internal::worker::WorkerQueue;

    #[test]
    fn test_executor_config_rejects_each_zero_limit_and_exposes_shutdown() {
        let invalid_configs = [
            (
                LocalExecutionConfig {
                    worker_count: 0,
                    ..Default::default()
                },
                LocalExecutorConfigError::ZeroWorkers,
            ),
            (
                LocalExecutionConfig {
                    queue_capacity: 0,
                    ..Default::default()
                },
                LocalExecutorConfigError::ZeroQueueCapacity,
            ),
            (
                LocalExecutionConfig {
                    waiter_capacity: 0,
                    ..Default::default()
                },
                LocalExecutorConfigError::ZeroWaiterCapacity,
            ),
            (
                LocalExecutionConfig {
                    max_owned_bytes: 0,
                    ..Default::default()
                },
                LocalExecutorConfigError::ZeroOwnedBytes,
            ),
            (
                LocalExecutionConfig {
                    max_open_handles: 0,
                    ..Default::default()
                },
                LocalExecutorConfigError::ZeroOpenHandles,
            ),
            (
                LocalExecutionConfig {
                    max_disk_bytes: 0,
                    ..Default::default()
                },
                LocalExecutorConfigError::ZeroDiskBytes,
            ),
            (
                LocalExecutionConfig {
                    max_fact_records: 0,
                    ..Default::default()
                },
                LocalExecutorConfigError::ZeroFactRecords,
            ),
        ];

        for (config, expected) in invalid_configs {
            let manager = OperationManager::new(ManagerOptions::default()).unwrap();
            assert!(matches!(
                LocalIoExecutor::new(manager, config),
                Err(crate::operation::LocalExecutorCreateError::InvalidConfig(actual)) if actual == expected
            ));
        }

        let manager = OperationManager::new(ManagerOptions::default()).unwrap();
        let executor = LocalIoExecutor::new(manager, LocalExecutionConfig::default()).unwrap();
        assert_eq!(executor.config(), &LocalExecutionConfig::default());
        let report = executor.request_shutdown(ShutdownMode::Drain).wait().unwrap();
        assert_eq!(report.snapshot.active_operations, 0);
        assert_eq!(report.snapshot.queued_operations, 0);
        assert_eq!(report.snapshot.cleaning_resources, 0);
    }

    #[test]
    fn test_cancel_shutdown_closes_executor_group_without_stopping_manager_or_workers() {
        let manager = OperationManager::new(ManagerOptions::default()).unwrap();
        let executor = LocalIoExecutor::new(manager.clone(), LocalExecutionConfig::default()).unwrap();

        let report = executor.request_shutdown(ShutdownMode::Cancel).wait().unwrap();
        assert_eq!(report.snapshot.active_operations, 0);
        assert_eq!(report.snapshot.queued_operations, 0);
        assert_eq!(report.snapshot.cleaning_resources, 0);

        let mut manager_operation = manager.try_submit(Default::default(), |_| Ok::<_, ()>(())).unwrap();
        assert!(matches!(
            manager_operation.wait_outcome().unwrap(),
            OperationOutcome::Succeeded(())
        ));

        let (done_tx, done_rx) = mpsc::channel();
        executor.enqueue_task(move || done_tx.send(()).unwrap()).unwrap();
        done_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    }

    #[test]
    fn test_drain_shutdown_waits_for_accepted_operation_to_settle() {
        let manager = OperationManager::new(ManagerOptions::default()).unwrap();
        let executor = LocalIoExecutor::new(
            manager.clone(),
            LocalExecutionConfig {
                worker_count: 1,
                ..LocalExecutionConfig::default()
            },
        )
        .unwrap();
        let (started_tx, started_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let mut operation = executor
            .inner
            .group
            .try_submit(Default::default(), move |_| {
                started_tx.send(()).unwrap();
                release_rx.recv().unwrap();
                Ok::<_, ()>(())
            })
            .expect("operation is accepted before shutdown");
        started_rx.recv_timeout(Duration::from_secs(5)).unwrap();

        let shutdown = executor.request_shutdown(ShutdownMode::Drain);
        let (shutdown_tx, shutdown_rx) = mpsc::channel();
        let shutdown_thread = std::thread::spawn(move || {
            shutdown_tx.send(shutdown.wait().unwrap()).unwrap();
        });
        assert!(matches!(
            shutdown_rx.recv_timeout(Duration::from_millis(50)),
            Err(mpsc::RecvTimeoutError::Timeout)
        ));

        release_tx.send(()).unwrap();
        assert!(matches!(
            operation.wait_outcome().unwrap(),
            OperationOutcome::Succeeded(())
        ));
        let report = shutdown_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(report.snapshot.active_operations, 0);
        assert_eq!(report.snapshot.queued_operations, 0);
        shutdown_thread.join().unwrap();
    }

    #[test]
    fn test_prepared_jobs_preserve_typed_request_result_and_error() {
        let manager = OperationManager::new(ManagerOptions::default()).unwrap();
        let executor = LocalIoExecutor::new(manager.clone(), LocalExecutionConfig::default()).unwrap();

        let make_typed_job = |request: String, fail: bool| {
            PreparedLocalJob::new(
                std::sync::Arc::clone(&executor.inner),
                request,
                executor.reserve_slot().unwrap(),
                std::sync::Arc::new(std::sync::Mutex::new(executor.reserve_bytes(0).unwrap())),
                executor.reserve_handle().unwrap(),
                LocalFactObserver::new(),
                move |request| {
                    if fail {
                        OperationOutcome::Failed(crate::LocalFileError::new(
                            crate::LocalFileErrorKind::NotFound,
                            crate::LocalFileOperation::Read,
                        ))
                    } else {
                        OperationOutcome::Succeeded(request.into_bytes())
                    }
                },
            )
        };

        let success = make_typed_job("owned request".to_owned(), false);
        let control = std::sync::Arc::clone(&success);
        let mut success_handle = manager
            .try_submit_external(Default::default(), success)
            .expect("typed job admission");
        let id = success_handle.id();
        assert_eq!(control.snapshot().operation_id, id);
        assert!(matches!(
            success_handle.wait_outcome().unwrap(),
            OperationOutcome::Succeeded(bytes) if bytes == b"owned request"
        ));
        assert!(control.snapshot().input_released);

        let failure = make_typed_job("request retained until execution".to_owned(), true);
        let mut failure_handle = manager
            .try_submit_external(Default::default(), failure)
            .expect("typed job admission");
        assert!(matches!(
            failure_handle.wait_outcome().unwrap(),
            OperationOutcome::Failed(error)
                if error.kind() == crate::LocalFileErrorKind::NotFound
        ));

        macro_rules! assert_typed_success {
            ($request_type:ty, $result_type:ty, $request:expr, $result:expr) => {{
                let request = $request;
                let expected: $result_type = $result;
                let produced = expected.clone();
                let job = PreparedLocalJob::<$request_type, $result_type, crate::LocalFileError>::new(
                    std::sync::Arc::clone(&executor.inner),
                    request,
                    executor.reserve_slot().unwrap(),
                    std::sync::Arc::new(std::sync::Mutex::new(executor.reserve_bytes(0).unwrap())),
                    executor.reserve_handle().unwrap(),
                    LocalFactObserver::new(),
                    move |_request| OperationOutcome::Succeeded(produced),
                );
                let mut handle = manager
                    .try_submit_external(Default::default(), job)
                    .expect("typed job admission");
                assert!(matches!(
                    handle.wait_outcome().unwrap(),
                    OperationOutcome::Succeeded(actual) if actual == expected
                ));
            }};
        }

        assert_typed_success!(
            std::path::PathBuf,
            Vec<u8>,
            std::path::PathBuf::from("payload"),
            b"file contents".to_vec()
        );
        assert_typed_success!(Vec<u8>, usize, b"append payload".to_vec(), 14);
        assert_typed_success!(String, String, "temporary resource".to_owned(), "committed".to_owned());
    }

    #[test]
    fn test_prepared_job_rejects_mismatched_identity_and_consumed_request() {
        use qubit_operation::PreparedExternalOperation;

        let manager = OperationManager::new(ManagerOptions::default()).unwrap();
        let executor = LocalIoExecutor::new(manager.clone(), LocalExecutionConfig::default()).unwrap();
        let unrelated_id = manager
            .try_submit(Default::default(), |_| Ok::<_, ()>(()))
            .unwrap()
            .id();

        let identity_executor = executor.clone();
        let mut identity_check = manager
            .try_submit(Default::default(), move |scope| {
                let job = make_job(&identity_executor, |_| OperationOutcome::Succeeded(()));
                PreparedExternalOperation::bind(&*job, unrelated_id).unwrap();
                let failure = PreparedExternalOperation::activate(std::sync::Arc::clone(&job), scope)
                    .expect_err("a job cannot activate under an identity other than the one it was bound to");
                assert_eq!("local job identity mismatch", failure.summary.as_str());
                assert!(qubit_operation::OperationControl::snapshot(&*job).input_released);
                Ok::<_, ()>(())
            })
            .unwrap();
        assert!(matches!(
            identity_check.wait_outcome().unwrap(),
            OperationOutcome::Succeeded(())
        ));

        let mut request_check = manager
            .try_submit(Default::default(), move |scope| {
                let job = make_job(&executor, |_| OperationOutcome::Succeeded(()));
                PreparedExternalOperation::bind(&*job, scope.id()).unwrap();
                assert_eq!(Some(()), job.take_request());
                let failure = PreparedExternalOperation::activate(job, scope)
                    .expect_err("activation must reject a request already taken by its owner");
                assert_eq!("local request already taken", failure.summary.as_str());
                Ok::<_, ()>(())
            })
            .unwrap();
        assert!(matches!(
            request_check.wait_outcome().unwrap(),
            OperationOutcome::Succeeded(())
        ));
    }

    #[test]
    fn test_prepared_job_activation_is_one_shot_across_operation_scopes() {
        use qubit_operation::PreparedExternalOperation;

        let manager = OperationManager::new(ManagerOptions::default()).unwrap();
        let executor = LocalIoExecutor::new(manager.clone(), LocalExecutionConfig::default()).unwrap();
        let (retained_sender, retained_receiver) = std::sync::mpsc::channel();

        let mut first = manager
            .try_submit(Default::default(), move |scope| {
                let job = make_job(&executor, |_| OperationOutcome::Succeeded(()));
                PreparedExternalOperation::bind(&*job, scope.id()).expect("first bind succeeds");
                retained_sender
                    .send(std::sync::Arc::clone(&job))
                    .expect("parent retains the job for the retry");
                PreparedExternalOperation::activate(job, scope).expect("first activation succeeds");
                Ok::<_, ()>(())
            })
            .unwrap();

        let retained = retained_receiver.recv().expect("first scope published the job");
        let mut second = manager
            .try_submit(Default::default(), move |scope| {
                let failure = PreparedExternalOperation::activate(retained, scope)
                    .expect_err("activation cannot be retried under a second operation scope");
                assert_eq!("local job activation was already attempted", failure.summary.as_str());
                Ok::<_, ()>(())
            })
            .unwrap();

        assert!(matches!(first.wait_outcome().unwrap(), OperationOutcome::Succeeded(())));
        assert!(matches!(
            second.wait_outcome().unwrap(),
            OperationOutcome::Succeeded(())
        ));
    }

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
        use std::task::Waker;

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
        let mut context = Context::from_waker(Waker::noop());
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
    #[test]
    fn test_replacing_a_control_waker_wakes_only_the_latest_waiter() {
        use std::sync::Arc;
        use std::sync::atomic::AtomicUsize;
        use std::sync::atomic::Ordering;
        use std::task::Context;
        use std::task::Wake;
        use std::task::Waker;

        struct CountWake(Arc<AtomicUsize>);
        impl Wake for CountWake {
            fn wake(self: Arc<Self>) {
                self.wake_by_ref();
            }

            fn wake_by_ref(self: &Arc<Self>) {
                self.0.fetch_add(1, Ordering::SeqCst);
            }
        }

        let manager = OperationManager::new(ManagerOptions::default()).unwrap();
        let executor = LocalIoExecutor::new(manager.clone(), LocalExecutionConfig::default()).unwrap();
        let prepared = make_job(&executor, |_| OperationOutcome::Succeeded(()));
        let old_wakes = Arc::new(AtomicUsize::new(0));
        let latest_wakes = Arc::new(AtomicUsize::new(0));
        let prepared_for_parent = Arc::clone(&prepared);
        let old_wakes_for_parent = Arc::clone(&old_wakes);
        let latest_wakes_for_parent = Arc::clone(&latest_wakes);
        let mut parent = manager
            .try_submit(Default::default(), move |scope| {
                qubit_operation::PreparedExternalOperation::bind(&*prepared_for_parent, scope.id()).unwrap();

                let old_waker = Waker::from(Arc::new(CountWake(old_wakes_for_parent)));
                let mut old_context = Context::from_waker(&old_waker);
                assert!(
                    qubit_operation::OperationControl::poll_outcome(&*prepared_for_parent, &mut old_context,)
                        .is_pending()
                );
                assert!(
                    qubit_operation::OperationControl::poll_outcome(&*prepared_for_parent, &mut old_context,)
                        .is_pending()
                );

                let latest_waker = Waker::from(Arc::new(CountWake(latest_wakes_for_parent)));
                let mut latest_context = Context::from_waker(&latest_waker);
                assert!(
                    qubit_operation::OperationControl::poll_outcome(&*prepared_for_parent, &mut latest_context,)
                        .is_pending()
                );
                qubit_operation::OperationControl::request_stop(
                    &*prepared_for_parent,
                    qubit_operation::StopReason::UserCancelled,
                );
                Ok::<_, ()>(())
            })
            .unwrap();

        assert!(matches!(
            parent.wait_outcome().unwrap(),
            OperationOutcome::Succeeded(())
        ));
        assert_eq!(old_wakes.load(Ordering::SeqCst), 0);
        assert_eq!(latest_wakes.load(Ordering::SeqCst), 1);
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
        use std::task::Waker;

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
        let mut context = Context::from_waker(Waker::noop());
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

    #[test]
    fn test_queued_stop_skips_worker_and_worker_panic_becomes_execution_failure() {
        use std::sync::atomic::AtomicUsize;
        use std::sync::atomic::Ordering;

        let manager = OperationManager::new(ManagerOptions::default()).unwrap();
        let executor = LocalIoExecutor::new(
            manager.clone(),
            LocalExecutionConfig {
                worker_count: 1,
                queue_capacity: 2,
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

        let executions = std::sync::Arc::new(AtomicUsize::new(0));
        let executions_for_job = std::sync::Arc::clone(&executions);
        let queued = make_job(&executor, move |_| {
            executions_for_job.fetch_add(1, Ordering::SeqCst);
            OperationOutcome::Succeeded(())
        });
        let queued_for_control = std::sync::Arc::clone(&queued);
        let mut queued_handle = manager.try_submit_external(Default::default(), queued).unwrap();
        qubit_operation::OperationControl::request_stop(
            &*queued_for_control,
            qubit_operation::StopReason::UserCancelled,
        );
        release_tx.send(()).unwrap();

        assert!(matches!(
            first_handle.wait_outcome().unwrap(),
            OperationOutcome::Succeeded(())
        ));
        assert!(matches!(
            queued_handle.wait_outcome().unwrap(),
            OperationOutcome::Stopped(_)
        ));
        assert_eq!(executions.load(Ordering::SeqCst), 0);

        let panicking = make_job(&executor, |_| -> OperationOutcome<(), ()> {
            panic!("injected worker task panic")
        });
        let mut panic_handle = manager.try_submit_external(Default::default(), panicking).unwrap();
        assert!(matches!(
            panic_handle.wait_outcome().unwrap(),
            OperationOutcome::ExecutionFailed(_)
        ));
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
    /// Ensures a rejected fire-and-forget task does not retain its admission
    /// permit or execute after the worker queue has stopped.
    #[test]
    fn test_enqueue_task_rejection_returns_admission_capacity() {
        use std::sync::atomic::AtomicBool;
        use std::sync::atomic::Ordering;

        let manager = OperationManager::new(ManagerOptions::default()).unwrap();
        let executor = LocalIoExecutor::new(manager, LocalExecutionConfig::default()).unwrap();
        let executed = std::sync::Arc::new(AtomicBool::new(false));
        executor.inner.queue.stop();

        let executed_by_task = std::sync::Arc::clone(&executed);
        assert!(
            executor
                .enqueue_task(move || executed_by_task.store(true, Ordering::SeqCst))
                .is_err()
        );
        assert_eq!(executor.inner.queue_slots.used(), 0);
        assert!(!executed.load(Ordering::SeqCst));
        assert!(executor.reserve_slot().is_some());
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
    /// Runs the test last executor owner can be dropped by a worker without
    /// self-joining behavior while preserving the operation lifecycle
    /// invariants.
    #[test]
    fn test_last_owner_drop_on_worker_stops_and_drains_without_self_join() {
        const CHILD: &str = "QUBIT_LOCAL_FILES_LAST_OWNER_CHILD";
        if std::env::var_os(CHILD).is_some() {
            let manager = OperationManager::new(ManagerOptions::default()).unwrap();
            let executor = LocalIoExecutor::new(
                manager,
                LocalExecutionConfig {
                    worker_count: 2,
                    queue_capacity: 2,
                    ..Default::default()
                },
            )
            .unwrap();
            let queue = std::sync::Arc::clone(&executor.inner.queue);
            let (started_tx, started_rx) = mpsc::channel();
            let (release_tx, release_rx) = mpsc::channel();
            let (finished_tx, finished_rx) = mpsc::channel();
            let worker_owned_executor = executor.clone();
            executor
                .enqueue_task(move || {
                    started_tx.send(()).unwrap();
                    release_rx.recv().unwrap();
                    drop(worker_owned_executor);
                    finished_tx.send(()).unwrap();
                })
                .unwrap();
            started_rx.recv_timeout(Duration::from_secs(5)).unwrap();
            drop(executor);
            release_tx.send(()).unwrap();
            finished_rx.recv_timeout(Duration::from_secs(5)).unwrap();
            assert!(queue.push(Box::new(|| {}), false, 1).is_err());

            let manager = OperationManager::new(ManagerOptions::default()).unwrap();
            let executor = LocalIoExecutor::new(
                manager,
                LocalExecutionConfig {
                    worker_count: 2,
                    queue_capacity: 2,
                    ..Default::default()
                },
            )
            .unwrap();
            let cleanup = executor.cleanup_executor();
            let queue = std::sync::Arc::clone(&cleanup.queue);
            let permit = cleanup.reserve_cleanup().unwrap();
            let (started_tx, started_rx) = mpsc::channel();
            let (release_tx, release_rx) = mpsc::channel();
            let (finished_tx, finished_rx) = mpsc::channel();
            let worker_owned_cleanup = cleanup.clone();
            if cleanup
                .enqueue_cleanup(
                    Box::new(move || {
                        started_tx.send(()).unwrap();
                        release_rx.recv().unwrap();
                        drop(worker_owned_cleanup);
                        finished_tx.send(()).unwrap();
                    }),
                    permit,
                )
                .is_err()
            {
                panic!("cleanup job should be admitted");
            }
            started_rx.recv_timeout(Duration::from_secs(5)).unwrap();
            drop(cleanup);
            drop(executor);
            release_tx.send(()).unwrap();
            finished_rx.recv_timeout(Duration::from_secs(5)).unwrap();
            assert!(queue.push(Box::new(|| {}), false, 1).is_err());
            return;
        }

        let executable = std::env::current_exe().unwrap();
        let mut child = std::process::Command::new(executable)
            .arg("--exact")
            .arg("operation::executor::tests::test_last_owner_drop_on_worker_stops_and_drains_without_self_join")
            .arg("--nocapture")
            .env(CHILD, "1")
            .spawn()
            .unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        loop {
            match child.try_wait().unwrap() {
                Some(status) => {
                    assert!(status.success(), "last-owner child failed: {status}");
                    break;
                }
                None if std::time::Instant::now() < deadline => {
                    std::thread::yield_now();
                }
                None => {
                    child.kill().unwrap();
                    let status = child.wait().unwrap();
                    panic!("last-owner child timed out and was reaped: {status}");
                }
            }
        }
    }

    /// Runs the test executor clone retains workers after original drop
    /// behavior while preserving the operation lifecycle invariants.
    #[test]
    fn test_executor_clone_retains_workers_after_original_drop() {
        let manager = OperationManager::new(ManagerOptions::default()).unwrap();
        let executor = LocalIoExecutor::new(
            manager,
            LocalExecutionConfig {
                worker_count: 1,
                ..LocalExecutionConfig::default()
            },
        )
        .unwrap();
        let queue = std::sync::Arc::clone(&executor.inner.queue);
        let clone = executor.clone();
        drop(executor);
        let (done_tx, done_rx) = mpsc::channel();
        clone.enqueue_task(move || done_tx.send(()).unwrap()).unwrap();
        done_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("remaining executor clone must keep its worker alive");
        drop(clone);
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
