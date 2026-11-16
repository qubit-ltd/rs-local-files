use std::io::Read;
use std::io::Write;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
use std::task::Context;
use std::task::Poll;

use qubit_io::AsyncInput;
use qubit_local_files::LocalFileSystem;
use qubit_local_files::operation::LocalExecutionConfig;
use qubit_local_files::operation::LocalIoExecutor;
use qubit_local_files::operation::handoff::LocalStageFailure;
use qubit_local_files::operation::handoff::LocalStageOptions;
use qubit_local_files::operation::handoff::stage_bytes;
use qubit_local_files::operation::handoff::stage_stream;
use qubit_operation::ManagerOptions;
use qubit_operation::OperationManager;
use qubit_operation::OperationOptions;
use qubit_operation::OperationOutcome;
use qubit_operation::StopReason;

fn block_on<F: std::future::Future>(future: F) -> F::Output {
    use std::sync::Arc;
    use std::sync::Condvar;
    use std::sync::Mutex;
    use std::task::Context;
    use std::task::Poll;
    use std::task::Wake;
    use std::task::Waker;

    struct Signal((Mutex<bool>, Condvar));
    impl Wake for Signal {
        fn wake(self: Arc<Self>) {
            let (lock, signal) = &self.0;
            *lock.lock().unwrap() = true;
            signal.notify_one();
        }
        fn wake_by_ref(self: &Arc<Self>) {
            let (lock, signal) = &self.0;
            *lock.lock().unwrap() = true;
            signal.notify_one();
        }
    }

    let signal = Arc::new(Signal((Mutex::new(false), Condvar::new())));
    let waker = Waker::from(Arc::clone(&signal));
    let mut context = Context::from_waker(&waker);
    let mut future = std::pin::pin!(future);
    loop {
        if let Poll::Ready(value) = future.as_mut().poll(&mut context) {
            return value;
        }
        let (lock, condvar) = &signal.0;
        let mut ready = lock.lock().unwrap();
        while !*ready {
            ready = condvar.wait(ready).unwrap();
        }
        *ready = false;
    }
}

fn setup() -> (OperationManager, LocalIoExecutor, LocalFileSystem) {
    let manager = OperationManager::new(ManagerOptions::default()).unwrap();
    let executor = LocalIoExecutor::new(manager.clone(), LocalExecutionConfig::default()).unwrap();
    let filesystem = LocalFileSystem::host().unwrap();
    (manager, executor, filesystem)
}

struct SliceInput {
    bytes: Vec<u8>,
    position: usize,
    polls: Arc<AtomicUsize>,
}

impl AsyncInput for SliceInput {
    type Item = u8;

    unsafe fn poll_read_unchecked(
        self: Pin<&mut Self>,
        _cx: &mut Context<'_>,
        output: &mut [u8],
        index: usize,
        count: usize,
    ) -> Poll<std::io::Result<usize>> {
        let this = self.get_mut();
        this.polls.fetch_add(1, Ordering::SeqCst);
        let count = count.min(this.bytes.len().saturating_sub(this.position));
        output[index..index + count].copy_from_slice(&this.bytes[this.position..this.position + count]);
        this.position += count;
        Poll::Ready(Ok(count))
    }
}

fn slice_input(bytes: &[u8]) -> (SliceInput, Arc<AtomicUsize>) {
    let polls = Arc::new(AtomicUsize::new(0));
    (
        SliceInput {
            bytes: bytes.to_vec(),
            position: 0,
            polls: Arc::clone(&polls),
        },
        polls,
    )
}

#[test]
fn stage_bytes_rejects_over_limit_and_returns_the_original_vec_before_admission() {
    let manager = OperationManager::new(ManagerOptions::default()).unwrap();
    let executor = LocalIoExecutor::new(
        manager.clone(),
        LocalExecutionConfig {
            max_owned_bytes: 16,
            ..LocalExecutionConfig::default()
        },
    )
    .unwrap();
    let filesystem = LocalFileSystem::host().unwrap();
    let bytes = Vec::with_capacity(32);
    let pointer = bytes.as_ptr();
    let result = stage_bytes(
        filesystem,
        executor,
        &manager.as_async(),
        OperationOptions::default(),
        bytes,
        LocalStageOptions::default(),
    );
    let returned = match result {
        Err(error) => error.into_bytes(),
        Ok(_) => panic!("over-limit stage must be rejected before admission"),
    };
    assert_eq!(returned.capacity(), 32);
    assert_eq!(returned.as_ptr(), pointer);
}

#[test]
fn sealed_export_is_read_only_and_keeps_same_resource_until_last_helper_releases() {
    let (manager, executor, filesystem) = setup();
    let mut operation = stage_bytes(
        filesystem,
        executor.clone(),
        &manager.as_async(),
        OperationOptions::default(),
        b"staged payload".to_vec(),
        LocalStageOptions::default(),
    )
    .expect("stage should be admitted");
    let OperationOutcome::Succeeded(lease) = block_on(operation.wait_outcome()).unwrap() else {
        panic!("stage should seal successfully");
    };
    let helper = lease.clone();
    let helper_two = lease.clone();
    assert_eq!(lease.resource_id(), helper.resource_id());
    let export = block_on(helper.export_read_handle(&executor)).expect("sealed handle export");
    let mut file = export.file.try_clone().unwrap();
    let mut bytes = Vec::new();
    file.read_to_end(&mut bytes).unwrap();
    assert_eq!(bytes, b"staged payload");
    assert!(file.write_all(b"x").is_err());
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        assert_eq!(file.metadata().unwrap().mode() & 0o077, 0);
    }
    drop(file);

    let mut settlement = Box::pin(operation.wait_settlement());
    let waker = std::task::Waker::noop();
    let mut context = Context::from_waker(waker);
    assert!(
        matches!(
            std::future::Future::poll(settlement.as_mut(), &mut context),
            Poll::Pending
        ),
        "outcome readiness must not end helper-held resource lifetime"
    );
    drop(lease);
    drop(export);
    drop(helper);
    assert!(matches!(
        std::future::Future::poll(settlement.as_mut(), &mut context),
        Poll::Pending
    ));
    drop(helper_two);
    assert_eq!(block_on(settlement).recovery_count, 0);
}

#[test]
fn stream_uses_one_sentinel_read_for_exact_eof_and_rejects_over_limit() {
    let (manager, executor, filesystem) = setup();
    let (input, polls) = slice_input(b"abc");
    let mut exact = stage_stream(
        filesystem.clone(),
        executor.clone(),
        &manager.as_async(),
        OperationOptions::default(),
        input,
        LocalStageOptions {
            max_content_bytes: 3,
            disk_reservation_bytes: 3,
            ..LocalStageOptions::default()
        },
    )
    .unwrap();
    let OperationOutcome::Succeeded(lease) = block_on(exact.wait_outcome()).unwrap() else {
        panic!("exact EOF at the limit should stage successfully");
    };
    assert_eq!(polls.load(Ordering::SeqCst), 2, "one full read plus the EOF sentinel");
    let export = block_on(lease.export_read_handle(&executor)).unwrap();
    let mut contents = Vec::new();
    export.file.try_clone().unwrap().read_to_end(&mut contents).unwrap();
    assert_eq!(contents, b"abc");
    drop(export);
    drop(lease);
    block_on(exact.wait_settlement());

    let (input, polls) = slice_input(b"abcd");
    let mut over = stage_stream(
        filesystem,
        executor,
        &manager.as_async(),
        OperationOptions::default(),
        input,
        LocalStageOptions {
            max_content_bytes: 3,
            disk_reservation_bytes: 3,
            ..LocalStageOptions::default()
        },
    )
    .unwrap();
    assert!(matches!(
        block_on(over.wait_outcome()).unwrap(),
        OperationOutcome::Failed(LocalStageFailure::ContentTooLarge)
    ));
    assert_eq!(polls.load(Ordering::SeqCst), 2, "the second read is the sentinel");
    assert_eq!(block_on(over.wait_settlement()).recovery_count, 0);
}

#[test]
fn rooted_export_is_denied_by_default_and_allowed_only_by_explicit_option() {
    let root = tempfile::tempdir().unwrap();
    let filesystem = LocalFileSystem::rooted(root.path()).unwrap();
    let manager = OperationManager::new(ManagerOptions::default()).unwrap();
    let executor = LocalIoExecutor::new(manager.clone(), LocalExecutionConfig::default()).unwrap();
    let mut denied = stage_bytes(
        filesystem.clone(),
        executor.clone(),
        &manager.as_async(),
        OperationOptions::default(),
        b"rooted".to_vec(),
        LocalStageOptions::default(),
    )
    .unwrap();
    let OperationOutcome::Succeeded(lease) = block_on(denied.wait_outcome()).unwrap() else {
        panic!("Rooted stage should succeed");
    };
    assert!(matches!(
        block_on(lease.export_read_handle(&executor)),
        Err(LocalStageFailure::RootedExportDenied)
    ));
    drop(lease);
    block_on(denied.wait_settlement());

    let mut allowed = stage_bytes(
        filesystem,
        executor.clone(),
        &manager.as_async(),
        OperationOptions::default(),
        b"rooted".to_vec(),
        LocalStageOptions {
            export_read_handle: true,
            ..LocalStageOptions::default()
        },
    )
    .unwrap();
    let OperationOutcome::Succeeded(lease) = block_on(allowed.wait_outcome()).unwrap() else {
        panic!("Rooted stage should succeed");
    };
    let export = block_on(lease.export_read_handle(&executor)).unwrap();
    assert_eq!(lease.resource_id(), export.lease.resource_id());
    drop(export);
    drop(lease);
    block_on(allowed.wait_settlement());
}

#[test]
fn stream_cancellation_interrupts_a_pending_owned_input() {
    struct PendingInput;
    impl AsyncInput for PendingInput {
        type Item = u8;
        unsafe fn poll_read_unchecked(
            self: Pin<&mut Self>,
            _cx: &mut Context<'_>,
            _output: &mut [u8],
            _index: usize,
            _count: usize,
        ) -> Poll<std::io::Result<usize>> {
            Poll::Pending
        }
    }

    let (manager, executor, filesystem) = setup();
    let mut operation = stage_stream(
        filesystem,
        executor,
        &manager.as_async(),
        OperationOptions::default(),
        PendingInput,
        LocalStageOptions::default(),
    )
    .unwrap();
    let _ = operation.request_stop(StopReason::UserCancelled);
    assert!(matches!(
        block_on(operation.wait_outcome()).unwrap(),
        OperationOutcome::Stopped(_) | OperationOutcome::Failed(LocalStageFailure::Stopped)
    ));
}
