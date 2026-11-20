// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

#![cfg(feature = "async")]

#[cfg(feature = "test-support")]
#[test]
fn test_recursive_worker_walker_returns_native_entry_error() {
    use qubit_local_files::LocalFileSystem;
    use qubit_local_files::operation::LocalExecutionConfig;
    use qubit_local_files::operation::LocalIoExecutor;
    use qubit_local_files::options::LocalListOptions;
    use qubit_local_files::test_support::install_test_fault_shared;
    use qubit_operation::ManagerOptions;
    use qubit_operation::OperationManager;
    use qubit_operation::OperationOutcome;

    let directory = tempfile::tempdir().unwrap();
    std::fs::create_dir(directory.path().join("nested")).unwrap();
    std::fs::write(directory.path().join("nested/entry"), b"").unwrap();
    let manager = OperationManager::new(ManagerOptions::default()).unwrap();
    let executor = LocalIoExecutor::new(manager.clone(), LocalExecutionConfig::default()).unwrap();
    let filesystem = LocalFileSystem::host().unwrap().into_async(executor).unwrap();
    let mut opening = filesystem
        .submit_open_walker(
            &manager,
            Default::default(),
            directory.path().to_path_buf(),
            LocalListOptions::default().with_recursive(),
        )
        .unwrap();
    let mut walker = match opening.wait_outcome().unwrap() {
        OperationOutcome::Succeeded(walker) => walker,
        _ => panic!("walker open failed"),
    };

    // The shared fault selector is process-wide. Keeping this test in its own
    // integration-test binary prevents another concurrent walker test from
    // consuming its one-shot fault.
    let _fault = install_test_fault_shared("walker-entry").unwrap();
    let error = poll_worker(|cx| Box::pin(walker.next_batch()).as_mut().poll(cx)).unwrap_err();
    assert_eq!(error.kind(), qubit_local_files::error::LocalFileErrorKind::Io);
    assert!(error.to_string().contains("injected walker directory entry failure"));
    drop(walker);
    opening.wait_settlement().unwrap();
}

#[cfg(feature = "test-support")]
#[test]
fn test_async_walker_preserves_native_resource_limit() {
    use qubit_local_files::LocalFileSystem;
    use qubit_local_files::error::LocalFileErrorKind;
    use qubit_local_files::error::LocalResourceKind;
    use qubit_local_files::operation::LocalExecutionConfig;
    use qubit_local_files::operation::LocalIoExecutor;
    use qubit_local_files::options::LocalListOptions;
    use qubit_operation::ManagerOptions;
    use qubit_operation::OperationManager;
    use qubit_operation::OperationOutcome;

    let directory = tempfile::tempdir().unwrap();
    std::fs::write(directory.path().join("entry-a"), b"").unwrap();
    std::fs::write(directory.path().join("entry-b"), b"").unwrap();
    let manager = OperationManager::new(ManagerOptions::default()).unwrap();
    let executor = LocalIoExecutor::new(manager.clone(), LocalExecutionConfig::default()).unwrap();
    let filesystem = LocalFileSystem::host().unwrap().into_async(executor).unwrap();
    let mut opening = filesystem
        .submit_open_walker(
            &manager,
            Default::default(),
            directory.path().to_path_buf(),
            LocalListOptions::default().with_max_entries(1),
        )
        .unwrap();
    let mut walker = match opening.wait_outcome().unwrap() {
        OperationOutcome::Succeeded(walker) => walker,
        _ => panic!("walker open failed"),
    };

    let first = poll_worker(|cx| Box::pin(walker.next_batch()).as_mut().poll(cx))
        .unwrap()
        .expect("the first entry batch should be delivered");
    assert_eq!(first.len(), 1);

    let error = poll_worker(|cx| Box::pin(walker.next_batch()).as_mut().poll(cx)).unwrap_err();
    assert_eq!(error.kind(), LocalFileErrorKind::ResourceLimit);
    assert_eq!(error.path(), Some(directory.path()));
    let limit = error.resource_limit_error().expect("native budget facts should be retained");
    assert_eq!(limit.resource(), LocalResourceKind::Entry);
    assert_eq!(limit.limit(), 1);
    assert_eq!(limit.remaining(), 0);
    assert_eq!(limit.requested(), 1);
    drop(walker);
    opening.wait_settlement().unwrap();
}

#[cfg(feature = "test-support")]
fn poll_worker<F, T>(mut poll: F) -> T
where
    F: FnMut(&mut std::task::Context<'_>) -> std::task::Poll<T>,
{
    struct Notify(std::sync::mpsc::Sender<()>);
    impl std::task::Wake for Notify {
        fn wake(self: std::sync::Arc<Self>) {
            let _ = self.0.send(());
        }

        fn wake_by_ref(self: &std::sync::Arc<Self>) {
            let _ = self.0.send(());
        }
    }

    let (wake_tx, wake_rx) = std::sync::mpsc::channel();
    let waker = std::task::Waker::from(std::sync::Arc::new(Notify(wake_tx)));
    let mut context = std::task::Context::from_waker(&waker);
    loop {
        match poll(&mut context) {
            std::task::Poll::Ready(value) => return value,
            std::task::Poll::Pending => wake_rx
                .recv_timeout(std::time::Duration::from_secs(5))
                .expect("worker operation should make progress"),
        }
    }
}
