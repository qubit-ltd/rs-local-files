// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

#![cfg(all(feature = "async", feature = "test-support"))]

use std::future::Future;
use std::io::Read;
use std::sync::Arc;
use std::task::Context;
use std::task::Poll;
use std::task::Wake;
use std::task::Waker;

use qubit_local_files::AsyncLocalFileSystem;
use qubit_local_files::LocalExecutionConfig;
use qubit_local_files::LocalFileSystem;
use qubit_local_files::LocalIoExecutor;
use qubit_local_files::LocalOperationOptions;
use qubit_local_files::LocalReadRequest;
use qubit_local_files::LocalStageFailure;
use qubit_local_files::LocalStageOptions;
use qubit_local_files::options::LocalReadAllOptions;
use qubit_local_files::test_support::install_test_fault_shared;
use qubit_local_files::test_support::install_test_gate_for_local_filesystem;
use qubit_operation::ManagerOptions;
use qubit_operation::OperationManager;
use qubit_operation::OperationOptions;
use qubit_operation::OperationOutcome;
use qubit_operation::RecoveryPhase;
use qubit_operation::RecoveryStore;
use qubit_operation::RecoveryStoreOptions;
use qubit_operation::StopReason;

struct ThreadWake(std::thread::Thread);

impl Wake for ThreadWake {
    fn wake(self: Arc<Self>) {
        self.0.unpark();
    }

    fn wake_by_ref(self: &Arc<Self>) {
        self.0.unpark();
    }
}

fn block_on<F: Future>(future: F) -> F::Output {
    let waker = Waker::from(Arc::new(ThreadWake(std::thread::current())));
    let mut context = Context::from_waker(&waker);
    let mut future = Box::pin(future);
    loop {
        match future.as_mut().poll(&mut context) {
            Poll::Ready(value) => return value,
            Poll::Pending => std::thread::park(),
        }
    }
}

#[test]
fn test_read_stage_cancel_late_consumer_cleanup_and_recovery_retry() {
    let directory = tempfile::tempdir().expect("fixture directory");
    let input_path = directory.path().join("input.bin");
    std::fs::write(&input_path, b"read, stage, and recover").expect("input bytes");

    let manager = OperationManager::new(ManagerOptions::default()).expect("manager");
    let executor = LocalIoExecutor::new(manager.clone(), LocalExecutionConfig::default()).expect("local executor");
    let filesystem = LocalFileSystem::host().expect("filesystem");
    let async_filesystem = block_on(AsyncLocalFileSystem::host(executor.clone())).expect("async filesystem");

    let (read, _facts) = async_filesystem
        .submit_read_all(
            &manager,
            OperationOptions::default(),
            LocalReadRequest {
                path: input_path,
                read: LocalReadAllOptions::default(),
            },
            LocalOperationOptions::default(),
        )
        .expect("read admitted");
    let mut read = read.into_async();
    let bytes = match block_on(read.wait_outcome()).expect("read outcome") {
        OperationOutcome::Succeeded(bytes) => bytes,
        _ => panic!("input read did not succeed"),
    };
    assert_eq!(bytes, b"read, stage, and recover");
    assert_eq!(block_on(read.wait_settlement()).recovery_count, 0);

    let mut stage = qubit_local_files::stage_bytes(
        filesystem.clone(),
        executor.clone(),
        &manager.as_async(),
        OperationOptions::default(),
        bytes,
        LocalStageOptions::default(),
    )
    .expect("stage admitted");
    let lease = match block_on(stage.wait_outcome()).expect("stage outcome") {
        OperationOutcome::Succeeded(lease) => lease,
        _ => panic!("stage did not seal successfully"),
    };
    let helper_lease = lease.clone();
    let export = block_on(helper_lease.export_read_handle(&executor)).expect("read export");
    let mut helper_file = export.file.try_clone().expect("helper file clone");
    let helper_file_lease = export.lease.clone();
    let mut staged_bytes = Vec::new();
    helper_file
        .read_to_end(&mut staged_bytes)
        .expect("consumer reads staged bytes");
    assert_eq!(staged_bytes, b"read, stage, and recover");

    // Cancel another accepted staging operation while the consumer helper
    // still owns the first staged resource.
    let gate = install_test_gate_for_local_filesystem("lf6-stage-bytes-after-write", &filesystem)
        .expect("stage write gate");
    let mut cancelled_stage = qubit_local_files::stage_bytes(
        filesystem.clone(),
        executor.clone(),
        &manager.as_async(),
        OperationOptions::default(),
        b"cancelled stage".to_vec(),
        LocalStageOptions::default(),
    )
    .expect("cancellable stage admitted");
    gate.wait_until_entered();
    let _ = cancelled_stage.request_stop(StopReason::UserCancelled);
    gate.release();
    assert!(matches!(
        block_on(cancelled_stage.wait_outcome()).expect("cancelled stage outcome"),
        OperationOutcome::Stopped(_) | OperationOutcome::Failed(LocalStageFailure::Stopped)
    ));
    assert_eq!(block_on(cancelled_stage.wait_settlement()).recovery_count, 0);

    // Settlement remains pending until all file handles and leases have been
    // returned. The next cleanup attempt fails once, then the RecoveryStore
    // retries the same registered owner.
    let cleanup_fault = install_test_fault_shared("temp-file-sandbox-remove").expect("cleanup fault");
    drop(helper_file);
    drop(helper_file_lease);
    drop(export);
    drop(helper_lease);
    let mut settlement = Box::pin(stage.wait_settlement());
    let mut context = Context::from_waker(Waker::noop());
    assert!(matches!(settlement.as_mut().poll(&mut context), Poll::Pending));
    drop(lease);
    let report = block_on(settlement);
    let child = report.children.first().expect("staged temp child settlement");
    assert_eq!(child.settlement.recovery_count, 1);
    assert_eq!(child.settlement.recoveries.len(), 1);

    let recovery = child.settlement.recoveries[0].clone();
    let recovery_id = recovery.id();
    let store = RecoveryStore::new(RecoveryStoreOptions::default()).expect("recovery store");
    let transfer = recovery.transfer_to(&store).expect("transfer recovery owner");
    assert_eq!(transfer.recovery, recovery_id);
    assert_eq!(
        store.snapshot().groups,
        1,
        "the store retains the unresolved resource owner"
    );
    let retry = store.try_retry(recovery_id).expect("retry accepted");
    let retry_report = retry.wait().expect("retry report");
    assert_eq!(retry_report.recovery.phase, RecoveryPhase::Resolved);
    assert!(retry_report.failures.is_empty());
    drop(cleanup_fault);
    store.request_shutdown().wait().expect("recovery store shutdown");
}
