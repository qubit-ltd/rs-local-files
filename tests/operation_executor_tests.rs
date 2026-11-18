// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

#![cfg(feature = "async")]

use std::path::PathBuf;

use qubit_local_files::LocalExecutionConfig;
use qubit_local_files::LocalFileSystem;
use qubit_local_files::LocalReadRequest;
use qubit_local_files::operation::AsyncLocalFileSystem;
use qubit_local_files::operation::LocalIoExecutor;
use qubit_local_files::options::LocalListOptions;
use qubit_local_files::options::LocalReadAllOptions;
use qubit_local_files::options::LocalReadOptions;
use qubit_local_files::options::LocalWriteMode;
use qubit_local_files::options::LocalWriteOptions;
use qubit_operation::ManagerOptions;
use qubit_operation::OperationManager;

static PROCESS_CWD_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn block_on<F: std::future::Future>(future: F) -> F::Output {
    use std::sync::Arc;
    use std::sync::Condvar;
    use std::sync::Mutex;
    use std::task::Context;
    use std::task::Wake;
    use std::task::Waker;
    struct Signal {
        ready: Mutex<bool>,
        changed: Condvar,
    }
    impl Wake for Signal {
        fn wake(self: Arc<Self>) {
            *self.ready.lock().unwrap() = true;
            self.changed.notify_one();
        }
        fn wake_by_ref(self: &Arc<Self>) {
            *self.ready.lock().unwrap() = true;
            self.changed.notify_one();
        }
    }
    let signal = Arc::new(Signal {
        ready: Mutex::new(false),
        changed: Condvar::new(),
    });
    let waker = Waker::from(Arc::clone(&signal));
    let mut context = Context::from_waker(&waker);
    let mut future = std::pin::pin!(future);
    loop {
        if let std::task::Poll::Ready(value) = future.as_mut().poll(&mut context) {
            return value;
        }
        let mut ready = signal.ready.lock().unwrap();
        while !*ready {
            ready = signal.changed.wait(ready).unwrap();
        }
        *ready = false;
    }
}

fn host_async(manager: OperationManager) -> AsyncLocalFileSystem {
    let executor = LocalIoExecutor::new(manager, LocalExecutionConfig::default()).unwrap();
    block_on(AsyncLocalFileSystem::host(executor)).unwrap()
}

#[test]
fn test_host_facade_retains_a_fact_observer_for_submitted_reads() {
    let _cwd_guard = PROCESS_CWD_TEST_LOCK.lock().unwrap();
    let manager = OperationManager::new(ManagerOptions::default()).unwrap();
    let filesystem = host_async(manager.clone());

    let (operation, observer) = filesystem
        .submit_read_all_with_observer(
            &manager,
            Default::default(),
            LocalReadRequest {
                path: PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml"),
                read: Default::default(),
            },
            Default::default(),
        )
        .unwrap();
    let mut operation = operation;
    let outcome = operation.wait_outcome().unwrap();
    assert!(
        matches!(outcome, qubit_operation::OperationOutcome::Succeeded(bytes) if !bytes.is_empty())
    );
    operation.wait_settlement().unwrap();
    assert_eq!(observer.snapshot().modified_count, 0);
}

#[test]
fn test_invalid_executor_configuration_is_rejected_before_starting_workers() {
    let manager = OperationManager::new(ManagerOptions::default()).unwrap();
    let config = LocalExecutionConfig {
        worker_count: 0,
        ..LocalExecutionConfig::default()
    };

    assert!(LocalIoExecutor::new(manager, config).is_err());
}

#[test]
fn test_group_shutdown_does_not_close_application_manager() {
    use qubit_operation::GroupOptions;
    use qubit_operation::ShutdownMode;

    let manager = OperationManager::new(ManagerOptions::default()).unwrap();
    let group = manager.group(GroupOptions { capacity: 1 }).unwrap();
    let report = group.request_shutdown(ShutdownMode::Drain).wait().unwrap();
    assert_eq!(report.snapshot.active_operations, 0);

    let mut operation = manager
        .try_submit(
            Default::default(),
            |_| Ok::<_, std::convert::Infallible>(()),
        )
        .expect("closing a runtime group must leave its application manager open");
    assert!(matches!(
        operation.wait_outcome().unwrap(),
        qubit_operation::OperationOutcome::Succeeded(())
    ));
    operation.wait_settlement().unwrap();
    let report = manager
        .request_shutdown(ShutdownMode::Drain)
        .wait()
        .unwrap();
    assert_eq!(report.snapshot.active_operations, 0);
}

#[test]
fn test_wrong_manager_rejection_returns_the_unmodified_request() {
    let _cwd_guard = PROCESS_CWD_TEST_LOCK.lock().unwrap();
    let owner = OperationManager::new(ManagerOptions::default()).unwrap();
    let other = OperationManager::new(ManagerOptions::default()).unwrap();
    let filesystem = host_async(owner);
    let original_read = LocalReadAllOptions::new().with_max_bytes(11);
    let request = LocalReadRequest {
        path: PathBuf::from("./original-path"),
        read: original_read,
    };

    let rejected = match filesystem.submit_read_all_with_observer(
        &other,
        Default::default(),
        request,
        Default::default(),
    ) {
        Ok(_) => panic!("cross-manager submission unexpectedly succeeded"),
        Err(error) => error,
    };
    assert_eq!(
        rejected.rejection(),
        qubit_local_files::operation::LocalSubmitRejection::WrongManager
    );
    let returned = rejected.into_request();
    assert_eq!(returned.path, PathBuf::from("./original-path"));
    assert_eq!(returned.read, original_read);
}

#[test]
fn test_oversized_owned_read_limit_is_rejected_with_original_request() {
    let manager = OperationManager::new(ManagerOptions::default()).unwrap();
    let config = LocalExecutionConfig {
        max_owned_bytes: 8,
        ..LocalExecutionConfig::default()
    };
    let executor = LocalIoExecutor::new(manager.clone(), config).unwrap();
    let filesystem = block_on(AsyncLocalFileSystem::host(executor)).unwrap();
    let path = PathBuf::from("./owned-limit-original");
    let rejected = match filesystem.submit_read_all_with_observer(
        &manager,
        Default::default(),
        LocalReadRequest {
            path: path.clone(),
            read: Default::default(),
        },
        qubit_local_files::LocalOperationOptions {
            max_owned_bytes: Some(9),
            ..Default::default()
        },
    ) {
        Ok(_) => panic!("read request larger than total executor memory was admitted"),
        Err(error) => error,
    };
    assert_eq!(
        rejected.rejection(),
        qubit_local_files::operation::LocalSubmitRejection::CapacityExceeded
    );
    assert_eq!(rejected.into_request().path, path);
}

#[test]
fn test_read_submission_returns_request_when_fact_observer_capacity_is_exhausted() {
    let directory = tempfile::tempdir().unwrap();
    let first_path = directory.path().join("first");
    let second_path = directory.path().join("second");
    std::fs::write(&first_path, b"first").unwrap();
    std::fs::write(&second_path, b"second").unwrap();

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
    let (mut first, observer) = filesystem
        .submit_read_all_with_observer(
            &manager,
            Default::default(),
            LocalReadRequest {
                path: first_path,
                read: Default::default(),
            },
            Default::default(),
        )
        .unwrap();
    assert!(matches!(
        first.wait_outcome().unwrap(),
        qubit_operation::OperationOutcome::Succeeded(_)
    ));
    first.wait_settlement().unwrap();

    let read = LocalReadAllOptions::new().with_max_bytes(17);
    let request = LocalReadRequest {
        path: second_path.clone(),
        read,
    };
    let rejected =
        match filesystem.submit_read_all_with_observer(&manager, Default::default(), request, Default::default()) {
            Ok(_) => panic!("the retained observer should own the only fact slot"),
            Err(error) => error,
        };
    assert_eq!(
        rejected.rejection(),
        qubit_local_files::operation::LocalSubmitRejection::CapacityExceeded
    );
    let returned = rejected.into_request();
    assert_eq!(returned.path, second_path);
    assert_eq!(returned.read, read);

    drop(observer);
    let (mut retry, _) = filesystem
        .submit_read_all_with_observer(
            &manager,
            Default::default(),
            LocalReadRequest {
                path: returned.path,
                read: returned.read,
            },
            Default::default(),
        )
        .unwrap();
    assert!(matches!(
        retry.wait_outcome().unwrap(),
        qubit_operation::OperationOutcome::Succeeded(_)
    ));
}

#[test]
fn test_stopped_parent_rejects_child_read_before_worker_io() {
    let _cwd_guard = PROCESS_CWD_TEST_LOCK.lock().unwrap();
    use std::sync::mpsc;
    use std::time::Duration;

    let manager = OperationManager::new(ManagerOptions::default()).unwrap();
    let filesystem = host_async(manager.clone());
    let (entered_tx, entered_rx) = mpsc::channel();
    let (release_tx, release_rx) = mpsc::channel();
    let (result_tx, result_rx) = mpsc::channel();
    let parent = manager
        .try_submit(Default::default(), move |scope| {
            entered_tx.send(()).unwrap();
            release_rx.recv().unwrap();
            let child_manager = scope.manager();
            let result = filesystem.submit_read_all_with_observer(
                &child_manager,
                Default::default(),
                LocalReadRequest {
                    path: PathBuf::from("./must-not-be-read"),
                    read: Default::default(),
                },
                Default::default(),
            );
            let status = result.map(|_| ()).map_err(|error| error.rejection());
            result_tx.send(status).unwrap();
            Ok::<_, ()>(())
        })
        .unwrap();

    entered_rx.recv_timeout(Duration::from_secs(5)).unwrap();
    let _ = parent.request_stop(qubit_operation::StopReason::UserCancelled);
    release_tx.send(()).unwrap();
    assert_eq!(
        result_rx.recv_timeout(Duration::from_secs(5)).unwrap(),
        Err(qubit_local_files::operation::LocalSubmitRejection::Stopped)
    );
    let mut parent = parent;
    let _ = parent.wait_outcome().unwrap();
}

#[test]
fn test_parent_manager_view_can_submit_through_executor_runtime_group() {
    let _cwd_guard = PROCESS_CWD_TEST_LOCK.lock().unwrap();
    let manager = OperationManager::new(ManagerOptions::default()).unwrap();
    let filesystem = host_async(manager.clone());
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("Cargo.toml");
    let filesystem_for_parent = filesystem.clone();
    let mut parent = manager
        .try_submit(Default::default(), move |scope| {
            let child_manager = scope.manager();
            let (mut child, _observer) = filesystem_for_parent
                .submit_read_all_with_observer(
                    &child_manager,
                    Default::default(),
                    LocalReadRequest {
                        path,
                        read: Default::default(),
                    },
                    Default::default(),
                )
                .map_err(|_| ())?;
            match child.wait_outcome().map_err(|_| ())? {
                qubit_operation::OperationOutcome::Succeeded(bytes) => Ok::<_, ()>(bytes),
                _ => Err(()),
            }
        })
        .unwrap();

    assert!(matches!(
        parent.wait_outcome().unwrap(),
        qubit_operation::OperationOutcome::Succeeded(bytes) if !bytes.is_empty()
    ));
    parent.wait_settlement().unwrap();
}

#[test]
fn test_host_relative_reads_use_cwd_captured_by_async_conversion() {
    let _cwd_guard = PROCESS_CWD_TEST_LOCK.lock().unwrap();
    struct RestoreCwd(PathBuf);
    impl Drop for RestoreCwd {
        fn drop(&mut self) {
            let _ = std::env::set_current_dir(&self.0);
        }
    }

    let original_cwd = std::env::current_dir().unwrap();
    let _restore = RestoreCwd(original_cwd);
    let first = tempfile::tempdir().unwrap();
    let second = tempfile::tempdir().unwrap();
    std::fs::write(first.path().join("payload"), b"from-first").unwrap();
    std::fs::write(second.path().join("payload"), b"from-second").unwrap();

    std::env::set_current_dir(first.path()).unwrap();
    let manager = OperationManager::new(ManagerOptions::default()).unwrap();
    let filesystem = host_async(manager.clone());
    std::env::set_current_dir(second.path()).unwrap();

    let (mut operation, _observer) = filesystem
        .submit_read_all_with_observer(
            &manager,
            Default::default(),
            LocalReadRequest {
                path: PathBuf::from("payload"),
                read: Default::default(),
            },
            Default::default(),
        )
        .unwrap();
    assert!(
        matches!(operation.wait_outcome().unwrap(), qubit_operation::OperationOutcome::Succeeded(bytes) if bytes == b"from-first")
    );
    operation.wait_settlement().unwrap();
}

#[test]
fn test_rooted_facade_keeps_open_authority_after_root_path_replacement() {
    let scratch = tempfile::tempdir().unwrap();
    let root = scratch.path().join("root");
    std::fs::create_dir(&root).unwrap();
    std::fs::write(root.join("payload"), b"original-authority").unwrap();

    let manager = OperationManager::new(ManagerOptions::default()).unwrap();
    let executor = LocalIoExecutor::new(manager.clone(), LocalExecutionConfig::default()).unwrap();
    let filesystem = block_on(AsyncLocalFileSystem::rooted(executor, root.clone())).unwrap();
    std::fs::rename(&root, scratch.path().join("moved-root")).unwrap();
    std::fs::create_dir(&root).unwrap();
    std::fs::write(root.join("payload"), b"replacement-path").unwrap();

    let (mut operation, _observer) = filesystem
        .submit_read_all_with_observer(
            &manager,
            Default::default(),
            LocalReadRequest {
                path: PathBuf::from("payload"),
                read: Default::default(),
            },
            Default::default(),
        )
        .unwrap();
    assert!(
        matches!(operation.wait_outcome().unwrap(), qubit_operation::OperationOutcome::Succeeded(bytes) if bytes == b"original-authority")
    );
    operation.wait_settlement().unwrap();
}

#[test]
fn test_into_async_does_not_capture_host_cwd_and_rejects_relative_paths() {
    let manager = OperationManager::new(ManagerOptions::default()).unwrap();
    let executor = LocalIoExecutor::new(manager.clone(), LocalExecutionConfig::default()).unwrap();
    let filesystem = LocalFileSystem::host()
        .unwrap()
        .into_async(executor)
        .unwrap();
    let rejected = filesystem
        .submit_read_all_with_observer(
            &manager,
            Default::default(),
            LocalReadRequest {
                path: PathBuf::from("relative"),
                read: Default::default(),
            },
            Default::default(),
        )
        .err()
        .unwrap();
    assert_eq!(
        rejected.rejection(),
        qubit_local_files::operation::LocalSubmitRejection::AuthorityNotCaptured
    );
    assert_eq!(rejected.into_request().path, PathBuf::from("relative"));
}

#[test]
fn test_uncaptured_host_authority_rejects_relative_stream_submissions() {
    let manager = OperationManager::new(ManagerOptions::default()).unwrap();
    let executor = LocalIoExecutor::new(manager.clone(), LocalExecutionConfig::default()).unwrap();
    let filesystem = LocalFileSystem::host()
        .unwrap()
        .into_async(executor)
        .unwrap();

    let reader_path = PathBuf::from("reader-relative");
    let reader_error = match filesystem.submit_open_reader(
        &manager,
        Default::default(),
        reader_path.clone(),
        LocalReadOptions::default(),
    ) {
        Ok(_) => panic!("uncaptured relative reader submission unexpectedly succeeded"),
        Err(error) => error,
    };
    assert_eq!(
        reader_error.rejection(),
        qubit_local_files::operation::LocalSubmitRejection::AuthorityNotCaptured
    );
    assert_eq!(reader_error.into_request(), reader_path);

    let writer_path = PathBuf::from("writer-relative");
    let writer_error = match filesystem.submit_open_writer(
        &manager,
        Default::default(),
        writer_path.clone(),
        LocalWriteOptions::new(LocalWriteMode::CreateOrReplace),
    ) {
        Ok(_) => panic!("uncaptured relative writer submission unexpectedly succeeded"),
        Err(error) => error,
    };
    assert_eq!(
        writer_error.rejection(),
        qubit_local_files::operation::LocalSubmitRejection::AuthorityNotCaptured
    );
    assert_eq!(writer_error.into_request(), writer_path);

    let walker_path = PathBuf::from("walker-relative");
    let walker_error = match filesystem.submit_open_walker(
        &manager,
        Default::default(),
        walker_path.clone(),
        LocalListOptions::default(),
    ) {
        Ok(_) => panic!("uncaptured relative walker submission unexpectedly succeeded"),
        Err(error) => error,
    };
    assert_eq!(
        walker_error.rejection(),
        qubit_local_files::operation::LocalSubmitRejection::AuthorityNotCaptured
    );
    assert_eq!(walker_error.into_request(), walker_path);
}
