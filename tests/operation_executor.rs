use std::path::PathBuf;

use qubit_local_files::LocalExecutionConfig;
use qubit_local_files::LocalFileSystem;
use qubit_local_files::LocalReadRequest;
use qubit_local_files::operation::LocalIoExecutor;
use qubit_local_files::options::LocalReadAllOptions;
use qubit_operation::ManagerOptions;
use qubit_operation::OperationManager;

#[test]
fn host_facade_retains_a_fact_observer_for_submitted_reads() {
    let manager = OperationManager::new(ManagerOptions::default()).unwrap();
    let executor = LocalIoExecutor::new(manager.clone(), LocalExecutionConfig::default()).unwrap();
    let filesystem = LocalFileSystem::host()
        .unwrap()
        .into_async(executor)
        .unwrap();

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
fn invalid_executor_configuration_is_rejected_before_starting_workers() {
    let manager = OperationManager::new(ManagerOptions::default()).unwrap();
    let config = LocalExecutionConfig {
        worker_count: 0,
        ..LocalExecutionConfig::default()
    };

    assert!(LocalIoExecutor::new(manager, config).is_err());
}

#[test]
fn wrong_manager_rejection_returns_the_unmodified_request() {
    let owner = OperationManager::new(ManagerOptions::default()).unwrap();
    let other = OperationManager::new(ManagerOptions::default()).unwrap();
    let executor = LocalIoExecutor::new(owner, LocalExecutionConfig::default()).unwrap();
    let filesystem = LocalFileSystem::host()
        .unwrap()
        .into_async(executor)
        .unwrap();
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
fn oversized_owned_read_limit_is_rejected_with_original_request() {
    let manager = OperationManager::new(ManagerOptions::default()).unwrap();
    let config = LocalExecutionConfig {
        max_owned_bytes: 8,
        ..LocalExecutionConfig::default()
    };
    let executor = LocalIoExecutor::new(manager.clone(), config).unwrap();
    let filesystem = LocalFileSystem::host()
        .unwrap()
        .into_async(executor)
        .unwrap();
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
fn stopped_parent_rejects_child_read_before_worker_io() {
    use std::sync::mpsc;
    use std::time::Duration;

    let manager = OperationManager::new(ManagerOptions::default()).unwrap();
    let executor = LocalIoExecutor::new(manager.clone(), LocalExecutionConfig::default()).unwrap();
    let filesystem = LocalFileSystem::host()
        .unwrap()
        .into_async(executor)
        .unwrap();
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
fn host_relative_reads_use_cwd_captured_by_async_conversion() {
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
    let executor = LocalIoExecutor::new(manager.clone(), LocalExecutionConfig::default()).unwrap();
    let filesystem = LocalFileSystem::host()
        .unwrap()
        .into_async(executor)
        .unwrap();
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
fn rooted_facade_keeps_open_authority_after_root_path_replacement() {
    let scratch = tempfile::tempdir().unwrap();
    let root = scratch.path().join("root");
    std::fs::create_dir(&root).unwrap();
    std::fs::write(root.join("payload"), b"original-authority").unwrap();

    let manager = OperationManager::new(ManagerOptions::default()).unwrap();
    let executor = LocalIoExecutor::new(manager.clone(), LocalExecutionConfig::default()).unwrap();
    let filesystem = LocalFileSystem::rooted(&root)
        .unwrap()
        .into_async(executor)
        .unwrap();
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
