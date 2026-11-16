#![cfg(feature = "async")]

use std::future::Future;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
use std::task::Context;
use std::task::Poll;
use std::task::Wake;
use std::task::Waker;

use qubit_local_files::LocalExecutionConfig;
use qubit_local_files::LocalFileSystem;
use qubit_local_files::operation::LocalCreateFailure;
use qubit_local_files::operation::LocalIoExecutor;
use qubit_local_files::operation::submit_temp_directory;
use qubit_local_files::operation::submit_temp_file;
use qubit_local_files::options::LocalTempDirectoryOptions;
use qubit_local_files::options::LocalTempFileOptions;
use qubit_operation::AsyncCleanup;
use qubit_operation::CleanupContext;
use qubit_operation::CleanupFailure;
use qubit_operation::CleanupFuture;
use qubit_operation::CleanupOutcome;
use qubit_operation::CleanupStage;
use qubit_operation::DiagnosticSummary;
use qubit_operation::ManagerOptions;
use qubit_operation::OperationManager;
use qubit_operation::OperationOptions;
use qubit_operation::OperationOutcome;
use qubit_operation::RecoveryPhase;
use qubit_operation::RecoveryStore;
use qubit_operation::RecoveryStoreOptions;

fn assert_send_sync<T: Send + Sync + 'static>() {}

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

fn find_file(root: &std::path::Path) -> std::path::PathBuf {
    for entry in std::fs::read_dir(root).expect("read root") {
        let path = entry.expect("entry").path();
        if path.is_file() {
            return path;
        }
        if path.is_dir() {
            if let Ok(found) = std::fs::read_dir(&path) {
                for child in found {
                    let child = child.expect("child").path();
                    if child.is_file() {
                        return child;
                    }
                }
            }
        }
    }
    panic!("temp payload file exists");
}

#[test]
fn local_temp_resource_is_a_send_sync_control_port() {
    assert_send_sync::<qubit_local_files::operation::LocalTempResource>();
    assert_send_sync::<qubit_operation::ResourceRef<qubit_local_files::operation::LocalTempResource>>();
}

#[test]
fn created_temp_file_is_cleaned_after_scope_ends_and_access_returns() {
    let parent = tempfile::tempdir().expect("temporary parent");
    let manager = OperationManager::new(ManagerOptions::default()).expect("manager");
    let executor = LocalIoExecutor::new(
        manager.clone(),
        LocalExecutionConfig {
            max_disk_bytes: 1,
            ..LocalExecutionConfig::default()
        },
    )
    .expect("executor");
    let filesystem = LocalFileSystem::host().expect("host filesystem");
    let mut operation = submit_temp_file(
        filesystem.clone(),
        executor.clone(),
        &manager.as_async(),
        OperationOptions::default(),
        LocalTempFileOptions::new().with_parent(parent.path()),
    )
    .expect("operation admitted");

    let outcome = block_on(operation.wait_outcome()).expect("outcome available");
    let OperationOutcome::Succeeded(access) = outcome else {
        panic!("temp creation should succeed");
    };
    assert_eq!(std::fs::read_dir(parent.path()).expect("read parent").count(), 1);
    drop(access);
    let settlement = block_on(operation.wait_settlement());
    assert_eq!(settlement.recovery_count, 0);
    assert_eq!(std::fs::read_dir(parent.path()).expect("read parent").count(), 0);
    let mut next = submit_temp_file(
        filesystem,
        executor,
        &manager.as_async(),
        OperationOptions::default(),
        LocalTempFileOptions::new().with_parent(parent.path()),
    )
    .expect("one-byte disk reservation returned after confirmed cleanup");
    let OperationOutcome::Succeeded(access) = block_on(next.wait_outcome()).expect("outcome") else {
        panic!("disk permit should be reusable");
    };
    drop(access);
    block_on(next.wait_settlement());
}

#[test]
fn created_temp_directory_is_cleaned_after_scope_ends_and_access_returns() {
    let parent = tempfile::tempdir().expect("temporary parent");
    let manager = OperationManager::new(ManagerOptions::default()).expect("manager");
    let executor = LocalIoExecutor::new(manager.clone(), LocalExecutionConfig::default()).expect("executor");
    let filesystem = LocalFileSystem::host().expect("host filesystem");
    let mut operation = submit_temp_directory(
        filesystem,
        executor,
        &manager.as_async(),
        OperationOptions::default(),
        LocalTempDirectoryOptions::new().with_parent(parent.path()),
    )
    .expect("operation admitted");
    let outcome = block_on(operation.wait_outcome()).expect("outcome available");
    let OperationOutcome::Succeeded(access) = outcome else {
        panic!("temp creation should succeed");
    };
    assert_eq!(std::fs::read_dir(parent.path()).expect("read parent").count(), 1);
    drop(access);
    let settlement = block_on(operation.wait_settlement());
    assert_eq!(settlement.recovery_count, 0);
    assert_eq!(std::fs::read_dir(parent.path()).expect("read parent").count(), 0);
}

#[test]
fn failed_cleanup_keeps_resource_permit_and_public_recovery_can_retry() {
    let parent = tempfile::tempdir().expect("temporary parent");
    let manager = OperationManager::new(ManagerOptions::default()).expect("manager");
    let executor = LocalIoExecutor::new(
        manager.clone(),
        LocalExecutionConfig {
            max_disk_bytes: 1,
            ..LocalExecutionConfig::default()
        },
    )
    .expect("executor");
    let filesystem = LocalFileSystem::host().expect("host filesystem");
    let mut operation = submit_temp_file(
        filesystem.clone(),
        executor.clone(),
        &manager.as_async(),
        OperationOptions::default(),
        LocalTempFileOptions::new().with_parent(parent.path()),
    )
    .expect("operation admitted");
    let outcome = block_on(operation.wait_outcome()).expect("outcome available");
    let OperationOutcome::Succeeded(access) = outcome else {
        panic!("temp creation should succeed");
    };
    let original = find_file(parent.path());
    let displaced = original.with_extension("retained");
    std::fs::rename(&original, &displaced).expect("displace generated entry");
    std::fs::create_dir(&original).expect("replace generated entry with a directory");
    drop(access);

    let settlement = block_on(operation.wait_settlement());
    assert_eq!(settlement.recoveries.len(), 1);
    let recovery = settlement.recoveries[0].clone();
    assert_eq!(recovery.snapshot().phase, RecoveryPhase::Pending);
    assert_eq!(recovery.snapshot().resources.len(), 1);

    let mut capacity_probe = submit_temp_file(
        filesystem.clone(),
        executor.clone(),
        &manager.as_async(),
        OperationOptions::default(),
        LocalTempFileOptions::new().with_parent(parent.path()),
    )
    .expect("operation itself is admitted");
    assert!(matches!(
        block_on(capacity_probe.wait_outcome()).expect("probe outcome"),
        OperationOutcome::Failed(qubit_local_files::operation::LocalCreateFailure::Capacity)
    ));
    assert_eq!(recovery.snapshot().resources.len(), 1);

    let failed_retry = recovery
        .try_retry()
        .expect("retry accepted")
        .wait()
        .expect("retry report");
    assert!(!failed_retry.failures.is_empty());
    assert_eq!(recovery.snapshot().attempts, 1);

    let second_retry = recovery
        .try_retry()
        .expect("second retry accepted")
        .wait()
        .expect("second retry report");
    assert!(!second_retry.failures.is_empty());
    assert_eq!(recovery.snapshot().attempts, 2);
    assert_eq!(recovery.snapshot().phase, RecoveryPhase::Pending);
    assert_eq!(recovery.snapshot().resources.len(), 1);

    let mut still_blocked = submit_temp_file(
        filesystem,
        executor,
        &manager.as_async(),
        OperationOptions::default(),
        LocalTempFileOptions::new().with_parent(parent.path()),
    )
    .expect("operation itself is admitted");
    assert!(matches!(
        block_on(still_blocked.wait_outcome()).expect("outcome"),
        OperationOutcome::Failed(qubit_local_files::operation::LocalCreateFailure::Capacity)
    ));
}

struct RetryOwner {
    id: Arc<std::sync::Mutex<Option<qubit_operation::ResourceId>>>,
    live: Arc<AtomicUsize>,
}
impl Drop for RetryOwner {
    fn drop(&mut self) {
        self.live.fetch_sub(1, Ordering::SeqCst);
    }
}
struct RetryOnceCleanup {
    attempts: AtomicUsize,
}
impl AsyncCleanup<RetryOwner> for RetryOnceCleanup {
    fn cleanup(self: Arc<Self>, owner: RetryOwner, _context: CleanupContext) -> CleanupFuture<RetryOwner> {
        let attempt = self.attempts.fetch_add(1, Ordering::SeqCst);
        if attempt == 0 {
            let failure = CleanupFailure {
                resource_id: owner
                    .id
                    .lock()
                    .expect("resource id lock")
                    .expect("id assigned after commit"),
                stage: CleanupStage::Drop,
                summary: DiagnosticSummary::new("injected first-attempt cleanup failure"),
                source: None,
            };
            Box::pin(async move {
                CleanupOutcome::Failed {
                    failure,
                    resource: owner,
                }
            })
        } else {
            Box::pin(async move {
                drop(owner);
                CleanupOutcome::Released
            })
        }
    }
}

#[test]
fn public_recovery_retry_and_store_transfer_preserve_the_same_owner() {
    let manager = OperationManager::new(ManagerOptions::default()).expect("manager");
    let live = Arc::new(AtomicUsize::new(1));
    let id_slot = Arc::new(std::sync::Mutex::new(None));
    let cleanup = Arc::new(RetryOnceCleanup {
        attempts: AtomicUsize::new(0),
    });
    let completion = manager
        .run_and_settle(OperationOptions::default(), {
            let live = Arc::clone(&live);
            let id_slot = Arc::clone(&id_slot);
            let cleanup = Arc::clone(&cleanup);
            move |scope| {
                let reservation = scope.reserve_resource().expect("resource reservation");
                let resource = reservation
                    .commit_with(
                        RetryOwner {
                            id: Arc::clone(&id_slot),
                            live: Arc::clone(&live),
                        },
                        cleanup,
                    )
                    .expect("resource commit");
                *id_slot.lock().expect("id lock") = Some(resource.id());
                drop(resource);
                Ok::<(), ()>(())
            }
        })
        .expect("operation completion");
    assert_eq!(live.load(Ordering::SeqCst), 1);
    assert_eq!(completion.settlement.recoveries.len(), 1);
    let recovery = completion.settlement.recoveries[0].clone();
    let initial = recovery.snapshot();
    assert_eq!(
        initial.resources.as_ref(),
        &[id_slot.lock().expect("id lock").expect("id")]
    );
    assert_eq!(initial.attempts, 0);

    let store = RecoveryStore::new(RecoveryStoreOptions::default()).expect("store");
    let transfer = recovery.transfer_to(&store).expect("recovery handoff");
    let recovery_id = transfer.receipt.recovery();
    assert_eq!(recovery.snapshot().phase, RecoveryPhase::Transferred);
    assert_eq!(store.snapshot().resources, 1);
    let retried = store
        .try_retry(recovery_id)
        .expect("retry")
        .wait()
        .expect("retry result");
    assert!(retried.failures.is_empty());
    assert_eq!(live.load(Ordering::SeqCst), 0);
    assert_eq!(store.snapshot().resources, 0);
    store.request_shutdown().wait().expect("shutdown");
}

struct GateState {
    open: bool,
    waker: Option<Waker>,
}
struct Gate {
    state: Arc<Mutex<GateState>>,
}
struct GateOpen {
    state: Arc<Mutex<GateState>>,
}
impl Future for Gate {
    type Output = ();
    fn poll(self: std::pin::Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<()> {
        let mut state = self.state.lock().expect("gate lock");
        if state.open {
            Poll::Ready(())
        } else {
            state.waker = Some(cx.waker().clone());
            Poll::Pending
        }
    }
}
impl GateOpen {
    fn open(self) {
        let mut state = self.state.lock().expect("gate lock");
        state.open = true;
        if let Some(waker) = state.waker.take() {
            waker.wake();
        }
    }
}
fn gate() -> (Gate, GateOpen) {
    let state = Arc::new(Mutex::new(GateState {
        open: false,
        waker: None,
    }));
    (
        Gate {
            state: Arc::clone(&state),
        },
        GateOpen { state },
    )
}

#[test]
fn stop_after_creation_before_commit_retains_the_original_owner() {
    let manager = OperationManager::new(ManagerOptions::default()).expect("manager");
    let (gate, open) = gate();
    let (started_tx, started_rx) = std::sync::mpsc::channel();
    let live = Arc::new(AtomicUsize::new(0));
    let id_slot = Arc::new(Mutex::new(None));
    let cleanup = Arc::new(RetryOnceCleanup {
        attempts: AtomicUsize::new(0),
    });
    let mut operation = manager
        .as_async()
        .try_submit(OperationOptions::default(), {
            let live = Arc::clone(&live);
            let id_slot = Arc::clone(&id_slot);
            let cleanup = Arc::clone(&cleanup);
            move |scope| async move {
                let reservation = scope.reserve_resource().map_err(LocalCreateFailure::Reservation)?;
                started_tx.send(()).expect("test receiver");
                gate.await;
                live.fetch_add(1, Ordering::SeqCst);
                let owner = RetryOwner {
                    id: Arc::clone(&id_slot),
                    live: Arc::clone(&live),
                };
                match reservation.commit_with(owner, cleanup) {
                    Ok(reference) => {
                        *id_slot.lock().expect("id lock") = Some(reference.id());
                        drop(reference);
                        Ok(())
                    }
                    Err(error) => {
                        let (reason, owner, cleanup, reservation) = error.into_parts();
                        let id_slot = Arc::clone(&owner.id);
                        let mut id = id_slot.lock().expect("id lock");
                        let resource_id = reservation.retain_with_for_cleanup(owner, cleanup);
                        *id = Some(resource_id);
                        Err(LocalCreateFailure::Retained { reason, resource_id })
                    }
                }
            }
        })
        .expect("operation accepted");
    started_rx
        .recv_timeout(std::time::Duration::from_secs(2))
        .expect("reservation before gate");
    let _stop_status = operation.request_stop(qubit_operation::StopReason::UserCancelled);
    open.open();
    let _outcome = block_on(operation.wait_outcome()).expect("outcome available");
    let settlement = block_on(operation.wait_settlement());
    assert_eq!(live.load(Ordering::SeqCst), 1);
    assert_eq!(settlement.recoveries.len(), 1);
    assert_eq!(
        settlement.recoveries[0].snapshot().resources.as_ref(),
        &[id_slot.lock().expect("id lock").expect("retained resource id")]
    );
    let retry = settlement.recoveries[0]
        .try_retry()
        .expect("retry accepted")
        .wait()
        .expect("retry result");
    assert!(retry.failures.is_empty());
    assert_eq!(live.load(Ordering::SeqCst), 0);
}

#[test]
fn exhausted_resource_capacity_skips_creation() {
    let manager = OperationManager::new(ManagerOptions::default()).expect("manager");
    let creates = Arc::new(AtomicUsize::new(0));
    let mut options = OperationOptions::default();
    options.limits.resources = 1;
    let result = manager
        .run(options, {
            let creates = Arc::clone(&creates);
            move |scope| {
                let _first = scope.reserve_resource().expect("first reservation");
                match scope.reserve_resource() {
                    Err(qubit_operation::ResourceCapacityError::CapacityExceeded) => Ok::<(), ()>(()),
                    Err(error) => panic!("unexpected reservation error: {error}"),
                    Ok(_reservation) => {
                        creates.fetch_add(1, Ordering::SeqCst);
                        Ok::<(), ()>(())
                    }
                }
            }
        })
        .expect("operation run");
    assert!(matches!(result, OperationOutcome::Succeeded(())));
    assert_eq!(creates.load(Ordering::SeqCst), 0);
}

#[test]
fn open_zero_reference_resource_can_be_reacquired_but_ended_scope_cannot() {
    let manager = OperationManager::new(ManagerOptions::default()).expect("manager");
    let result = manager
        .run(OperationOptions::default(), |scope| {
            let resource_scope = scope
                .resource_scope(qubit_operation::ResourceScopeOptions { capacity: 1 })
                .expect("resource scope");
            let reference = resource_scope
                .manage(String::from("reserved owner"))
                .expect("managed resource");
            let key = reference.key();
            let id = reference.id();
            drop(reference);
            let reacquired = resource_scope.get_resource(key).expect("Open+0 remains reachable");
            assert_eq!(reacquired.id(), id);
            drop(reacquired);
            resource_scope.end();
            assert!(matches!(
                resource_scope.get_resource(key),
                Err(qubit_operation::ResourceLookupError::ScopeEnded)
            ));
            Ok::<(), ()>(())
        })
        .expect("operation run");
    assert!(matches!(result, OperationOutcome::Succeeded(())));
}
