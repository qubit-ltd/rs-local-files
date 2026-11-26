// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

#![cfg(feature = "async")]

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Condvar;
use std::sync::Mutex;
use std::task::Context;
use std::task::Wake;
use std::task::Waker;

use qubit_local_files::LocalExecutionConfig;
use qubit_local_files::operation::AsyncLocalFileSystem;
use qubit_local_files::operation::LocalIoExecutor;
use qubit_local_files::operation::LocalOperationOptions;
use qubit_local_files::operation::publish::LocalWriterCommitRequest;
use qubit_local_files::options::LocalWriteMode;
use qubit_local_files::options::LocalWriteOptions;
use qubit_operation::ManagerOptions;
use qubit_operation::OperationManager;
use qubit_operation::OperationOutcome;

#[cfg(feature = "test-support")]
static PUBLISH_TEST_SUPPORT_LOCK: Mutex<()> = Mutex::new(());

#[cfg(feature = "test-support")]
static WRITER_PUBLISH_GATE_TEST_LOCK: Mutex<()> = Mutex::new(());

fn block_on<F: std::future::Future>(future: F) -> F::Output {
    struct Signal(Mutex<bool>, Condvar);
    impl Wake for Signal {
        fn wake(self: Arc<Self>) {
            *self.0.lock().unwrap() = true;
            self.1.notify_one();
        }
        fn wake_by_ref(self: &Arc<Self>) {
            *self.0.lock().unwrap() = true;
            self.1.notify_one();
        }
    }
    let signal = Arc::new(Signal(Mutex::new(false), Condvar::new()));
    let waker = Waker::from(Arc::clone(&signal));
    let mut context = Context::from_waker(&waker);
    let mut future = std::pin::pin!(future);
    loop {
        if let std::task::Poll::Ready(value) = future.as_mut().poll(&mut context) {
            return value;
        }
        let mut ready = signal.0.lock().unwrap();
        while !*ready {
            ready = signal.1.wait(ready).unwrap();
        }
        *ready = false;
    }
}

fn facade(manager: &OperationManager) -> AsyncLocalFileSystem {
    let executor = LocalIoExecutor::new(manager.clone(), LocalExecutionConfig::default()).unwrap();
    block_on(AsyncLocalFileSystem::host(executor)).unwrap()
}

#[test]
fn test_successful_commit_publishes_and_records_effect_independently_of_outcome() {
    let directory = tempfile::tempdir().unwrap();
    let destination = directory.path().join("published");
    let manager = OperationManager::new(ManagerOptions::default()).unwrap();
    let filesystem = facade(&manager);
    let (mut operation, observer) = filesystem
        .submit_writer_commit(
            &manager,
            Default::default(),
            LocalWriterCommitRequest {
                path: PathBuf::from(&destination),
                options: LocalWriteOptions::new(LocalWriteMode::CreateOrReplace),
                bytes: b"payload".to_vec(),
            },
            LocalOperationOptions::default(),
        )
        .unwrap();
    assert!(matches!(
        operation.wait_outcome().unwrap(),
        OperationOutcome::Succeeded(_)
    ));
    assert_eq!(std::fs::read(destination).unwrap(), b"payload");
    let facts = observer.snapshot();
    assert_eq!(
        facts.publication,
        qubit_local_files::operation::LocalPublicationState::Published
    );
    assert_eq!(facts.modified_bytes, 7);
    assert_eq!(facts.modified_count, 1);
}

#[test]
fn test_create_new_conflict_preserves_the_existing_destination_and_not_published_fact() {
    let directory = tempfile::tempdir().unwrap();
    let destination = directory.path().join("occupied");
    std::fs::write(&destination, b"original").unwrap();
    let manager = OperationManager::new(ManagerOptions::default()).unwrap();
    let filesystem = facade(&manager);
    let (mut operation, observer) = filesystem
        .submit_writer_commit(
            &manager,
            Default::default(),
            LocalWriterCommitRequest {
                path: PathBuf::from(&destination),
                options: LocalWriteOptions::new(LocalWriteMode::CreateNew),
                bytes: b"replacement".to_vec(),
            },
            LocalOperationOptions::default(),
        )
        .unwrap();
    assert!(matches!(operation.wait_outcome().unwrap(), OperationOutcome::Failed(_)));
    assert_eq!(std::fs::read(destination).unwrap(), b"original");
    assert_eq!(
        observer.snapshot().publication,
        qubit_local_files::operation::LocalPublicationState::NotPublished
    );
}

#[test]
fn test_writer_rejects_owned_allocation_over_budget_before_opening_destination() {
    let directory = tempfile::tempdir().unwrap();
    let destination = directory.path().join("must-not-exist");
    let manager = OperationManager::new(ManagerOptions::default()).unwrap();
    let filesystem = facade(&manager)
        .with_symlink_policy(qubit_local_files::policy::LocalSymlinkPolicy::Reject)
        .unwrap();
    let request = LocalWriterCommitRequest {
        path: PathBuf::from(&destination),
        options: LocalWriteOptions::new(LocalWriteMode::CreateOrReplace),
        bytes: Vec::with_capacity(32),
    };
    let rejected = match filesystem.submit_writer_commit(
        &manager,
        Default::default(),
        request,
        LocalOperationOptions {
            max_owned_bytes: Some(8),
            ..LocalOperationOptions::default()
        },
    ) {
        Err(error) => error,
        Ok(_) => panic!("allocation should be rejected before admission"),
    };
    assert_eq!(rejected.into_request().bytes.capacity(), 32);
    assert!(!destination.exists());
}

#[test]
fn test_staged_tree_disk_quota_rejection_returns_the_original_request() {
    use qubit_local_files::operation::LocalSubmitRejection;
    use qubit_local_files::operation::publish::LocalStagedTreeFile;
    use qubit_local_files::operation::publish::LocalStagedTreePublishRequest;
    use qubit_local_files::options::LocalTempDirectoryOptions;
    use qubit_local_files::options::LocalTreePublishOptions;

    let directory = tempfile::tempdir().unwrap();
    let manager = OperationManager::new(ManagerOptions::default()).unwrap();
    let executor = LocalIoExecutor::new(
        manager.clone(),
        LocalExecutionConfig {
            max_disk_bytes: 4,
            ..LocalExecutionConfig::default()
        },
    )
    .unwrap();
    let filesystem = block_on(AsyncLocalFileSystem::host(executor)).unwrap();
    let request = LocalStagedTreePublishRequest {
        parent: directory.path().into(),
        name: "over-quota".into(),
        stage_options: LocalTempDirectoryOptions::new(),
        files: vec![LocalStagedTreeFile {
            path: PathBuf::from("payload"),
            bytes: b"12345".to_vec(),
        }],
        publish_options: LocalTreePublishOptions::new(),
    };
    let error = match filesystem.submit_staged_tree_publish(
        &manager,
        Default::default(),
        request,
        LocalOperationOptions::default(),
    ) {
        Err(error) => error,
        Ok(_) => panic!("disk quota should reject before manager admission"),
    };
    assert_eq!(error.rejection(), LocalSubmitRejection::CapacityExceeded);
    let returned = error.into_request();
    assert_eq!(returned.files[0].bytes, b"12345");
    assert!(!directory.path().join("over-quota").exists());
}

#[test]
fn test_staged_tree_uses_one_parent_authority_and_publishes_only_after_exact_validation() {
    use qubit_local_files::operation::publish::LocalStagedTreeFile;
    use qubit_local_files::operation::publish::LocalStagedTreePublishRequest;
    use qubit_local_files::options::LocalTempDirectoryOptions;
    use qubit_local_files::options::LocalTreePublishOptions;

    let directory = tempfile::tempdir().unwrap();
    let published = directory.path().join("tree");
    let manager = OperationManager::new(ManagerOptions::default()).unwrap();
    let filesystem = facade(&manager);
    let (mut operation, observer) = filesystem
        .submit_staged_tree_publish(
            &manager,
            Default::default(),
            LocalStagedTreePublishRequest {
                parent: PathBuf::from(directory.path()),
                name: "tree".into(),
                stage_options: LocalTempDirectoryOptions::new(),
                files: vec![LocalStagedTreeFile {
                    path: PathBuf::from("item"),
                    bytes: b"staged".to_vec(),
                }],
                publish_options: LocalTreePublishOptions::new(),
            },
            LocalOperationOptions::default(),
        )
        .unwrap();
    assert!(matches!(
        operation.wait_outcome().unwrap(),
        OperationOutcome::Succeeded(_)
    ));
    assert_eq!(std::fs::read(published.join("item")).unwrap(), b"staged");
    assert_eq!(
        observer.snapshot().publication,
        qubit_local_files::operation::LocalPublicationState::Published
    );
    assert_eq!(observer.snapshot().modified_count, 2);
}

#[cfg(feature = "digest")]
#[test]
fn test_digest_conditional_conflict_keeps_existing_data_and_published_state_correct() {
    use qubit_local_files::operation::publish::LocalConditionalWriterCommitRequest;
    use qubit_local_files::options::LocalWriteMode;
    use qubit_local_files::options::LocalWritePrecondition;
    use sha2::Sha256;

    let directory = tempfile::tempdir().unwrap();
    // macOS exposes the temporary directory through /var, which is a symlink
    // to /private/var. Resolve it before using the Reject symlink policy.
    let root = std::fs::canonicalize(directory.path()).unwrap();
    let destination = root.join("occupied");
    std::fs::write(&destination, b"original").unwrap();
    let manager = OperationManager::new(ManagerOptions::default()).unwrap();
    let filesystem = facade(&manager)
        .with_symlink_policy(qubit_local_files::policy::LocalSymlinkPolicy::Reject)
        .unwrap();
    let (mut operation, observer) = filesystem
        .submit_conditional_writer_commit::<Sha256>(
            &manager,
            Default::default(),
            LocalConditionalWriterCommitRequest {
                path: PathBuf::from(&destination),
                precondition: LocalWritePrecondition::Absent,
                options: LocalWriteOptions::new(LocalWriteMode::CreateOrReplace),
                bytes: b"replacement".to_vec(),
            },
            LocalOperationOptions::default(),
        )
        .unwrap();
    assert!(matches!(
        operation.wait_outcome().unwrap(),
        OperationOutcome::Failed(qubit_local_files::operation::publish::LocalConditionalWriterCommandError::Commit(_))
    ));
    assert_eq!(std::fs::read(destination).unwrap(), b"original");
    assert_eq!(
        observer.snapshot().publication,
        qubit_local_files::operation::LocalPublicationState::NotPublished
    );
}

#[cfg(feature = "digest")]
#[test]
fn test_digest_conditional_content_mismatch_preserves_existing_data_and_effect_facts() {
    use qubit_local_files::operation::publish::LocalConditionalWriterCommitRequest;
    use qubit_local_files::options::LocalWritePrecondition;
    use sha2::Digest;
    use sha2::Sha256;

    let directory = tempfile::tempdir().unwrap();
    let root = std::fs::canonicalize(directory.path()).unwrap();
    let destination = root.join("content-mismatch");
    std::fs::write(&destination, b"current content").unwrap();
    let manager = OperationManager::new(ManagerOptions::default()).unwrap();
    let filesystem = facade(&manager)
        .with_symlink_policy(qubit_local_files::policy::LocalSymlinkPolicy::Reject)
        .unwrap();
    let (mut operation, observer) = filesystem
        .submit_conditional_writer_commit::<Sha256>(
            &manager,
            Default::default(),
            LocalConditionalWriterCommitRequest {
                path: destination.clone(),
                precondition: LocalWritePrecondition::Content(Sha256::digest(b"stale content")),
                options: LocalWriteOptions::new(LocalWriteMode::CreateOrReplace),
                bytes: b"candidate content".to_vec(),
            },
            LocalOperationOptions::default(),
        )
        .unwrap();

    assert!(matches!(
        operation.wait_outcome().unwrap(),
        OperationOutcome::Failed(qubit_local_files::operation::publish::LocalConditionalWriterCommandError::Commit(_))
    ));
    assert_eq!(std::fs::read(destination).unwrap(), b"current content");
    let facts = observer.snapshot();
    assert_eq!(
        facts.publication,
        qubit_local_files::operation::LocalPublicationState::NotPublished
    );
    assert_eq!(facts.modified_count, 0);
    assert_eq!(facts.modified_bytes, 0);
}

#[cfg(feature = "digest")]
#[test]
fn test_digest_conditional_absent_commit_publishes_and_records_effect() {
    use qubit_local_files::operation::publish::LocalConditionalWriterCommitRequest;
    use qubit_local_files::options::LocalWritePrecondition;
    use sha2::Sha256;

    let directory = tempfile::tempdir().unwrap();
    let destination = std::fs::canonicalize(directory.path())
        .unwrap()
        .join("created-conditionally");
    let manager = OperationManager::new(ManagerOptions::default()).unwrap();
    let filesystem = facade(&manager)
        .with_symlink_policy(qubit_local_files::policy::LocalSymlinkPolicy::Reject)
        .unwrap();
    let (mut operation, observer) = filesystem
        .submit_conditional_writer_commit::<Sha256>(
            &manager,
            Default::default(),
            LocalConditionalWriterCommitRequest {
                path: destination.clone(),
                precondition: LocalWritePrecondition::Absent,
                options: LocalWriteOptions::new(LocalWriteMode::CreateOrReplace),
                bytes: b"conditional payload".to_vec(),
            },
            LocalOperationOptions::default(),
        )
        .unwrap();

    let outcome = operation.wait_outcome().unwrap();
    assert!(matches!(outcome, OperationOutcome::Succeeded(_)));
    assert_eq!(std::fs::read(destination).unwrap(), b"conditional payload");
    let facts = observer.snapshot();
    assert_eq!(
        facts.publication,
        qubit_local_files::operation::LocalPublicationState::Published
    );
    assert_eq!(facts.modified_bytes, 19);
    assert_eq!(facts.modified_count, 1);
}

#[cfg(all(feature = "digest", feature = "test-support"))]
#[test]
fn test_conditional_writer_cancellation_at_precommit_gate_aborts_without_publishing() {
    use qubit_local_files::operation::publish::LocalConditionalWriterCommitRequest;
    use qubit_local_files::options::LocalWritePrecondition;
    use qubit_local_files::test_support::install_test_gate;
    use qubit_operation::StopReason;
    use sha2::Sha256;

    let _support_lock = PUBLISH_TEST_SUPPORT_LOCK
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let directory = tempfile::tempdir().unwrap();
    let destination = std::fs::canonicalize(directory.path())
        .unwrap()
        .join("cancelled-conditional");
    let manager = OperationManager::new(ManagerOptions::default()).unwrap();
    let filesystem = facade(&manager)
        .with_symlink_policy(qubit_local_files::policy::LocalSymlinkPolicy::Reject)
        .unwrap();
    let gate = install_test_gate("lf7-conditional-before-publish").unwrap();
    let (mut operation, observer) = filesystem
        .submit_conditional_writer_commit::<Sha256>(
            &manager,
            Default::default(),
            LocalConditionalWriterCommitRequest {
                path: destination.clone(),
                precondition: LocalWritePrecondition::Absent,
                options: LocalWriteOptions::new(LocalWriteMode::CreateOrReplace),
                bytes: b"never published".to_vec(),
            },
            LocalOperationOptions::default(),
        )
        .unwrap();
    gate.wait_until_entered();
    operation.request_stop(StopReason::UserCancelled);
    gate.release();

    assert!(matches!(
        operation.wait_outcome().unwrap(),
        OperationOutcome::Stopped(report) if report.reason == StopReason::UserCancelled
    ));
    operation
        .wait_settlement()
        .expect("cancelled writer cleanup should settle before the test exits");
    assert!(!destination.exists());
    assert_eq!(
        observer.snapshot().publication,
        qubit_local_files::operation::LocalPublicationState::NotPublished
    );
}

#[cfg(all(feature = "digest", feature = "test-support"))]
#[test]
fn test_conditional_abort_failure_after_stop_retains_recovery_owner() {
    use qubit_local_files::operation::publish::LocalConditionalWriterCommitRequest;
    use qubit_local_files::options::LocalWriteMode;
    use qubit_local_files::options::LocalWritePrecondition;
    use qubit_local_files::test_support::install_test_fault_shared;
    use qubit_local_files::test_support::install_test_gate;
    use qubit_operation::StopReason;
    use sha2::Sha256;

    let _support_lock = PUBLISH_TEST_SUPPORT_LOCK
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let directory = tempfile::tempdir().unwrap();
    // macOS exposes the temporary directory through /var, which is a symlink
    // to /private/var. Resolve it before using the Reject symlink policy.
    let root = std::fs::canonicalize(directory.path()).unwrap();
    let destination = root.join("conditional-abort-recovery");
    let manager = OperationManager::new(ManagerOptions::default()).unwrap();
    let executor = LocalIoExecutor::new(manager.clone(), LocalExecutionConfig::default()).unwrap();
    let filesystem = block_on(AsyncLocalFileSystem::host(executor.clone()))
        .unwrap()
        .with_symlink_policy(qubit_local_files::policy::LocalSymlinkPolicy::Reject)
        .unwrap();
    let gate = Arc::new(install_test_gate("lf7-conditional-before-publish").unwrap());
    let (mut operation, observer) = filesystem
        .submit_conditional_writer_commit::<Sha256>(
            &manager,
            Default::default(),
            LocalConditionalWriterCommitRequest {
                path: destination.clone(),
                precondition: LocalWritePrecondition::Absent,
                options: LocalWriteOptions::new(LocalWriteMode::CreateOrReplace),
                bytes: b"conditional staging".to_vec(),
            },
            LocalOperationOptions::default(),
        )
        .unwrap();
    let gate_waiter = Arc::clone(&gate);
    let (entered_sender, entered_receiver) = std::sync::mpsc::channel();
    let gate_waiter = std::thread::spawn(move || {
        gate_waiter.wait_until_entered();
        let _ = entered_sender.send(());
    });
    if entered_receiver
        .recv_timeout(std::time::Duration::from_secs(5))
        .is_err()
    {
        gate.release();
        let _ = gate_waiter.join();
        let _ = operation.request_stop(StopReason::UserCancelled);
        let outcome = operation.wait_outcome();
        panic!("conditional operation did not reach publish gate; outcome: {outcome:?}");
    }
    gate_waiter.join().expect("gate waiter should exit after entry");
    let fault = install_test_fault_shared("lf7-conditional-abort").unwrap();
    let _ = operation.request_stop(StopReason::UserCancelled);
    gate.release();
    assert!(matches!(
        operation.wait_outcome().unwrap(),
        OperationOutcome::Stopped(_)
    ));
    assert_eq!(
        observer.snapshot().publication,
        qubit_local_files::operation::LocalPublicationState::NotPublished
    );
    let settlement = operation.wait_settlement().unwrap();
    assert!(settlement.recovery_count > 0);
    let recovery = settlement
        .recoveries
        .first()
        .expect("conditional writer cleanup failure must retain its owner")
        .clone();
    drop(settlement);
    drop(fault);
    let report = recovery.try_retry().unwrap().wait().unwrap();
    assert_eq!(report.recovery.phase, qubit_operation::RecoveryPhase::Resolved);
    assert!(!destination.exists());
    drop(report);
    drop(observer);
    let shutdown_report = executor
        .request_shutdown(qubit_operation::ShutdownMode::Drain)
        .wait()
        .expect("executor group shutdown should complete after recovery");
    assert_eq!(shutdown_report.snapshot.active_operations, 0);
    drop(recovery);
    drop(operation);
    drop(filesystem);
    drop(executor);
    let shutdown_report = manager
        .request_shutdown(qubit_operation::ShutdownMode::Drain)
        .wait()
        .expect("manager shutdown should release recovery tracking");
    assert_eq!(shutdown_report.snapshot.active_operations, 0);
    assert_eq!(shutdown_report.snapshot.recovery_groups, 0);
}

#[test]
fn test_expired_deadline_rejects_owned_request_before_native_work() {
    use std::task::Waker;

    use qubit_operation::ClockDomainId;
    use qubit_operation::ClockSource;
    use qubit_operation::ContextOptions;
    use qubit_operation::Deadline;
    use qubit_operation::MonotonicTime;
    use qubit_operation::TimerRegistration;

    struct FakeClock {
        domain: ClockDomainId,
        ticks: u64,
    }
    struct NoTimer;
    impl TimerRegistration for NoTimer {
        fn cancel(self: Box<Self>) {}
    }
    impl ClockSource for FakeClock {
        fn now(&self) -> MonotonicTime {
            MonotonicTime::new(self.domain, self.ticks)
        }
        fn register_deadline(&self, _at: MonotonicTime, _wake: Waker) -> Box<dyn TimerRegistration> {
            Box::new(NoTimer)
        }
    }

    let directory = tempfile::tempdir().unwrap();
    let destination = directory.path().join("cancelled");
    let manager = OperationManager::new(ManagerOptions::default()).unwrap();
    let filesystem = facade(&manager);
    let domain = ClockDomainId::new_unique();
    let options = qubit_operation::OperationOptions {
        context: ContextOptions {
            clock: Some(Arc::new(FakeClock { domain, ticks: 10 })),
            deadline: Some(Deadline::at(MonotonicTime::new(domain, 1))),
            budget: None,
        },
        ..Default::default()
    };
    let request = LocalWriterCommitRequest {
        path: PathBuf::from(&destination),
        options: LocalWriteOptions::new(LocalWriteMode::CreateOrReplace),
        bytes: b"never written".to_vec(),
    };
    let rejected = match filesystem.submit_writer_commit(&manager, options, request, LocalOperationOptions::default()) {
        Err(error) => error,
        Ok(_) => panic!("expired deadline should reject admission"),
    };
    assert_eq!(rejected.into_request().bytes, b"never written");
    assert!(!destination.exists());
}

#[test]
fn test_partial_stage_write_failure_keeps_the_unpublished_stage_for_cleanup() {
    use qubit_local_files::operation::publish::LocalStagedTreeFile;
    use qubit_local_files::operation::publish::LocalStagedTreePublishError;
    use qubit_local_files::operation::publish::LocalStagedTreePublishRequest;
    use qubit_local_files::options::LocalTempCleanupLimits;
    use qubit_local_files::options::LocalTempDirectoryOptions;
    use qubit_local_files::options::LocalTreePublishOptions;

    let directory = tempfile::tempdir().unwrap();
    let published = directory.path().join("tree");
    let manager = OperationManager::new(ManagerOptions::default()).unwrap();
    let filesystem = facade(&manager);
    let (mut operation, observer) = filesystem
        .submit_staged_tree_publish(
            &manager,
            Default::default(),
            LocalStagedTreePublishRequest {
                parent: PathBuf::from(directory.path()),
                name: "tree".into(),
                stage_options: LocalTempDirectoryOptions::new(),
                files: vec![
                    LocalStagedTreeFile {
                        path: PathBuf::from("same"),
                        bytes: b"first".to_vec(),
                    },
                    LocalStagedTreeFile {
                        path: PathBuf::from("same"),
                        bytes: b"second".to_vec(),
                    },
                ],
                publish_options: LocalTreePublishOptions::new(),
            },
            LocalOperationOptions::default(),
        )
        .unwrap();
    match operation.wait_outcome().unwrap() {
        OperationOutcome::Failed(LocalStagedTreePublishError::Write { mut stage, .. }) => {
            let stage_name = stage.relative_name().to_os_string();
            assert!(!stage_name.is_empty());
            assert!(stage.observation().is_ok());
            assert!(format!("{stage:?}").contains("LocalStagedTreeRecovery"));

            // Exercise mutable dereferencing as a caller configuring the
            // native retained stage through the recovery owner.
            std::ops::DerefMut::deref_mut(&mut stage).set_cleanup_limits(LocalTempCleanupLimits::new());
            stage.set_cleanup_limits(LocalTempCleanupLimits::new());

            stage.cleanup().unwrap();
        }
        outcome => panic!("expected retained stage after duplicate write, got {outcome:?}"),
    }
    assert!(!published.exists());
    assert_eq!(
        observer.snapshot().publication,
        qubit_local_files::operation::LocalPublicationState::NotPublished
    );
    assert_eq!(observer.snapshot().modified_count, 2);
    assert!(observer.snapshot().range_unknown);
}

#[test]
fn test_staged_recovery_cleanup_failure_keeps_disk_quota_and_retry_owner() {
    use qubit_local_files::directory::LocalTreeRetainedResource;
    use qubit_local_files::operation::publish::LocalStagedTreeFile;
    use qubit_local_files::operation::publish::LocalStagedTreePublishError;
    use qubit_local_files::operation::publish::LocalStagedTreePublishRequest;
    use qubit_local_files::options::LocalTempCleanupLimits;
    use qubit_local_files::options::LocalTempDirectoryOptions;
    use qubit_local_files::options::LocalTreePublishOptions;

    let directory = tempfile::tempdir().unwrap();
    let manager = OperationManager::new(ManagerOptions::default()).unwrap();
    let filesystem = facade(&manager);
    let (mut operation, _) = filesystem
        .submit_staged_tree_publish(
            &manager,
            Default::default(),
            LocalStagedTreePublishRequest {
                parent: directory.path().into(),
                name: "recovery-cleanup-failure".into(),
                stage_options: LocalTempDirectoryOptions::new(),
                files: vec![
                    LocalStagedTreeFile {
                        path: PathBuf::from("same"),
                        bytes: b"first".to_vec(),
                    },
                    LocalStagedTreeFile {
                        path: PathBuf::from("same"),
                        bytes: b"second".to_vec(),
                    },
                ],
                publish_options: LocalTreePublishOptions::new(),
            },
            LocalOperationOptions::default(),
        )
        .unwrap();

    let mut recovery = match operation.wait_outcome().unwrap() {
        OperationOutcome::Failed(LocalStagedTreePublishError::Write { stage, .. }) => stage,
        outcome => panic!("expected retained stage after duplicate write, got {outcome:?}"),
    };
    recovery.set_cleanup_limits(LocalTempCleanupLimits::new().with_max_entries(0));

    let cleanup = recovery
        .cleanup()
        .expect_err("the zero-entry cleanup limit should retain the recovery owner");
    let (native_error, reservation) = cleanup.into_parts();
    assert!(reservation.bytes() >= 1);
    assert_eq!(native_error.retained_resources().len(), 1);

    let mut retained: Vec<LocalTreeRetainedResource> = native_error.into_retained_resources();
    let mut retry = retained.pop().expect("failed cleanup retains its stage");
    retry.set_cleanup_limits(LocalTempCleanupLimits::new());
    retry.cleanup().unwrap();
    drop(reservation);
}

#[cfg(unix)]
#[test]
fn test_staged_tree_forces_private_file_creation_even_when_defaults_were_requested() {
    use std::os::unix::fs::PermissionsExt;

    use qubit_local_files::operation::publish::LocalStagedTreeFile;
    use qubit_local_files::operation::publish::LocalStagedTreePublishError;
    use qubit_local_files::operation::publish::LocalStagedTreePublishRequest;
    use qubit_local_files::options::LocalTempDirectoryOptions;
    use qubit_local_files::options::LocalTreePublishOptions;
    use qubit_local_files::policy::LocalPrivacyRequirement;

    let directory = tempfile::tempdir().unwrap();
    let manager = OperationManager::new(ManagerOptions::default()).unwrap();
    let filesystem = facade(&manager);
    let (mut operation, _) = filesystem
        .submit_staged_tree_publish(
            &manager,
            Default::default(),
            LocalStagedTreePublishRequest {
                parent: directory.path().into(),
                name: "private-stage".into(),
                stage_options: LocalTempDirectoryOptions::new().with_privacy(LocalPrivacyRequirement::PlatformDefault),
                files: vec![
                    LocalStagedTreeFile {
                        path: PathBuf::from("partial"),
                        bytes: b"private".to_vec(),
                    },
                    LocalStagedTreeFile {
                        path: PathBuf::from("partial"),
                        bytes: b"duplicate".to_vec(),
                    },
                ],
                publish_options: LocalTreePublishOptions::new(),
            },
            LocalOperationOptions::default(),
        )
        .unwrap();
    let stage = match operation.wait_outcome().unwrap() {
        OperationOutcome::Failed(LocalStagedTreePublishError::Write { stage, .. }) => stage,
        _ => panic!("expected partial stage write failure"),
    };
    let staged_file = directory.path().join(stage.relative_name()).join("partial");
    assert_eq!(
        std::fs::metadata(staged_file).unwrap().permissions().mode() & 0o777,
        0o600
    );
    stage.cleanup().unwrap();
}

#[test]
fn test_exhausted_fact_record_slots_reject_new_mutation_before_native_work() {
    let directory = tempfile::tempdir().unwrap();
    let first_path = directory.path().join("first");
    let second_path = directory.path().join("second");
    let manager = OperationManager::new(ManagerOptions::default()).unwrap();
    let executor = LocalIoExecutor::new(
        manager.clone(),
        LocalExecutionConfig {
            max_fact_records: 1,
            ..LocalExecutionConfig::default()
        },
    )
    .unwrap();
    let filesystem = block_on(AsyncLocalFileSystem::host(executor)).unwrap();
    let (mut first, _first_observer) = filesystem
        .submit_writer_commit(
            &manager,
            Default::default(),
            LocalWriterCommitRequest {
                path: first_path,
                options: LocalWriteOptions::new(LocalWriteMode::CreateOrReplace),
                bytes: b"first".to_vec(),
            },
            LocalOperationOptions::default(),
        )
        .unwrap();
    let first_outcome = first.wait_outcome().unwrap();
    assert!(
        matches!(first_outcome, OperationOutcome::Succeeded(_)),
        "first mutation should complete before the record limit is exhausted: {first_outcome:?}"
    );
    let request = LocalWriterCommitRequest {
        path: second_path.clone(),
        options: LocalWriteOptions::new(LocalWriteMode::CreateOrReplace),
        bytes: b"second".to_vec(),
    };
    let rejected = match filesystem.submit_writer_commit(
        &manager,
        Default::default(),
        request,
        LocalOperationOptions::default(),
    ) {
        Err(error) => error,
        Ok(_) => panic!("fact record exhaustion should reject admission"),
    };
    assert_eq!(
        rejected.rejection(),
        qubit_local_files::operation::LocalSubmitRejection::CapacityExceeded
    );
    assert_eq!(rejected.into_request().bytes, b"second");
    assert!(!second_path.exists());
}

#[test]
fn test_failed_publish_effect_registration_keeps_observer_facts() {
    use qubit_local_files::operation::publish::LocalStagedTreeFile;
    use qubit_local_files::operation::publish::LocalStagedTreePublishRequest;
    use qubit_local_files::options::LocalTempDirectoryOptions;
    use qubit_local_files::options::LocalTreePublishOptions;

    let directory = tempfile::tempdir().unwrap();
    let manager = OperationManager::new(ManagerOptions::default()).unwrap();
    let filesystem = facade(&manager);
    let mut options = qubit_operation::OperationOptions::default();
    options.limits.effect_records = 1;
    let (mut operation, observer) = filesystem
        .submit_staged_tree_publish(
            &manager,
            options,
            LocalStagedTreePublishRequest {
                parent: PathBuf::from(directory.path()),
                name: "tree".into(),
                stage_options: LocalTempDirectoryOptions::new(),
                files: vec![LocalStagedTreeFile {
                    path: PathBuf::from("one"),
                    bytes: b"data".to_vec(),
                }],
                publish_options: LocalTreePublishOptions::new(),
            },
            LocalOperationOptions::default(),
        )
        .unwrap();
    assert!(matches!(
        operation.wait_outcome().unwrap(),
        OperationOutcome::Succeeded(_)
    ));
    let facts = observer.snapshot();
    assert_eq!(
        facts.publication,
        qubit_local_files::operation::LocalPublicationState::Published
    );
    assert_eq!(facts.modified_count, 2);
    assert_eq!(facts.modified_bytes, 4);
    assert_eq!(
        facts.effect_record_error,
        Some(qubit_operation::EffectRecordError::CapacityExceeded)
    );
}

#[test]
fn test_temp_persist_conflict_retains_same_resource_for_a_later_keep() {
    use qubit_local_files::LocalFileSystem;
    use qubit_local_files::operation::publish::LocalTempLifecycleCommand;
    use qubit_local_files::operation::publish::LocalTempLifecycleError;
    use qubit_local_files::operation::publish::LocalTempLifecycleRequest;
    use qubit_local_files::operation::publish::LocalTempLifecycleResult;
    use qubit_local_files::operation::submit_temp_file;
    use qubit_local_files::options::LocalPersistOptions;
    use qubit_local_files::options::LocalTempFileOptions;

    let directory = tempfile::tempdir().unwrap();
    let target = directory.path().join("occupied");
    std::fs::write(&target, b"preserve").unwrap();
    let manager = OperationManager::new(ManagerOptions::default()).unwrap();
    let executor = LocalIoExecutor::new(manager.clone(), LocalExecutionConfig::default()).unwrap();
    let filesystem = LocalFileSystem::host().unwrap();
    let async_filesystem = block_on(AsyncLocalFileSystem::host(executor.clone())).unwrap();
    let mut creation = submit_temp_file(
        filesystem,
        executor,
        &manager.as_async(),
        Default::default(),
        LocalTempFileOptions::new().with_parent(directory.path()),
    )
    .unwrap();
    let access = match block_on(creation.wait_outcome()).unwrap() {
        OperationOutcome::Succeeded(access) => access,
        _ => panic!("temp creation failed"),
    };
    let original_id = access.resource_id();
    let (mut failed, same_access, failed_facts) = async_filesystem
        .submit_temp_lifecycle(
            &manager,
            Default::default(),
            LocalTempLifecycleRequest {
                access,
                command: LocalTempLifecycleCommand::Persist {
                    target: target.clone(),
                    options: LocalPersistOptions::new(),
                },
            },
        )
        .unwrap();
    assert_eq!(same_access.resource_id(), original_id);
    assert!(matches!(
        block_on(failed.wait_outcome()).unwrap(),
        OperationOutcome::Failed(LocalTempLifecycleError::Persist(_))
    ));
    assert_eq!(std::fs::read(&target).unwrap(), b"preserve");
    assert_eq!(
        failed_facts.snapshot().publication,
        qubit_local_files::operation::LocalPublicationState::NotPublished
    );

    let (mut kept, same_access, facts) = async_filesystem
        .submit_temp_lifecycle(
            &manager,
            Default::default(),
            LocalTempLifecycleRequest {
                access: same_access,
                command: LocalTempLifecycleCommand::Keep,
            },
        )
        .unwrap();
    assert_eq!(same_access.resource_id(), original_id);
    let persisted = match block_on(kept.wait_outcome()).unwrap() {
        OperationOutcome::Succeeded(LocalTempLifecycleResult::Persisted(outcome)) => outcome,
        _ => panic!("temp keep failed"),
    };
    assert!(persisted.path().exists());
    assert_eq!(
        facts.snapshot().publication,
        qubit_local_files::operation::LocalPublicationState::Published
    );
    std::fs::remove_file(persisted.path()).unwrap();
}

#[test]
fn test_temp_seal_returns_a_read_lease_for_the_same_registered_resource() {
    use qubit_local_files::LocalFileSystem;
    use qubit_local_files::operation::publish::LocalTempLifecycleCommand;
    use qubit_local_files::operation::publish::LocalTempLifecycleRequest;
    use qubit_local_files::operation::publish::LocalTempLifecycleResult;
    use qubit_local_files::operation::submit_temp_file;
    use qubit_local_files::options::LocalTempFileOptions;

    let directory = tempfile::tempdir().unwrap();
    let manager = OperationManager::new(ManagerOptions::default()).unwrap();
    let executor = LocalIoExecutor::new(manager.clone(), LocalExecutionConfig::default()).unwrap();
    let filesystem = LocalFileSystem::host().unwrap();
    let async_filesystem = block_on(AsyncLocalFileSystem::host(executor.clone())).unwrap();
    let mut creation = submit_temp_file(
        filesystem,
        executor.clone(),
        &manager.as_async(),
        Default::default(),
        LocalTempFileOptions::new().with_parent(directory.path()),
    )
    .unwrap();
    let access = match block_on(creation.wait_outcome()).unwrap() {
        OperationOutcome::Succeeded(access) => access,
        _ => panic!("temp creation failed"),
    };
    let resource_id = access.resource_id();
    let (mut operation, same_access, _) = async_filesystem
        .submit_temp_lifecycle(
            &manager,
            Default::default(),
            LocalTempLifecycleRequest {
                access,
                command: LocalTempLifecycleCommand::Seal {
                    options: qubit_local_files::LocalSealOptions::default(),
                },
            },
        )
        .unwrap();
    assert_eq!(same_access.resource_id(), resource_id);
    match block_on(operation.wait_outcome()).unwrap() {
        OperationOutcome::Succeeded(LocalTempLifecycleResult::Sealed(lease)) => {
            assert_eq!(lease.resource_id(), resource_id);
        }
        _ => panic!("temp seal failed"),
    }
    drop(block_on(operation.wait_settlement()));
    drop(operation);
    drop(same_access);
    drop(creation);
    drop(async_filesystem);
    let cleanup_deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while std::fs::read_dir(directory.path()).unwrap().next().is_some() && std::time::Instant::now() < cleanup_deadline
    {
        std::thread::yield_now();
    }
    assert_eq!(
        0,
        std::fs::read_dir(directory.path()).unwrap().count(),
        "dropping the last sealed-resource lease should finish native cleanup",
    );
    let executor_shutdown = executor
        .request_shutdown(qubit_operation::ShutdownMode::Drain)
        .wait()
        .expect("executor group shutdown should complete after sealing");
    assert_eq!(executor_shutdown.snapshot.active_operations, 0);
    drop(executor);
    let manager_shutdown = manager
        .request_shutdown(qubit_operation::ShutdownMode::Drain)
        .wait()
        .expect("manager shutdown should release temporary-resource tracking");
    assert_eq!(manager_shutdown.snapshot.active_operations, 0);
    assert_eq!(manager_shutdown.snapshot.recovery_groups, 0);
}

#[test]
fn test_sealed_temp_rejects_repeat_lifecycle_commands_without_consuming_the_owner() {
    use qubit_local_files::LocalFileSystem;
    use qubit_local_files::operation::publish::LocalTempLifecycleCommand;
    use qubit_local_files::operation::publish::LocalTempLifecycleError;
    use qubit_local_files::operation::publish::LocalTempLifecycleRequest;
    use qubit_local_files::operation::publish::LocalTempLifecycleResult;
    use qubit_local_files::operation::submit_temp_file;
    use qubit_local_files::options::LocalPersistOptions;
    use qubit_local_files::options::LocalTempFileOptions;

    let directory = tempfile::tempdir().unwrap();
    let manager = OperationManager::new(ManagerOptions::default()).unwrap();
    let executor = LocalIoExecutor::new(manager.clone(), LocalExecutionConfig::default()).unwrap();
    let filesystem = LocalFileSystem::host().unwrap();
    let async_filesystem = block_on(AsyncLocalFileSystem::host(executor.clone())).unwrap();
    let mut creation = submit_temp_file(
        filesystem,
        executor.clone(),
        &manager.as_async(),
        Default::default(),
        LocalTempFileOptions::new().with_parent(directory.path()),
    )
    .unwrap();
    let access = match block_on(creation.wait_outcome()).unwrap() {
        OperationOutcome::Succeeded(access) => access,
        _ => panic!("temp creation failed"),
    };
    let resource_id = access.resource_id();
    let (mut seal, access, _) = async_filesystem
        .submit_temp_lifecycle(
            &manager,
            Default::default(),
            LocalTempLifecycleRequest {
                access,
                command: LocalTempLifecycleCommand::Seal {
                    options: qubit_local_files::LocalSealOptions::default(),
                },
            },
        )
        .unwrap();
    let lease = match block_on(seal.wait_outcome()).unwrap() {
        OperationOutcome::Succeeded(LocalTempLifecycleResult::Sealed(lease)) => lease,
        _ => panic!("temp seal failed"),
    };
    assert_eq!(lease.resource_id(), resource_id);

    let commands = [
        LocalTempLifecycleCommand::Keep,
        LocalTempLifecycleCommand::Persist {
            target: directory.path().join("should-not-persist"),
            options: LocalPersistOptions::new(),
        },
        LocalTempLifecycleCommand::Seal {
            options: qubit_local_files::LocalSealOptions::default(),
        },
    ];
    let mut access = access;
    for command in commands {
        let (mut operation, returned_access, _) = async_filesystem
            .submit_temp_lifecycle(
                &manager,
                Default::default(),
                LocalTempLifecycleRequest { access, command },
            )
            .unwrap();
        assert_eq!(returned_access.resource_id(), resource_id);
        assert!(matches!(
            block_on(operation.wait_outcome()).unwrap(),
            OperationOutcome::Failed(LocalTempLifecycleError::Unsupported(_))
        ));
        access = returned_access;
    }
    let exported = block_on(lease.export_read_handle(&executor)).unwrap();
    assert_eq!(exported.lease.resource_id(), resource_id);
    drop(exported);
    drop(lease);
    drop(access);
    drop(block_on(seal.wait_settlement()));
    drop(block_on(creation.wait_settlement()));
    drop(seal);
    drop(creation);
    drop(async_filesystem);
    let cleanup_deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while std::fs::read_dir(directory.path()).unwrap().next().is_some() && std::time::Instant::now() < cleanup_deadline
    {
        std::thread::yield_now();
    }
    assert_eq!(0, std::fs::read_dir(directory.path()).unwrap().count());
    let executor_shutdown = executor
        .request_shutdown(qubit_operation::ShutdownMode::Drain)
        .wait()
        .expect("executor group shutdown should complete after lifecycle rejection");
    assert_eq!(executor_shutdown.snapshot.active_operations, 0);
    drop(executor);
    let manager_shutdown = manager
        .request_shutdown(qubit_operation::ShutdownMode::Drain)
        .wait()
        .expect("manager shutdown should release temporary-resource tracking");
    assert_eq!(manager_shutdown.snapshot.active_operations, 0);
    assert_eq!(manager_shutdown.snapshot.recovery_groups, 0);
}

#[test]
fn test_temp_directory_keep_and_persist_are_supported() {
    use qubit_local_files::LocalFileSystem;
    use qubit_local_files::operation::publish::LocalTempLifecycleCommand;
    use qubit_local_files::operation::publish::LocalTempLifecycleRequest;
    use qubit_local_files::operation::publish::LocalTempLifecycleResult;
    use qubit_local_files::operation::submit_temp_directory;
    use qubit_local_files::options::LocalPersistOptions;
    use qubit_local_files::options::LocalTempDirectoryOptions;

    let directory = tempfile::tempdir().unwrap();
    let manager = OperationManager::new(ManagerOptions::default()).unwrap();
    let executor = LocalIoExecutor::new(manager.clone(), LocalExecutionConfig::default()).unwrap();
    let filesystem = LocalFileSystem::host().unwrap();
    let async_filesystem = block_on(AsyncLocalFileSystem::host(executor.clone())).unwrap();

    let mut persist_creation = submit_temp_directory(
        filesystem.clone(),
        executor.clone(),
        &manager.as_async(),
        Default::default(),
        LocalTempDirectoryOptions::new().with_parent(directory.path()),
    )
    .unwrap();
    let persist_access = match block_on(persist_creation.wait_outcome()).unwrap() {
        OperationOutcome::Succeeded(access) => access,
        _ => panic!("temp directory creation failed"),
    };
    let target = directory.path().join("persisted-directory");
    let (mut persist, _, _) = async_filesystem
        .submit_temp_lifecycle(
            &manager,
            Default::default(),
            LocalTempLifecycleRequest {
                access: persist_access,
                command: LocalTempLifecycleCommand::Persist {
                    target: target.clone(),
                    options: LocalPersistOptions::new(),
                },
            },
        )
        .unwrap();
    assert!(matches!(
        block_on(persist.wait_outcome()).unwrap(),
        OperationOutcome::Succeeded(LocalTempLifecycleResult::Persisted(_))
    ));
    assert!(target.is_dir());
    std::fs::remove_dir_all(target).unwrap();

    let mut keep_creation = submit_temp_directory(
        filesystem,
        executor,
        &manager.as_async(),
        Default::default(),
        LocalTempDirectoryOptions::new().with_parent(directory.path()),
    )
    .unwrap();
    let keep_access = match block_on(keep_creation.wait_outcome()).unwrap() {
        OperationOutcome::Succeeded(access) => access,
        _ => panic!("temp directory creation failed"),
    };
    let (mut keep, _, _) = async_filesystem
        .submit_temp_lifecycle(
            &manager,
            Default::default(),
            LocalTempLifecycleRequest {
                access: keep_access,
                command: LocalTempLifecycleCommand::Keep,
            },
        )
        .unwrap();
    let kept = match block_on(keep.wait_outcome()).unwrap() {
        OperationOutcome::Succeeded(LocalTempLifecycleResult::Persisted(outcome)) => outcome,
        _ => panic!("temp directory keep failed"),
    };
    assert!(kept.path().is_dir());
    std::fs::remove_dir_all(kept.path()).unwrap();
}

#[test]
fn test_temp_lifecycle_rejects_wrong_manager_and_returns_original_access() {
    use qubit_local_files::LocalFileSystem;
    use qubit_local_files::operation::LocalSubmitRejection;
    use qubit_local_files::operation::publish::LocalTempLifecycleCommand;
    use qubit_local_files::operation::publish::LocalTempLifecycleRequest;
    use qubit_local_files::operation::publish::LocalTempLifecycleResult;
    use qubit_local_files::operation::submit_temp_file;
    use qubit_local_files::options::LocalTempFileOptions;

    let directory = tempfile::tempdir().unwrap();
    let manager = OperationManager::new(ManagerOptions::default()).unwrap();
    let other_manager = OperationManager::new(ManagerOptions::default()).unwrap();
    let executor = LocalIoExecutor::new(manager.clone(), LocalExecutionConfig::default()).unwrap();
    let filesystem = LocalFileSystem::host().unwrap();
    let async_filesystem = block_on(AsyncLocalFileSystem::host(executor.clone())).unwrap();
    let mut creation = submit_temp_file(
        filesystem,
        executor,
        &manager.as_async(),
        Default::default(),
        LocalTempFileOptions::new().with_parent(directory.path()),
    )
    .unwrap();
    let access = match block_on(creation.wait_outcome()).unwrap() {
        OperationOutcome::Succeeded(access) => access,
        _ => panic!("temp file creation failed"),
    };
    let resource_id = access.resource_id();

    let rejection = match async_filesystem.submit_temp_lifecycle(
        &other_manager,
        Default::default(),
        LocalTempLifecycleRequest {
            access,
            command: LocalTempLifecycleCommand::Keep,
        },
    ) {
        Err(rejection) => rejection,
        Ok(_) => panic!("a different manager cannot submit to this executor"),
    };
    assert_eq!(rejection.rejection(), LocalSubmitRejection::WrongManager);
    let request = rejection.into_request();
    assert_eq!(request.access.resource_id(), resource_id);

    let (mut kept, returned_access, _) = async_filesystem
        .submit_temp_lifecycle(
            &manager,
            Default::default(),
            LocalTempLifecycleRequest {
                access: request.access,
                command: LocalTempLifecycleCommand::Keep,
            },
        )
        .unwrap();
    assert_eq!(returned_access.resource_id(), resource_id);
    let persisted = match block_on(kept.wait_outcome()).unwrap() {
        OperationOutcome::Succeeded(LocalTempLifecycleResult::Persisted(outcome)) => outcome,
        _ => panic!("temp keep failed after recovering the rejected request"),
    };
    assert!(persisted.path().exists());
    std::fs::remove_file(persisted.path()).unwrap();
}

#[cfg(feature = "test-support")]
#[test]
fn test_queued_temp_persist_observes_stop_before_publishing() {
    use qubit_local_files::LocalFileSystem;
    use qubit_local_files::operation::publish::LocalTempLifecycleCommand;
    use qubit_local_files::operation::publish::LocalTempLifecycleRequest;
    use qubit_local_files::operation::submit_temp_file;
    use qubit_local_files::options::LocalPersistOptions;
    use qubit_local_files::options::LocalTempFileOptions;
    use qubit_local_files::test_support::install_test_gate;
    use qubit_operation::StopReason;

    let directory = tempfile::tempdir().unwrap();
    let target = directory.path().join("stopped-persist");
    let manager = OperationManager::new(ManagerOptions::default()).unwrap();
    let executor = LocalIoExecutor::new(manager.clone(), LocalExecutionConfig::default()).unwrap();
    let filesystem = LocalFileSystem::host().unwrap();
    let async_filesystem = block_on(AsyncLocalFileSystem::host(executor.clone())).unwrap();
    let mut creation = submit_temp_file(
        filesystem,
        executor,
        &manager.as_async(),
        Default::default(),
        LocalTempFileOptions::new().with_parent(directory.path()),
    )
    .unwrap();
    let access = match block_on(creation.wait_outcome()).unwrap() {
        OperationOutcome::Succeeded(access) => access,
        _ => panic!("temp creation failed"),
    };
    let gate = install_test_gate("lf7-temp-before-persist-primitive").unwrap();
    let (mut operation, same_access, _) = async_filesystem
        .submit_temp_lifecycle(
            &manager,
            Default::default(),
            LocalTempLifecycleRequest {
                access,
                command: LocalTempLifecycleCommand::Persist {
                    target: target.clone(),
                    options: LocalPersistOptions::new(),
                },
            },
        )
        .unwrap();
    gate.wait_until_entered();
    let _ = operation.request_stop(StopReason::UserCancelled);
    gate.release();
    let _ = block_on(operation.wait_outcome()).unwrap();
    block_on(operation.wait_settlement());
    assert!(!target.exists(), "queued stop must prevent temp publication");

    let (mut kept, _, _) = async_filesystem
        .submit_temp_lifecycle(
            &manager,
            Default::default(),
            LocalTempLifecycleRequest {
                access: same_access,
                command: LocalTempLifecycleCommand::Keep,
            },
        )
        .unwrap();
    assert!(matches!(
        block_on(kept.wait_outcome()).unwrap(),
        OperationOutcome::Succeeded(_)
    ));
}

#[cfg(feature = "test-support")]
#[test]
fn test_cancellation_at_last_precommit_gate_aborts_without_publishing() {
    use qubit_local_files::test_support::install_test_gate_for_path;
    use qubit_operation::StopReason;

    let _gate_lock = WRITER_PUBLISH_GATE_TEST_LOCK
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let directory = tempfile::tempdir().unwrap();
    let destination = directory.path().join("cancelled-at-gate");
    let manager = OperationManager::new(ManagerOptions::default()).unwrap();
    let filesystem = facade(&manager);
    let gate = Arc::new(install_test_gate_for_path("lf7-writer-before-publish", &destination).unwrap());
    let (mut operation, observer) = filesystem
        .submit_writer_commit(
            &manager,
            Default::default(),
            LocalWriterCommitRequest {
                path: destination.clone(),
                options: LocalWriteOptions::new(LocalWriteMode::CreateOrReplace),
                bytes: b"never visible".to_vec(),
            },
            LocalOperationOptions::default(),
        )
        .unwrap();
    let gate_waiter = Arc::clone(&gate);
    let (entered_sender, entered_receiver) = std::sync::mpsc::channel();
    let gate_waiter = std::thread::spawn(move || {
        gate_waiter.wait_until_entered();
        let _ = entered_sender.send(());
    });
    let target_reached_gate = entered_receiver.recv_timeout(std::time::Duration::from_secs(5)).is_ok();
    if !target_reached_gate {
        let _ = operation.request_stop(StopReason::UserCancelled);
        gate.release();
        let _ = gate_waiter.join();
        let outcome = operation.wait_outcome();
        panic!("target writer did not reach its path-scoped publish gate; outcome: {outcome:?}");
    }
    gate_waiter.join().expect("gate waiter should exit after target entry");

    let unrelated_destination = directory.path().join("unrelated-writer");
    let (mut unrelated_operation, _) = filesystem
        .submit_writer_commit(
            &manager,
            Default::default(),
            LocalWriterCommitRequest {
                path: unrelated_destination.clone(),
                options: LocalWriteOptions::new(LocalWriteMode::CreateOrReplace),
                bytes: b"unrelated writer proceeds".to_vec(),
            },
            LocalOperationOptions::default(),
        )
        .unwrap();
    let (unrelated_sender, unrelated_receiver) = std::sync::mpsc::channel();
    let unrelated_waiter = std::thread::spawn(move || {
        let succeeded = matches!(
            unrelated_operation.wait_outcome().unwrap(),
            OperationOutcome::Succeeded(_)
        );
        let _ = unrelated_sender.send(succeeded);
    });
    let unrelated_result_before_release = unrelated_receiver.recv_timeout(std::time::Duration::from_secs(5)).ok();
    let unrelated_completed_before_release = unrelated_result_before_release.is_some();

    operation.request_stop(StopReason::UserCancelled);
    gate.release();
    let unrelated_succeeded = unrelated_result_before_release.unwrap_or_else(|| {
        unrelated_receiver
            .recv_timeout(std::time::Duration::from_secs(5))
            .unwrap_or(false)
    });
    unrelated_waiter.join().expect("unrelated writer waiter should exit");
    assert!(
        unrelated_completed_before_release,
        "a different destination must not enter the target writer's gate"
    );
    assert!(unrelated_succeeded);
    assert!(unrelated_destination.exists());
    assert!(matches!(
        operation.wait_outcome().unwrap(),
        OperationOutcome::Stopped(_)
    ));
    assert!(!destination.exists());
    assert_eq!(
        observer.snapshot().publication,
        qubit_local_files::operation::LocalPublicationState::NotPublished
    );
}

#[cfg(feature = "test-support")]
#[test]
fn test_failed_abort_after_stop_returns_retryable_writer_and_cleanup_error() {
    use qubit_local_files::test_support::install_test_fault_shared;
    use qubit_local_files::test_support::install_test_gate_for_path;
    use qubit_operation::StopReason;

    let _gate_lock = WRITER_PUBLISH_GATE_TEST_LOCK
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let _support_lock = PUBLISH_TEST_SUPPORT_LOCK
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let directory = tempfile::tempdir().unwrap();
    let destination = directory.path().join("abort-recovery");
    let manager = OperationManager::new(ManagerOptions::default()).unwrap();
    let executor = LocalIoExecutor::new(manager.clone(), LocalExecutionConfig::default()).unwrap();
    let filesystem = block_on(AsyncLocalFileSystem::host(executor.clone())).unwrap();
    let gate = install_test_gate_for_path("lf7-writer-before-publish", &destination).unwrap();
    let (mut operation, _) = filesystem
        .submit_writer_commit(
            &manager,
            Default::default(),
            LocalWriterCommitRequest {
                path: destination.clone(),
                options: LocalWriteOptions::new(LocalWriteMode::CreateOrReplace),
                bytes: b"retained staging".to_vec(),
            },
            LocalOperationOptions::default(),
        )
        .unwrap();
    gate.wait_until_entered();
    let fault = install_test_fault_shared("atomic-install-unlink-persistent").unwrap();
    let _ = operation.request_stop(StopReason::UserCancelled);
    gate.release();
    assert!(matches!(
        operation.wait_outcome().unwrap(),
        OperationOutcome::Stopped(_)
    ));
    let settlement = operation.wait_settlement().unwrap();
    assert!(settlement.recovery_count > 0);
    let recovery = settlement
        .recoveries
        .first()
        .expect("failed cleanup should expose a recovery handle")
        .clone();
    drop(settlement);
    drop(fault);
    let report = recovery.try_retry().unwrap().wait().unwrap();
    assert_eq!(report.recovery.phase, qubit_operation::RecoveryPhase::Resolved);
    assert!(!destination.exists());
    drop(report);
    let shutdown_report = executor
        .request_shutdown(qubit_operation::ShutdownMode::Drain)
        .wait()
        .expect("executor group shutdown should complete after recovery");
    assert_eq!(shutdown_report.snapshot.active_operations, 0);
    drop(recovery);
    drop(operation);
    drop(filesystem);
    drop(executor);
    let shutdown_report = manager
        .request_shutdown(qubit_operation::ShutdownMode::Drain)
        .wait()
        .expect("manager shutdown should release recovery tracking");
    assert_eq!(shutdown_report.snapshot.active_operations, 0);
    assert_eq!(shutdown_report.snapshot.recovery_groups, 0);
}

#[cfg(feature = "test-support")]
#[test]
fn test_append_effect_is_published_before_worker_panic_and_cleanup_releases_writer() {
    use qubit_local_files::test_support::install_test_fault_shared;

    let _support_lock = PUBLISH_TEST_SUPPORT_LOCK
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let directory = tempfile::tempdir().unwrap();
    let destination = directory.path().join("append-panic");
    std::fs::write(&destination, b"prefix:").unwrap();
    let manager = OperationManager::new(ManagerOptions::default()).unwrap();
    let filesystem = facade(&manager);
    let fault = install_test_fault_shared("lf7-append-panic-after-write").unwrap();
    let (mut operation, observer) = filesystem
        .submit_writer_commit(
            &manager,
            Default::default(),
            LocalWriterCommitRequest {
                path: destination.clone(),
                options: LocalWriteOptions::new(LocalWriteMode::Append),
                bytes: b"suffix".to_vec(),
            },
            LocalOperationOptions::default(),
        )
        .unwrap();
    assert!(matches!(
        operation.wait_outcome().unwrap(),
        OperationOutcome::ExecutionFailed(_)
    ));
    drop(fault);
    assert_eq!(std::fs::read(destination).unwrap(), b"prefix:suffix");
    let facts = observer.snapshot();
    assert_eq!(
        facts.publication,
        qubit_local_files::operation::LocalPublicationState::Published
    );
    assert_eq!(facts.modified_bytes, 6);
    assert_eq!(facts.modified_count, 1);
    let settlement = operation.wait_settlement().unwrap();
    assert_eq!(settlement.released_count, 1);
    assert_eq!(settlement.recovery_count, 0);
}

#[cfg(all(feature = "test-support", unix))]
#[test]
fn test_append_sync_failure_preserves_published_bytes_and_failed_durability() {
    use qubit_local_files::operation::publish::LocalWriterCommandError;
    use qubit_local_files::policy::LocalDurabilityRequirement;
    use qubit_local_files::test_support::install_test_fault_shared;

    let _support_lock = PUBLISH_TEST_SUPPORT_LOCK
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let directory = tempfile::tempdir().unwrap();
    let destination = directory.path().join("append-sync-failure");
    std::fs::write(&destination, b"prefix:").unwrap();
    let manager = OperationManager::new(ManagerOptions::default()).unwrap();
    let filesystem = facade(&manager);
    let fault = install_test_fault_shared("writer-append-required-sync").unwrap();
    let (mut operation, observer) = filesystem
        .submit_writer_commit(
            &manager,
            Default::default(),
            LocalWriterCommitRequest {
                path: destination.clone(),
                options: LocalWriteOptions::new(LocalWriteMode::Append)
                    .with_durability(LocalDurabilityRequirement::Required),
                bytes: b"suffix".to_vec(),
            },
            LocalOperationOptions::default(),
        )
        .unwrap();
    assert!(matches!(
        operation.wait_outcome().unwrap(),
        OperationOutcome::Failed(LocalWriterCommandError::Commit(_))
    ));
    drop(fault);
    assert_eq!(std::fs::read(destination).unwrap(), b"prefix:suffix");
    let facts = observer.snapshot();
    assert_eq!(
        facts.publication,
        qubit_local_files::operation::LocalPublicationState::Published
    );
    assert_eq!(
        facts.durability,
        qubit_local_files::operation::LocalDurabilityState::Failed
    );
    assert_eq!(facts.modified_bytes, 6);
    assert_eq!(facts.modified_count, 1);
}

// This scenario requires native directory synchronization. Windows rejects a
// Required durability request before publication because that guarantee is
// unavailable there; its capability preflight is covered separately.
#[cfg(all(feature = "test-support", not(windows)))]
#[test]
fn test_stop_during_native_publish_preserves_published_fact_when_parent_sync_fails() {
    use qubit_local_files::operation::publish::LocalWriterCommandError;
    use qubit_local_files::outcome::LocalWriteFailureState;
    use qubit_local_files::policy::LocalDurabilityRequirement;
    use qubit_local_files::test_support::install_test_fault_shared;
    use qubit_local_files::test_support::install_test_gate_for_path;
    use qubit_operation::StopReason;

    let _support_lock = PUBLISH_TEST_SUPPORT_LOCK
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let directory = tempfile::tempdir().unwrap();
    let canonical_root = std::fs::canonicalize(directory.path()).unwrap();
    let destination = canonical_root.join("nested/created/payload");
    let manager = OperationManager::new(ManagerOptions::default()).unwrap();
    let filesystem = facade(&manager);
    let gate = install_test_gate_for_path("atomic-writer-after-install-before-parent-sync", &destination).unwrap();
    let _fault = install_test_fault_shared("atomic-writer-created-parent-sync").unwrap();
    let (mut operation, observer) = filesystem
        .submit_writer_commit(
            &manager,
            Default::default(),
            LocalWriterCommitRequest {
                path: destination.clone(),
                options: LocalWriteOptions::new(LocalWriteMode::CreateNew)
                    .with_create_parent()
                    .with_durability(LocalDurabilityRequirement::Required),
                bytes: b"visible despite stop".to_vec(),
            },
            LocalOperationOptions::default(),
        )
        .unwrap();
    gate.wait_until_entered();
    operation.request_stop(StopReason::UserCancelled);
    gate.release();
    let outcome = operation.wait_outcome().unwrap();
    assert!(
        matches!(
            outcome,
            OperationOutcome::Failed(LocalWriterCommandError::Commit(ref error))
                if error.state() == LocalWriteFailureState::Published
        ) || matches!(outcome, OperationOutcome::Stopped(_)),
        "unexpected outcome after stop during installed publication: {outcome:?}"
    );
    operation.wait_settlement().unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    while observer.snapshot().publication != qubit_local_files::operation::LocalPublicationState::Published
        && std::time::Instant::now() < deadline
    {
        std::thread::yield_now();
    }
    assert_eq!(std::fs::read(destination).unwrap(), b"visible despite stop");
    assert_eq!(
        observer.snapshot().publication,
        qubit_local_files::operation::LocalPublicationState::Published,
        "outcome={outcome:?}, facts={:?}",
        observer.snapshot()
    );
    assert_eq!(
        observer.snapshot().durability,
        qubit_local_files::operation::LocalDurabilityState::Failed
    );
}

#[test]
fn test_partial_tree_cleanup_failure_keeps_a_retryable_recovery_owner() {
    use qubit_local_files::directory::LocalTreeRetainedResource;
    use qubit_local_files::operation::publish::LocalStagedTreeFile;
    use qubit_local_files::operation::publish::LocalStagedTreePublishError;
    use qubit_local_files::operation::publish::LocalStagedTreePublishRequest;
    use qubit_local_files::options::LocalTempCleanupLimits;
    use qubit_local_files::options::LocalTempDirectoryOptions;
    use qubit_local_files::options::LocalTreePublishOptions;

    let directory = tempfile::tempdir().unwrap();
    let manager = OperationManager::new(ManagerOptions::default()).unwrap();
    let filesystem = facade(&manager);
    let (mut operation, observer) = filesystem
        .submit_staged_tree_publish(
            &manager,
            Default::default(),
            LocalStagedTreePublishRequest {
                parent: PathBuf::from(directory.path()),
                name: "unpublished".into(),
                stage_options: LocalTempDirectoryOptions::new()
                    .with_cleanup_limits(LocalTempCleanupLimits::new().with_max_entries(0)),
                files: vec![
                    LocalStagedTreeFile {
                        path: PathBuf::from("same"),
                        bytes: b"first".to_vec(),
                    },
                    LocalStagedTreeFile {
                        path: PathBuf::from("same"),
                        bytes: b"second".to_vec(),
                    },
                ],
                publish_options: LocalTreePublishOptions::new(),
            },
            LocalOperationOptions::default(),
        )
        .unwrap();
    let stage = match operation.wait_outcome().unwrap() {
        OperationOutcome::Failed(LocalStagedTreePublishError::Write { stage, .. }) => stage,
        _ => panic!("expected a partial staged write failure"),
    };
    assert_eq!(observer.snapshot().modified_count, 2);
    assert!(observer.snapshot().range_unknown);
    let cleanup = stage
        .cleanup()
        .expect_err("cleanup limits should retain the staged tree");
    let (cleanup, reservation) = cleanup.into_parts();
    assert!(reservation.bytes() >= 1);
    let mut recovery: Vec<LocalTreeRetainedResource> = cleanup.into_retained_resources();
    assert_eq!(recovery.len(), 1);
    let mut retained = recovery.pop().unwrap();
    retained.set_cleanup_limits(LocalTempCleanupLimits::new());
    assert!(retained.cleanup().is_ok());
    drop(reservation);
}

#[test]
fn test_writer_open_failure_returns_error_without_recording_publication() {
    let directory = tempfile::tempdir().unwrap();
    let destination = directory.path().join("missing-parent").join("published");
    let manager = OperationManager::new(ManagerOptions::default()).unwrap();
    let filesystem = facade(&manager);
    let (mut operation, observer) = filesystem
        .submit_writer_commit(
            &manager,
            Default::default(),
            LocalWriterCommitRequest {
                path: destination.clone(),
                options: LocalWriteOptions::new(LocalWriteMode::CreateOrReplace),
                bytes: b"candidate".to_vec(),
            },
            LocalOperationOptions::default(),
        )
        .unwrap();

    assert!(matches!(
        operation.wait_outcome().unwrap(),
        OperationOutcome::Failed(qubit_local_files::operation::publish::LocalWriterCommandError::Open(_))
    ));
    assert!(!destination.exists());
    let facts = observer.snapshot();
    assert_eq!(
        facts.publication,
        qubit_local_files::operation::LocalPublicationState::NotPublished
    );
    assert_eq!(facts.modified_count, 0);
    assert_eq!(facts.modified_bytes, 0);
}

#[cfg(feature = "digest")]
#[test]
fn test_conditional_writer_open_failure_does_not_claim_a_destination_change() {
    use qubit_local_files::operation::publish::LocalConditionalWriterCommitRequest;
    use qubit_local_files::options::LocalWritePrecondition;
    use sha2::Sha256;

    let directory = tempfile::tempdir().unwrap();
    let root = std::fs::canonicalize(directory.path()).unwrap();
    let destination = root.join("missing-parent").join("published");
    let manager = OperationManager::new(ManagerOptions::default()).unwrap();
    let filesystem = facade(&manager)
        .with_symlink_policy(qubit_local_files::policy::LocalSymlinkPolicy::Reject)
        .unwrap();
    let (mut operation, observer) = filesystem
        .submit_conditional_writer_commit::<Sha256>(
            &manager,
            Default::default(),
            LocalConditionalWriterCommitRequest {
                path: destination.clone(),
                precondition: LocalWritePrecondition::Absent,
                options: LocalWriteOptions::new(LocalWriteMode::CreateOrReplace),
                bytes: b"candidate".to_vec(),
            },
            LocalOperationOptions::default(),
        )
        .unwrap();

    assert!(matches!(
        operation.wait_outcome().unwrap(),
        OperationOutcome::Failed(qubit_local_files::operation::publish::LocalConditionalWriterCommandError::Open(_))
    ));
    assert!(!destination.exists());
    let facts = observer.snapshot();
    assert_eq!(
        facts.publication,
        qubit_local_files::operation::LocalPublicationState::NotPublished
    );
    assert_eq!(facts.modified_count, 0);
    assert_eq!(facts.modified_bytes, 0);
}
#[test]
fn test_staged_tree_publish_preserves_caller_validation_limits_and_recovery() {
    use qubit_local_files::directory::LocalTreeValidationOptions;
    use qubit_local_files::operation::publish::LocalStagedTreeFile;
    use qubit_local_files::operation::publish::LocalStagedTreePublishError;
    use qubit_local_files::operation::publish::LocalStagedTreePublishRequest;
    use qubit_local_files::options::LocalListOptions;
    use qubit_local_files::options::LocalTempDirectoryOptions;
    use qubit_local_files::options::LocalTreePublishOptions;

    let directory = tempfile::tempdir().unwrap();
    let manager = OperationManager::new(ManagerOptions::default()).unwrap();
    let filesystem = facade(&manager);
    let (mut operation, _) = filesystem
        .submit_staged_tree_publish(
            &manager,
            Default::default(),
            LocalStagedTreePublishRequest {
                parent: directory.path().into(),
                name: "limited-validation".into(),
                stage_options: LocalTempDirectoryOptions::new(),
                files: vec![LocalStagedTreeFile {
                    path: PathBuf::from("payload"),
                    bytes: b"staged".to_vec(),
                }],
                publish_options: LocalTreePublishOptions::new().with_validation(
                    LocalTreeValidationOptions::new()
                        .with_limits(LocalListOptions::new().with_max_entries(0)),
                ),
            },
            LocalOperationOptions::default(),
        )
        .unwrap();

    match operation.wait_outcome().unwrap() {
        OperationOutcome::Failed(LocalStagedTreePublishError::Publish(error)) => {
            assert!(error.cause().resource_limit_error().is_some());
            let (native, _reservation) = error.into_parts();
            assert!(native.cause().resource_limit_error().is_some());
            let (_cause, stage, _retained) = native.into_parts();
            let retained = stage.expect("validation failure retains stage");
            assert!(retained.observation().is_ok());
            retained.cleanup().unwrap();
        }
        outcome => panic!("caller validation limit must prevent publication, got {outcome:?}"),
    }
    assert!(!directory.path().join("limited-validation").exists());
}

#[test]
fn test_staged_tree_publish_rejects_handle_limit_before_admission() {
    use qubit_local_files::operation::LocalSubmitRejection;
    use qubit_local_files::operation::publish::LocalStagedTreeFile;
    use qubit_local_files::operation::publish::LocalStagedTreePublishRequest;
    use qubit_local_files::options::LocalTempDirectoryOptions;
    use qubit_local_files::options::LocalTreePublishOptions;

    let directory = tempfile::tempdir().unwrap();
    let manager = OperationManager::new(ManagerOptions::default()).unwrap();
    let filesystem = facade(&manager);
    let request = LocalStagedTreePublishRequest {
        parent: directory.path().into(),
        name: "insufficient-handles".into(),
        stage_options: LocalTempDirectoryOptions::new(),
        files: vec![LocalStagedTreeFile {
            path: PathBuf::from("payload"),
            bytes: b"staged".to_vec(),
        }],
        publish_options: LocalTreePublishOptions::new()
            .with_owner_access_policy(qubit_local_files::policy::LocalOwnerAccessPolicy::new()),
    };
    let rejected = match filesystem.submit_staged_tree_publish(
        &manager,
        Default::default(),
        request,
        LocalOperationOptions {
            max_open_handles: Some(2),
            ..LocalOperationOptions::default()
        },
    ) {
        Err(error) => error,
        Ok(_) => {
            panic!("owner-policy root handles must be admitted against the operation handle limit")
        }
    };
    assert_eq!(rejected.rejection(), LocalSubmitRejection::CapacityExceeded);
    assert!(!directory.path().join("insufficient-handles").exists());
}

#[cfg(target_os = "linux")]
#[test]
fn test_staged_recovery_retains_and_refunds_handle_quota_across_cleanup() {
    use qubit_local_files::operation::publish::LocalStagedTreeFile;
    use qubit_local_files::operation::publish::LocalStagedTreePublishError;
    use qubit_local_files::operation::publish::LocalStagedTreePublishRequest;
    use qubit_local_files::options::LocalTempDirectoryOptions;
    use qubit_local_files::options::LocalTreePublishOptions;

    let directory = tempfile::tempdir().unwrap();
    let manager = OperationManager::new(ManagerOptions::default()).unwrap();
    let executor = LocalIoExecutor::new(
        manager.clone(),
        LocalExecutionConfig {
            max_open_handles: 4,
            ..LocalExecutionConfig::default()
        },
    )
    .unwrap();
    let filesystem = block_on(AsyncLocalFileSystem::host(executor)).unwrap();
    let (mut operation, _) = filesystem
        .submit_staged_tree_publish(
            &manager,
            Default::default(),
            LocalStagedTreePublishRequest {
                parent: directory.path().into(),
                name: "retained-quota".into(),
                stage_options: LocalTempDirectoryOptions::new(),
                files: vec![
                    LocalStagedTreeFile {
                        path: PathBuf::from("duplicate"),
                        bytes: b"first".to_vec(),
                    },
                    LocalStagedTreeFile {
                        path: PathBuf::from("duplicate"),
                        bytes: b"second".to_vec(),
                    },
                ],
                publish_options: LocalTreePublishOptions::new().with_owner_access_policy(
                    qubit_local_files::policy::LocalOwnerAccessPolicy::new()
                        .with_unix_modes(0o700, 0o600),
                ),
            },
            LocalOperationOptions::default(),
        )
        .unwrap();
    let recovery = match operation.wait_outcome().unwrap() {
        OperationOutcome::Failed(LocalStagedTreePublishError::Write { stage, .. }) => stage,
        outcome => panic!("duplicate write should leave a recoverable stage, got {outcome:?}"),
    };

    let writer = LocalWriterCommitRequest {
        path: directory.path().join("after-recovery"),
        options: LocalWriteOptions::new(LocalWriteMode::CreateOrReplace),
        bytes: b"writer".to_vec(),
    };
    let (mut writer_operation, _) = filesystem
        .submit_writer_commit(
            &manager,
            Default::default(),
            writer,
            LocalOperationOptions::default(),
        )
        .expect("shrunk recovery quota leaves capacity for this writer");
    assert!(matches!(writer_operation.wait_outcome().unwrap(), OperationOutcome::Succeeded(_)));

    recovery.cleanup().unwrap();
    let (mut writer, _) = filesystem
        .submit_writer_commit(
            &manager,
            Default::default(),
            LocalWriterCommitRequest {
                path: directory.path().join("after-recovery"),
                options: LocalWriteOptions::new(LocalWriteMode::CreateOrReplace),
                bytes: b"writer".to_vec(),
            },
            LocalOperationOptions::default(),
        )
        .expect("successful cleanup refunds the retained handle quota");
    assert!(matches!(
        writer.wait_outcome().unwrap(),
        OperationOutcome::Succeeded(_)
    ));
}

#[test]
fn test_successful_staged_publish_refunds_transient_and_root_handle_quota() {
    use qubit_local_files::operation::publish::LocalStagedTreeFile;
    use qubit_local_files::operation::publish::LocalStagedTreePublishRequest;
    use qubit_local_files::options::LocalTempDirectoryOptions;
    use qubit_local_files::options::LocalTreePublishOptions;

    let directory = tempfile::tempdir().unwrap();
    let manager = OperationManager::new(ManagerOptions::default()).unwrap();
    let executor = LocalIoExecutor::new(
        manager.clone(),
        LocalExecutionConfig {
            max_open_handles: 3,
            ..LocalExecutionConfig::default()
        },
    )
    .unwrap();
    let filesystem = block_on(AsyncLocalFileSystem::host(executor)).unwrap();
    let (mut operation, _) = filesystem
        .submit_staged_tree_publish(
            &manager,
            Default::default(),
            LocalStagedTreePublishRequest {
                parent: directory.path().into(),
                name: "success-refund".into(),
                stage_options: LocalTempDirectoryOptions::new(),
                files: vec![LocalStagedTreeFile {
                    path: PathBuf::from("payload"),
                    bytes: b"done".to_vec(),
                }],
                publish_options: LocalTreePublishOptions::new(),
            },
            LocalOperationOptions::default(),
        )
        .unwrap();
    assert!(matches!(
        operation.wait_outcome().unwrap(),
        OperationOutcome::Succeeded(_)
    ));

    let (mut writer, _) = filesystem
        .submit_writer_commit(
            &manager,
            Default::default(),
            LocalWriterCommitRequest {
                path: directory.path().join("after-success"),
                options: LocalWriteOptions::new(LocalWriteMode::CreateOrReplace),
                bytes: b"writer".to_vec(),
            },
            LocalOperationOptions::default(),
        )
        .expect("successful publication refunds all staged handle quota");
    assert!(matches!(
        writer.wait_outcome().unwrap(),
        OperationOutcome::Succeeded(_)
    ));
}

#[test]
fn test_staged_publish_checks_caller_exact_manifest_against_generated_manifest() {
    use qubit_local_files::directory::LocalTreeEntry;
    use qubit_local_files::operation::publish::LocalStagedTreeFile;
    use qubit_local_files::operation::publish::LocalStagedTreePublishError;
    use qubit_local_files::operation::publish::LocalStagedTreePublishRequest;
    use qubit_local_files::options::LocalTempDirectoryOptions;
    use qubit_local_files::options::LocalTreePublishOptions;

    let directory = tempfile::tempdir().unwrap();
    let manager = OperationManager::new(ManagerOptions::default()).unwrap();
    let filesystem = facade(&manager);
    let wrong = LocalTreeEntry::new(
        PathBuf::from("other"),
        qubit_local_files::outcome::LocalFileKind::File,
    )
    .unwrap();
    let (mut operation, _) = filesystem
        .submit_staged_tree_publish(
            &manager,
            Default::default(),
            LocalStagedTreePublishRequest {
                parent: directory.path().into(),
                name: "manifest-conflict".into(),
                stage_options: LocalTempDirectoryOptions::new(),
                files: vec![LocalStagedTreeFile {
                    path: PathBuf::from("payload"),
                    bytes: b"payload".to_vec(),
                }],
                publish_options: LocalTreePublishOptions::new().with_validation(
                    qubit_local_files::directory::LocalTreeValidationOptions::new()
                        .with_exact_entries(vec![wrong]),
                ),
            },
            LocalOperationOptions::default(),
        )
        .unwrap();
    match operation.wait_outcome().unwrap() {
        OperationOutcome::Failed(LocalStagedTreePublishError::Publish(error)) => {
            assert_eq!(
                error.cause().kind(),
                qubit_local_files::error::LocalFileErrorKind::InvalidOptions
            );
            let (native, _) = error.into_parts();
            let (_, stage, _) = native.into_parts();
            stage
                .expect("manifest mismatch preserves stage")
                .cleanup()
                .unwrap();
        }
        outcome => panic!("conflicting exact manifest must prevent publication, got {outcome:?}"),
    }
    assert!(!directory.path().join("manifest-conflict").exists());
}

#[cfg(target_os = "linux")]
#[test]
fn test_recovery_charges_retained_roots_and_rejects_over_remaining_quota() {
    use qubit_local_files::operation::LocalSubmitRejection;
    use qubit_local_files::operation::publish::LocalStagedTreeFile;
    use qubit_local_files::operation::publish::LocalStagedTreePublishError;
    use qubit_local_files::operation::publish::LocalStagedTreePublishRequest;
    use qubit_local_files::options::LocalTempDirectoryOptions;
    use qubit_local_files::options::LocalTreePublishOptions;
    use qubit_local_files::policy::LocalOwnerAccessPolicy;

    let directory = tempfile::tempdir().unwrap();
    let manager = OperationManager::new(ManagerOptions::default()).unwrap();
    let executor = LocalIoExecutor::new(
        manager.clone(),
        LocalExecutionConfig {
            max_open_handles: 5,
            ..LocalExecutionConfig::default()
        },
    )
    .unwrap();
    let filesystem = block_on(AsyncLocalFileSystem::host(executor)).unwrap();
    let (mut write_operation, _) = filesystem
        .submit_staged_tree_publish(
            &manager,
            Default::default(),
            LocalStagedTreePublishRequest {
                parent: directory.path().into(),
                name: "write-recovery".into(),
                stage_options: LocalTempDirectoryOptions::new(),
                files: vec![
                    LocalStagedTreeFile {
                        path: PathBuf::from("duplicate"),
                        bytes: b"first".to_vec(),
                    },
                    LocalStagedTreeFile {
                        path: PathBuf::from("duplicate"),
                        bytes: b"second".to_vec(),
                    },
                ],
                publish_options: LocalTreePublishOptions::new().with_owner_access_policy(
                    LocalOwnerAccessPolicy::new().with_unix_modes(0o700, 0o600),
                ),
            },
            LocalOperationOptions::default(),
        )
        .unwrap();
    let write_recovery = match write_operation.wait_outcome().unwrap() {
        OperationOutcome::Failed(LocalStagedTreePublishError::Write { stage, .. }) => stage,
        outcome => panic!("duplicate write should preserve a stage, got {outcome:?}"),
    };

    let recovery_publish = |name: &str| LocalStagedTreePublishRequest {
        parent: directory.path().into(),
        name: name.into(),
        stage_options: LocalTempDirectoryOptions::new(),
        files: Vec::new(),
        publish_options: LocalTreePublishOptions::new()
            .with_owner_access_policy(LocalOwnerAccessPolicy::new()),
    };
    let (mut publish_operation, _) = filesystem
        .submit_staged_tree_publish(
            &manager,
            Default::default(),
            recovery_publish("publish-recovery"),
            LocalOperationOptions::default(),
        )
        .expect("the worker has dropped transient handles and the Write recovery keeps two roots");
    let publish_recovery = match publish_operation.wait_outcome().unwrap() {
        OperationOutcome::Failed(LocalStagedTreePublishError::Publish(error)) => {
            let (native, _disk) = error.into_parts();
            let (_cause, stage, retained) = native.into_parts();
            assert!(retained.is_empty());
            stage.expect("Publish failure keeps its stage")
        }
        outcome => panic!("empty invalid owner policy should retain a Publish stage, got {outcome:?}"),
    };

    let excessive = filesystem.submit_staged_tree_publish(
        &manager,
        Default::default(),
        recovery_publish("over-remaining"),
        LocalOperationOptions::default(),
    );
    let rejected = match excessive {
        Err(error) => error,
        Ok(_) => panic!("three handles must reject when only one remains"),
    };
    assert_eq!(rejected.rejection(), LocalSubmitRejection::CapacityExceeded);

    let (mut writer, _) = filesystem
        .submit_writer_commit(
            &manager,
            Default::default(),
            LocalWriterCommitRequest {
                path: directory.path().join("fits-remaining"),
                options: LocalWriteOptions::new(LocalWriteMode::CreateOrReplace),
                bytes: b"fits".to_vec(),
            },
            LocalOperationOptions::default(),
        )
        .expect("one handle fits the actual one-handle remainder");
    assert!(matches!(writer.wait_outcome().unwrap(), OperationOutcome::Succeeded(_)));

    drop(write_recovery);
    drop(publish_recovery);
    let (mut after_drop, _) = filesystem
        .submit_staged_tree_publish(
            &manager,
            Default::default(),
            LocalStagedTreePublishRequest {
                parent: directory.path().into(),
                name: "after-recovery-drop".into(),
                stage_options: LocalTempDirectoryOptions::new(),
                files: Vec::new(),
                publish_options: LocalTreePublishOptions::new(),
            },
            LocalOperationOptions::default(),
        )
        .expect("dropping both recovery owners refunds their retained-root quota");
    assert!(matches!(after_drop.wait_outcome().unwrap(), OperationOutcome::Succeeded(_)));
}

#[cfg(feature = "test-support")]
#[test]
fn test_concurrent_staged_submissions_compete_for_shared_handle_quota() {
    use qubit_local_files::operation::LocalSubmitRejection;
    use qubit_local_files::operation::publish::LocalStagedTreeFile;
    use qubit_local_files::operation::publish::LocalStagedTreePublishRequest;
    use qubit_local_files::options::LocalTempDirectoryOptions;
    use qubit_local_files::options::LocalTreePublishOptions;
    use qubit_local_files::test_support::install_test_gate;

    let _support_lock = PUBLISH_TEST_SUPPORT_LOCK
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let directory = tempfile::tempdir().unwrap();
    let manager = OperationManager::new(ManagerOptions::default()).unwrap();
    let executor = LocalIoExecutor::new(
        manager.clone(),
        LocalExecutionConfig {
            max_open_handles: 5,
            ..LocalExecutionConfig::default()
        },
    )
    .unwrap();
    let filesystem = block_on(AsyncLocalFileSystem::host(executor)).unwrap();
    let request = |name: &str| LocalStagedTreePublishRequest {
        parent: directory.path().into(),
        name: name.into(),
        stage_options: LocalTempDirectoryOptions::new(),
        files: vec![LocalStagedTreeFile {
            path: PathBuf::from("payload"),
            bytes: b"concurrent".to_vec(),
        }],
        publish_options: LocalTreePublishOptions::new(),
    };
    let gate = install_test_gate("lf10-staged-publish-after-admission").unwrap();
    let (mut first, _) = filesystem
        .submit_staged_tree_publish(
            &manager,
            Default::default(),
            request("first-concurrent"),
            LocalOperationOptions::default(),
        )
        .unwrap();
    gate.wait_until_entered();

    let rejected = match filesystem.submit_staged_tree_publish(
        &manager,
        Default::default(),
        request("second-concurrent"),
        LocalOperationOptions::default(),
    ) {
        Err(error) => error,
        Ok(_) => panic!("the in-flight staged operation already holds three shared handles"),
    };
    assert_eq!(rejected.rejection(), LocalSubmitRejection::CapacityExceeded);
    gate.release();
    assert!(matches!(first.wait_outcome().unwrap(), OperationOutcome::Succeeded(_)));
}

#[cfg(target_os = "linux")]
#[test]
fn test_cleanup_recovery_retains_shrunk_quota_across_parts_and_retry_failure() {
    use qubit_local_files::directory::LocalTreeRetainedResource;
    use qubit_local_files::operation::LocalSubmitRejection;
    use qubit_local_files::operation::publish::LocalStagedTreeFile;
    use qubit_local_files::operation::publish::LocalStagedTreePublishError;
    use qubit_local_files::operation::publish::LocalStagedTreePublishRequest;
    use qubit_local_files::options::LocalTempDirectoryOptions;
    use qubit_local_files::options::LocalTreePublishOptions;
    use qubit_local_files::policy::LocalOwnerAccessPolicy;
    use std::os::unix::fs::PermissionsExt;

    let directory = tempfile::tempdir().unwrap();
    let manager = OperationManager::new(ManagerOptions::default()).unwrap();
    let executor = LocalIoExecutor::new(
        manager.clone(),
        LocalExecutionConfig {
            max_open_handles: 5,
            ..LocalExecutionConfig::default()
        },
    )
    .unwrap();
    let filesystem = block_on(AsyncLocalFileSystem::host(executor)).unwrap();
    let (mut operation, _) = filesystem
        .submit_staged_tree_publish(
            &manager,
            Default::default(),
            LocalStagedTreePublishRequest {
                parent: directory.path().into(),
                name: "cleanup-recovery".into(),
                stage_options: LocalTempDirectoryOptions::new(),
                files: vec![
                    LocalStagedTreeFile {
                        path: PathBuf::from("duplicate"),
                        bytes: b"first".to_vec(),
                    },
                    LocalStagedTreeFile {
                        path: PathBuf::from("duplicate"),
                        bytes: b"second".to_vec(),
                    },
                ],
                publish_options: LocalTreePublishOptions::new().with_owner_access_policy(
                    LocalOwnerAccessPolicy::new().with_unix_modes(0o700, 0o600),
                ),
            },
            LocalOperationOptions::default(),
        )
        .unwrap();
    let write_recovery = match operation.wait_outcome().unwrap() {
        OperationOutcome::Failed(LocalStagedTreePublishError::Write { stage, .. }) => stage,
        outcome => panic!("duplicate write should preserve a stage, got {outcome:?}"),
    };
    let stage_path = directory.path().join(write_recovery.relative_name());
    std::fs::set_permissions(&stage_path, std::fs::Permissions::from_mode(0o500)).unwrap();
    let _cleanup_fault = qubit_local_files::test_support::install_test_fault_shared("lf10-staged-cleanup-failure")
        .unwrap();
    let cleanup_error = write_recovery
        .cleanup()
        .expect_err("injected Cleanup failure should retain its cleanup owner");
    let (native_cleanup, _disk) = cleanup_error.into_parts();
    let (_cause, mut cleanup_owners) = native_cleanup.into_parts();
    assert_eq!(cleanup_owners.len(), 1);
    let recovery: LocalTreeRetainedResource = cleanup_owners.pop().unwrap();

    let publish_request = |name: &str| LocalStagedTreePublishRequest {
        parent: directory.path().into(),
        name: name.into(),
        stage_options: LocalTempDirectoryOptions::new(),
        files: Vec::new(),
        publish_options: LocalTreePublishOptions::new()
            .with_owner_access_policy(LocalOwnerAccessPolicy::new()),
    };
    let (mut publish_operation, _) = filesystem
        .submit_staged_tree_publish(
            &manager,
            Default::default(),
            publish_request("publish-recovery-after-cleanup"),
            LocalOperationOptions::default(),
        )
        .expect("Cleanup recovery retains two roots, leaving three handles available");
    let publish_recovery = match publish_operation.wait_outcome().unwrap() {
        OperationOutcome::Failed(LocalStagedTreePublishError::Publish(error)) => {
            let (native, _disk) = error.into_parts();
            let (_cause, stage, _) = native.into_parts();
            stage.expect("invalid owner policy keeps its stage")
        }
        outcome => panic!("expected Publish recovery, got {outcome:?}"),
    };
    let rejected = match filesystem.submit_staged_tree_publish(
        &manager,
        Default::default(),
        publish_request("publish-over-quota"),
        LocalOperationOptions::default(),
    ) {
        Err(error) => error,
        Ok(_) => panic!("three handles must reject with only one remaining"),
    };
    assert_eq!(rejected.rejection(), LocalSubmitRejection::CapacityExceeded);

    let retry_error = recovery
        .cleanup()
        .expect_err("the configured cleanup retry should still fail");
    let (_cause, retry_owners) = retry_error.into_parts();
    let recovery = retry_owners.into_iter().next().expect("failed retry keeps owner");
    let retry_rejected = filesystem
        .submit_staged_tree_publish(
            &manager,
            Default::default(),
            publish_request("publish-over-quota-after-retry"),
            LocalOperationOptions::default(),
        )
        .err()
        .expect("retry failure keeps both recovery roots charged");
    assert_eq!(retry_rejected.rejection(), LocalSubmitRejection::CapacityExceeded);

    drop(publish_recovery);
    std::fs::set_permissions(&stage_path, std::fs::Permissions::from_mode(0o700)).unwrap();
    recovery.cleanup().unwrap();
    let (mut writer, _) = filesystem
        .submit_writer_commit(
            &manager,
            Default::default(),
            LocalWriterCommitRequest {
                path: directory.path().join("after-retry-cleanup"),
                options: LocalWriteOptions::new(LocalWriteMode::CreateOrReplace),
                bytes: b"refunded".to_vec(),
            },
            LocalOperationOptions::default(),
        )
        .expect("successful final cleanup refunds the staged handle quota");
    assert!(matches!(writer.wait_outcome().unwrap(), OperationOutcome::Succeeded(_)));

    let (mut drop_operation, _) = filesystem
        .submit_staged_tree_publish(
            &manager,
            Default::default(),
            LocalStagedTreePublishRequest {
                parent: directory.path().into(),
                name: "drop-recovery".into(),
                stage_options: LocalTempDirectoryOptions::new(),
                files: vec![
                    LocalStagedTreeFile { path: PathBuf::from("duplicate"), bytes: b"one".to_vec() },
                    LocalStagedTreeFile { path: PathBuf::from("duplicate"), bytes: b"two".to_vec() },
                ],
                publish_options: LocalTreePublishOptions::new(),
            },
            LocalOperationOptions::default(),
        )
        .unwrap();
    let drop_recovery = match drop_operation.wait_outcome().unwrap() {
        OperationOutcome::Failed(LocalStagedTreePublishError::Write { stage, .. }) => stage,
        outcome => panic!("duplicate write should preserve a stage for Drop, got {outcome:?}"),
    };
    let (mut other_operation, _) = filesystem
        .submit_staged_tree_publish(
            &manager,
            Default::default(),
            LocalStagedTreePublishRequest {
                parent: directory.path().into(),
                name: "other-drop-recovery".into(),
                stage_options: LocalTempDirectoryOptions::new(),
                files: vec![
                    LocalStagedTreeFile { path: PathBuf::from("duplicate"), bytes: b"one".to_vec() },
                    LocalStagedTreeFile { path: PathBuf::from("duplicate"), bytes: b"two".to_vec() },
                ],
                publish_options: LocalTreePublishOptions::new(),
            },
            LocalOperationOptions::default(),
        )
        .unwrap();
    let other_recovery = match other_operation.wait_outcome().unwrap() {
        OperationOutcome::Failed(LocalStagedTreePublishError::Write { stage, .. }) => stage,
        outcome => panic!("second duplicate write should preserve a stage, got {outcome:?}"),
    };
    let oversized = filesystem
        .submit_staged_tree_publish(
            &manager,
            Default::default(),
            publish_request("while-drop-owner-retained"),
            LocalOperationOptions::default(),
        )
        .err()
        .expect("two retained Write owners consume four handles, so a three-handle request must reject");
    assert_eq!(oversized.rejection(), LocalSubmitRejection::CapacityExceeded);
    drop(drop_recovery);
    let (mut after_drop, _) = filesystem
        .submit_staged_tree_publish(
            &manager,
            Default::default(),
            publish_request("after-drop-owner"),
            LocalOperationOptions::default(),
        )
        .expect("dropping the unretried Write recovery refunds its retained handle quota");
    let after_drop_recovery = match after_drop.wait_outcome().unwrap() {
        OperationOutcome::Failed(LocalStagedTreePublishError::Publish(error)) => {
            let (native, _) = error.into_parts();
            let (_, stage, _) = native.into_parts();
            stage.unwrap()
        }
        outcome => panic!("invalid owner policy should produce a Publish recovery, got {outcome:?}"),
    };
    drop(other_recovery);
    drop(after_drop_recovery);
}
