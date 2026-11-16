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
fn successful_commit_publishes_and_records_effect_independently_of_outcome() {
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
fn create_new_conflict_preserves_the_existing_destination_and_not_published_fact() {
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
fn writer_rejects_owned_allocation_over_budget_before_opening_destination() {
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
fn staged_tree_disk_quota_rejection_returns_the_original_request() {
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
fn staged_tree_uses_one_parent_authority_and_publishes_only_after_exact_validation() {
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
fn digest_conditional_conflict_keeps_existing_data_and_published_state_correct() {
    use qubit_local_files::operation::publish::LocalConditionalWriterCommitRequest;
    use qubit_local_files::options::LocalWriteMode;
    use qubit_local_files::options::LocalWritePrecondition;
    use sha2::Sha256;

    let directory = tempfile::tempdir().unwrap();
    let destination = directory.path().join("occupied");
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

#[cfg(all(feature = "digest", feature = "test-support"))]
#[test]
fn conditional_abort_failure_after_stop_retains_recovery_owner() {
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
    let destination = directory.path().join("conditional-abort-recovery");
    let manager = OperationManager::new(ManagerOptions::default()).unwrap();
    let filesystem = facade(&manager)
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
    drop(fault);
    let report = recovery.try_retry().unwrap().wait().unwrap();
    assert_eq!(report.recovery.phase, qubit_operation::RecoveryPhase::Resolved);
    assert!(!destination.exists());
}

#[test]
fn expired_deadline_rejects_owned_request_before_native_work() {
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
    let mut options = qubit_operation::OperationOptions::default();
    options.context = ContextOptions {
        clock: Some(Arc::new(FakeClock { domain, ticks: 10 })),
        deadline: Some(Deadline::at(MonotonicTime::new(domain, 1))),
        budget: None,
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
fn partial_stage_write_failure_keeps_the_unpublished_stage_for_cleanup() {
    use qubit_local_files::operation::publish::LocalStagedTreeFile;
    use qubit_local_files::operation::publish::LocalStagedTreePublishError;
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
        OperationOutcome::Failed(LocalStagedTreePublishError::Write { stage, .. }) => {
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

#[cfg(unix)]
#[test]
fn staged_tree_forces_private_file_creation_even_when_defaults_were_requested() {
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
fn exhausted_fact_record_slots_reject_new_mutation_before_native_work() {
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
    assert!(matches!(first.wait_outcome().unwrap(), OperationOutcome::Succeeded(_)));
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
fn failed_publish_effect_registration_keeps_observer_facts() {
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
fn temp_persist_conflict_retains_same_resource_for_a_later_keep() {
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
fn temp_seal_returns_a_read_lease_for_the_same_registered_resource() {
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
}

#[test]
fn sealed_temp_rejects_repeat_lifecycle_commands_without_consuming_the_owner() {
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
}

#[test]
fn temp_directory_keep_and_persist_are_supported() {
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

#[cfg(feature = "test-support")]
#[test]
fn queued_temp_persist_observes_stop_before_publishing() {
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
fn cancellation_at_last_precommit_gate_aborts_without_publishing() {
    use qubit_local_files::test_support::install_test_gate;
    use qubit_operation::StopReason;

    let _gate_lock = WRITER_PUBLISH_GATE_TEST_LOCK
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let directory = tempfile::tempdir().unwrap();
    let destination = directory.path().join("cancelled-at-gate");
    let manager = OperationManager::new(ManagerOptions::default()).unwrap();
    let filesystem = facade(&manager);
    let gate = install_test_gate("lf7-writer-before-publish").unwrap();
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
    gate.wait_until_entered();
    operation.request_stop(StopReason::UserCancelled);
    gate.release();
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
fn failed_abort_after_stop_returns_retryable_writer_and_cleanup_error() {
    use qubit_local_files::test_support::install_test_fault_shared;
    use qubit_local_files::test_support::install_test_gate;
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
    let filesystem = facade(&manager);
    let gate = install_test_gate("lf7-writer-before-publish").unwrap();
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
    drop(fault);
    let report = recovery.try_retry().unwrap().wait().unwrap();
    assert_eq!(report.recovery.phase, qubit_operation::RecoveryPhase::Resolved);
    assert!(!destination.exists());
}

#[cfg(feature = "test-support")]
#[test]
fn append_effect_is_published_before_worker_panic_and_cleanup_releases_writer() {
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
fn append_sync_failure_preserves_published_bytes_and_failed_durability() {
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

#[cfg(feature = "test-support")]
#[test]
fn stop_during_native_publish_preserves_published_fact_when_parent_sync_fails() {
    use qubit_local_files::operation::publish::LocalWriterCommandError;
    use qubit_local_files::outcome::LocalWriteFailureState;
    use qubit_local_files::policy::LocalDurabilityRequirement;
    use qubit_local_files::test_support::install_test_fault_shared;
    use qubit_local_files::test_support::install_test_gate;
    use qubit_operation::StopReason;

    let _support_lock = PUBLISH_TEST_SUPPORT_LOCK
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let directory = tempfile::tempdir().unwrap();
    let destination = directory.path().join("nested/created/payload");
    let manager = OperationManager::new(ManagerOptions::default()).unwrap();
    let filesystem = facade(&manager);
    let gate = install_test_gate("atomic-writer-after-install-before-parent-sync").unwrap();
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
        ) || matches!(outcome, OperationOutcome::Stopped(_))
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
fn partial_tree_cleanup_failure_keeps_a_retryable_recovery_owner() {
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
