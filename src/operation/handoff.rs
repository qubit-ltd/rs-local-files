// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

//! Owned staging and read-only handoff for managed local temporary files.

use std::future::poll_fn;
use std::io::Write;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::Mutex;
use std::task::Context;
use std::task::Poll;

use qubit_io::AsyncInput;
use qubit_operation::AsyncOperationHandle;
use qubit_operation::OperationOptions;
use qubit_operation::OperationOutcome;
use qubit_operation::ResourceRef;

use super::temp::LocalTempFileAccess;
use crate::operation::executor::LocalIoExecutor;
use crate::operation::temp::LocalTempCommandError;
use crate::operation::temp::LocalTempNative;
use crate::operation::temp::LocalTempResource;
use crate::operation::temp::submit_temp_file;

mod local_read_export;
mod local_read_lease;
mod local_read_resource;
mod local_stage_failure;
mod local_stage_options;
mod local_stage_stream_submit_error;
mod local_stage_submit_error;
mod stage_bytes_submission;

pub use local_read_export::LocalReadExport;
pub use local_read_lease::LocalReadLease;
pub use local_read_resource::LocalReadResource;
pub use local_stage_failure::LocalStageFailure;
pub use local_stage_options::LocalStageOptions;
pub use local_stage_stream_submit_error::LocalStageStreamSubmitError;
pub use local_stage_submit_error::LocalStageSubmitError;
use stage_bytes_submission::StageBytesSubmission;

/// Typed operation returned by [`stage_bytes`].
pub type LocalStageOperation = AsyncOperationHandle<LocalReadLease, LocalStageFailure>;

/// Stages owned bytes, seals the managed temporary file, and returns a lease.
/// The owned byte capacity is reserved before admission and returned on any
/// pre-admission rejection.
///
/// # Parameters
///
/// * `filesystem` — local filesystem used to create the managed temporary file.
/// * `executor` — bounded worker used for blocking filesystem operations.
/// * `manager` — operation manager that owns admission and cleanup scope.
/// * `operation` — cancellation and resource options for this stage.
/// * `bytes` — payload transferred to the accepted operation.
/// * `stage` — payload, disk, privacy, and export limits.
///
/// Admission follows the supplied manager view. When it is an external view
/// outside the executor's group, it may keep accepting stages after that
/// group begins draining. The executor facade applies group-scoped admission
/// and rejects new facade submissions once its group is draining.
pub fn stage_bytes(
    filesystem: crate::LocalFileSystem,
    executor: LocalIoExecutor,
    manager: &qubit_operation::AsyncOperationManager,
    operation: OperationOptions,
    bytes: Vec<u8>,
    stage: LocalStageOptions,
) -> Result<LocalStageOperation, LocalStageSubmitError> {
    if bytes.len() > stage.max_content_bytes {
        return Err(LocalStageSubmitError::Budget { bytes });
    }
    if stage.disk_reservation_bytes < (bytes.len() as u64).max(1) {
        return Err(LocalStageSubmitError::Budget { bytes });
    }
    let owned = match executor.reserve_bytes(bytes.capacity()) {
        Some(value) => value,
        None => return Err(LocalStageSubmitError::Budget { bytes }),
    };
    // `submit_temp_file` reserves one byte on the native owner; reserve the
    // remainder here so total stage disk accounting stays within one limit.
    let disk = match executor.reserve_disk(stage.disk_reservation_bytes.saturating_sub(1)) {
        Some(value) => value,
        None => return Err(LocalStageSubmitError::Budget { bytes }),
    };
    let payload = Arc::new(Mutex::new(Some(bytes)));
    let body_payload = Arc::clone(&payload);
    let child_operation = operation.clone();
    let submit = manager.try_submit(operation, move |scope| async move {
        let bytes = body_payload
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take()
            .expect("accepted stage retains its owned payload");
        let temp_options = crate::options::LocalTempFileOptions::new().with_privacy(stage.privacy);
        stage_bytes_in_scope(
            filesystem,
            executor,
            scope,
            StageBytesSubmission {
                operation: child_operation,
                bytes,
                stage,
                owned,
                disk,
                temp_options,
            },
        )
        .await
    });
    match submit {
        Ok(handle) => Ok(handle),
        Err(error) => {
            let (reason, _body) = error.into_parts();
            let bytes = payload
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .take()
                .unwrap_or_default();
            Err(LocalStageSubmitError::Admission { reason, bytes })
        }
    }
}

/// Writes one accepted payload and seals the resulting managed temporary file.
/// The reservations are transferred to the resource before the input allocation
/// reservation is released.
///
/// # Parameters
///
/// * `filesystem` — filesystem used to create the temporary file.
/// * `executor` — worker that performs all blocking file operations.
/// * `scope` — accepted operation scope that owns cancellation and resources.
/// * `operation` — child operation options passed to temporary-file creation.
/// * `bytes` — payload written to the new file.
/// * `stage` — limits and privacy/export requirements for the stage.
/// * `owned` — reservation for the caller's owned payload allocation.
/// * `disk` — additional disk capacity reserved for the staged contents.
/// * `temp_options` — native temporary-file creation policy.
async fn stage_bytes_in_scope(
    filesystem: crate::LocalFileSystem,
    executor: LocalIoExecutor,
    scope: qubit_operation::OperationScope,
    submission: StageBytesSubmission,
) -> Result<LocalReadLease, LocalStageFailure> {
    let StageBytesSubmission {
        operation,
        bytes,
        stage,
        owned,
        disk,
        temp_options,
    } = submission;
    let authority_identity = Arc::as_ptr(&filesystem.core) as usize;
    let context = scope.context();
    if context.stop_reason().is_some() {
        return Err(LocalStageFailure::Stopped);
    }
    let access = create_temp_access(filesystem, executor.clone(), &scope, operation, temp_options, disk).await?;
    if context.stop_reason().is_some() {
        return Err(LocalStageFailure::Stopped);
    }
    write_access_chunk(&access, &executor, bytes, context.clone()).await?;
    #[cfg(feature = "test-support")]
    crate::test_support::hit_test_gate_for_authority("lf6-stage-bytes-after-write", authority_identity);
    if context.stop_reason().is_some() {
        return Err(LocalStageFailure::Stopped);
    }
    drop(owned);
    seal_access(access, executor, stage, context, authority_identity).await
}

/// Creates a child temporary file within the accepted operation scope.
///
/// # Parameters
///
/// * `filesystem` — filesystem used to create the native temporary file.
/// * `executor` — worker that performs the native creation operation.
/// * `scope` — parent operation scope used to create and own the child.
/// * `operation` — options applied to the child operation.
/// * `temp_options` — privacy and creation requirements for the temporary file.
/// * `disk` — disk reservation retained by the new resource.
async fn create_temp_access(
    filesystem: crate::LocalFileSystem,
    executor: LocalIoExecutor,
    scope: &qubit_operation::OperationScope,
    operation: OperationOptions,
    temp_options: crate::options::LocalTempFileOptions,
    disk: super::internal::permits::DiskPermit,
) -> Result<LocalTempFileAccess, LocalStageFailure> {
    if scope.context().stop_reason().is_some() {
        return Err(LocalStageFailure::Stopped);
    }
    let child_manager = scope.manager();
    let async_manager = child_manager.as_async();
    let mut create = submit_temp_file(filesystem, executor.clone(), &async_manager, operation, temp_options)
        .map_err(|_| LocalStageFailure::Capacity)?;
    let access = match create
        .wait_outcome()
        .await
        .map_err(|_| LocalStageFailure::ChildOutcome)?
    {
        OperationOutcome::Succeeded(access) => access,
        OperationOutcome::Failed(error) => return Err(LocalStageFailure::Child(error)),
        _ => return Err(LocalStageFailure::ChildOutcome),
    };
    if scope.context().stop_reason().is_some() {
        drop(access);
        return Err(LocalStageFailure::Stopped);
    }
    access.retain_disk_permit(disk);
    Ok(access)
}

/// Writes one complete payload chunk through the serialized native command
/// queue.
///
/// # Parameters
///
/// * `access` — registered file access token for the writable temporary file.
/// * `executor` — worker queue that serializes access to the native file.
/// * `bytes` — owned chunk written without copying into a second buffer.
/// * `context` — operation context checked before the write begins.
async fn write_access_chunk(
    access: &LocalTempFileAccess,
    executor: &LocalIoExecutor,
    bytes: Vec<u8>,
    context: qubit_operation::OperationContext,
) -> Result<(), LocalStageFailure> {
    let receiver = access
        .enqueue_native_command(executor, move |native| {
            if context.stop_reason().is_some() {
                return Err(LocalStageFailure::Stopped);
            }
            match native {
                LocalTempNative::File(file) => file
                    .as_file_mut()
                    .and_then(|handle| handle.write_all(&bytes))
                    .map_err(LocalStageFailure::Io),
                _ => Err(LocalStageFailure::NotSealed),
            }
        })
        .map_err(command_error)?;
    receiver.await.map_err(command_error)??;
    Ok(())
}

/// Converts the writable temporary file into a sealed read-only resource.
///
/// # Parameters
///
/// * `access` — registered owner whose file is being sealed.
/// * `executor` — worker that performs the native seal transition.
/// * `stage` — synchronization and export policy applied to sealing.
/// * `context` — operation context checked before committing the transition.
async fn seal_access(
    access: LocalTempFileAccess,
    executor: LocalIoExecutor,
    stage: LocalStageOptions,
    context: qubit_operation::OperationContext,
    authority_identity: usize,
) -> Result<LocalReadLease, LocalStageFailure> {
    let command_context = context.clone();
    let receiver = access
        .enqueue_native_transition(&executor, move |native| {
            let LocalTempNative::File(file) = native else {
                return Err((native, LocalStageFailure::NotSealed));
            };
            if command_context.stop_reason().is_some() {
                return Err((LocalTempNative::File(file), LocalStageFailure::Stopped));
            }
            #[cfg(all(test, feature = "test-support"))]
            tests::SEAL_ATTEMPTS.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
            let transition = match file
                .seal_with_options(&crate::options::LocalSealOptions::new().with_file_sync(stage.file_sync))
            {
                Ok(sealed) => Ok((Some(LocalTempNative::Sealed(sealed)), ())),
                Err(error) => {
                    let (cause, access_state, file) = error.into_parts();
                    Err((
                        LocalTempNative::File(file),
                        LocalStageFailure::Seal { cause, access_state },
                    ))
                }
            };
            // Hold the command result after the native seal has completed so
            // cancellation can be requested deterministically in tests. The
            // transition bridge still stores the sealed owner before waking
            // the operation future.
            #[cfg(feature = "test-support")]
            crate::test_support::hit_test_gate_for_authority("lf6-stage-bytes-after-native-seal", authority_identity);
            transition
        })
        .map_err(command_error)?;
    receiver.await.map_err(command_error)??;
    if context.stop_reason().is_some() {
        // The transition has already installed the sealed owner in the
        // managed resource. Returning Stopped drops this access and lets the
        // normal scope cleanup remove the sealed file.
        return Err(LocalStageFailure::Stopped);
    }
    Ok(LocalReadLease::from_temp_access(&access, stage.export_read_handle))
}

/// Reads and stages an owned asynchronous input in bounded chunks.
///
/// When the configured content limit is reached, one additional byte is read
/// as an EOF sentinel. The input is polled by the operation future; filesystem
/// access and sealing remain on the bounded local worker.
///
/// # Type Parameters
///
/// * `I` — asynchronous byte input transferred to the accepted operation.
///
/// # Parameters
///
/// * `filesystem` — filesystem used to create the staged temporary file.
/// * `executor` — bounded worker for blocking file operations.
/// * `manager` — operation manager that owns admission and resource cleanup.
/// * `operation` — cancellation and resource options for this stage.
/// * `input` — asynchronous payload source owned by the stage.
/// * `stage` — content, disk, privacy, and export requirements.
///
/// Admission follows the supplied manager view. An external view outside the
/// executor's group may continue accepting stages while the executor facade's
/// group is draining; facade submissions are rejected once that group closes
/// admission. Cleanup remains available for already-accepted resources.
pub fn stage_stream<I>(
    filesystem: crate::LocalFileSystem,
    executor: LocalIoExecutor,
    manager: &qubit_operation::AsyncOperationManager,
    operation: OperationOptions,
    input: I,
    stage: LocalStageOptions,
) -> Result<LocalStageOperation, LocalStageStreamSubmitError<I>>
where
    I: AsyncInput<Item = u8> + Send + 'static,
{
    if stage.disk_reservation_bytes < (stage.max_content_bytes as u64).max(1) {
        return Err(LocalStageStreamSubmitError::Budget { input });
    }
    let disk = match executor.reserve_disk(stage.disk_reservation_bytes.saturating_sub(1)) {
        Some(value) => value,
        None => return Err(LocalStageStreamSubmitError::Budget { input }),
    };
    let input_owner = Arc::new(Mutex::new(Some(input)));
    let body_input = Arc::clone(&input_owner);
    let authority_identity = Arc::as_ptr(&filesystem.core) as usize;
    let task_executor = executor.clone();
    let child_operation = operation.clone();
    let submit = manager.try_submit(operation, move |scope| async move {
        let input = body_input
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take()
            .expect("accepted stage retains its owned input");
        let mut input = Box::pin(input);
        let max_chunk = LOCAL_STAGE_STREAM_CHUNK_SIZE
            .min(stage.max_content_bytes.max(1))
            .min(task_executor.config().max_owned_bytes);
        let temp_options = crate::options::LocalTempFileOptions::new().with_privacy(stage.privacy);
        let access = create_temp_access(
            filesystem,
            task_executor.clone(),
            &scope,
            child_operation,
            temp_options,
            disk,
        )
        .await?;
        let mut total = 0_usize;
        let context = scope.context();
        let mut stop = context.stop_requested();
        loop {
            let at_limit = total == stage.max_content_bytes;
            let requested = if at_limit {
                1
            } else {
                (stage.max_content_bytes - total).min(max_chunk)
            };
            let Some(chunk_permit) = task_executor.reserve_bytes(requested) else {
                return Err(LocalStageFailure::Capacity);
            };
            let mut chunk = vec![0_u8; requested];
            let read = poll_fn(|cx: &mut Context<'_>| {
                if Pin::new(&mut stop).poll(cx).is_ready() {
                    return Poll::Ready(Err(LocalStageFailure::Stopped));
                }
                match input.as_mut().poll_read(cx, &mut chunk) {
                    Poll::Ready(Ok(count)) => Poll::Ready(Ok(count)),
                    Poll::Ready(Err(error)) => Poll::Ready(Err(LocalStageFailure::Io(error))),
                    Poll::Pending => Poll::Pending,
                }
            })
            .await?;
            if read == 0 {
                break;
            }
            if at_limit {
                return Err(LocalStageFailure::ContentTooLarge);
            }
            chunk.truncate(read);
            write_access_chunk(&access, &task_executor, chunk, context.clone()).await?;
            drop(chunk_permit);
            total = total.saturating_add(read);
        }
        if context.stop_reason().is_some() {
            return Err(LocalStageFailure::Stopped);
        }
        seal_access(access, task_executor, stage, context, authority_identity).await
    });
    match submit {
        Ok(handle) => Ok(handle),
        Err(error) => {
            let (reason, _) = error.into_parts();
            let input = input_owner
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .take()
                .expect("rejected operation returns its owned input");
            Err(LocalStageStreamSubmitError::Admission { reason, input })
        }
    }
}

/// Chunk bound used while polling an asynchronous stage input.
pub const LOCAL_STAGE_STREAM_CHUNK_SIZE: usize = 64 * 1024;

/// Schedules one command while retaining the resource reference until
/// completion.
///
/// # Type Parameters
///
/// * `T` — value produced by the native command.
/// * `F` — command closure executed by the local worker.
///
/// # Parameters
///
/// * `executor` — bounded worker queue receiving the command.
/// * `reference` — registered resource retained for the command's lifetime.
/// * `command` — operation performed while the native owner is locked.
fn enqueue_resource_command<T, F>(
    executor: &LocalIoExecutor,
    reference: ResourceRef<LocalTempResource>,
    command: F,
) -> Result<super::temp::LocalTempCommandReceiver<T>, LocalTempCommandError>
where
    T: Send + 'static,
    F: FnOnce(&mut LocalTempNative) -> T + Send + 'static,
{
    super::temp::enqueue_resource_command(executor, reference, command)
}

/// Converts a worker command failure into the public stage error type.
///
/// # Parameters
///
/// * `error` — internal scheduling or native-command failure.
fn command_error(error: LocalTempCommandError) -> LocalStageFailure {
    let detail = match error {
        LocalTempCommandError::Capacity => "local worker queue capacity exhausted",
        LocalTempCommandError::ResourceUnavailable => "managed temp resource is unavailable",
        LocalTempCommandError::UnsupportedKind => "managed temp resource kind is unsupported",
        LocalTempCommandError::Panicked => "managed temp command panicked; completion is unconfirmed",
    };
    LocalStageFailure::Command(detail.to_owned())
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    #[cfg(feature = "test-support")]
    use std::sync::Mutex;
    #[cfg(feature = "test-support")]
    use std::sync::atomic::AtomicUsize;
    use std::task::Context;
    use std::task::Poll;
    use std::task::Wake;
    use std::task::Waker;

    use qubit_operation::OperationOptions;
    use qubit_operation::OperationOutcome;

    use super::LocalReadLease;
    use super::LocalStageFailure;
    use super::LocalStageOptions;
    use super::LocalTempCommandError;
    use super::command_error;
    use super::stage_bytes;
    use crate::operation::executor::LocalIoExecutor;
    use crate::operation::temp::submit_temp_file;

    #[cfg(feature = "test-support")]
    pub(super) static SEAL_ATTEMPTS: AtomicUsize = AtomicUsize::new(0);
    #[cfg(feature = "test-support")]
    static SEAL_TEST_LOCK: Mutex<()> = Mutex::new(());

    struct ThreadWake(std::thread::Thread);
    impl Wake for ThreadWake {
        fn wake(self: Arc<Self>) {
            self.0.unpark();
        }
        fn wake_by_ref(self: &Arc<Self>) {
            self.0.unpark();
        }
    }

    fn block_on<F: std::future::Future>(future: F) -> F::Output {
        let waker = Waker::from(Arc::new(ThreadWake(std::thread::current())));
        let mut context = Context::from_waker(&waker);
        let mut future = std::pin::pin!(future);
        loop {
            if let Poll::Ready(output) = future.as_mut().poll(&mut context) {
                return output;
            }
            std::thread::park();
        }
    }

    #[test]
    fn test_lease_from_temp_access_reuses_the_registered_resource() {
        let manager =
            qubit_operation::OperationManager::new(qubit_operation::ManagerOptions::default()).expect("manager");
        let executor = LocalIoExecutor::new(manager.clone(), crate::operation::LocalExecutionConfig::default())
            .expect("local executor");
        let filesystem = crate::LocalFileSystem::host().expect("filesystem");
        let mut create = submit_temp_file(
            filesystem,
            executor,
            &manager.as_async(),
            OperationOptions::default(),
            crate::options::LocalTempFileOptions::new(),
        )
        .expect("temp operation");
        let OperationOutcome::Succeeded(access) = block_on(create.wait_outcome()).unwrap() else {
            panic!("temp file should be created");
        };
        let lease = LocalReadLease::from_temp_access(&access, true);
        assert_eq!(lease.resource_id(), access.resource_id());
        let clone = access.clone_access();
        assert_eq!(clone.resource_id(), lease.resource_id());
        drop(access);
        drop(clone);
        drop(lease);
        assert_eq!(block_on(create.wait_settlement()).recovery_count, 0);
    }

    #[test]
    fn test_repeated_exports_share_file_offset_and_hold_resource_until_last_lease() {
        use std::io::Read;

        #[cfg(feature = "test-support")]
        let _guard = SEAL_TEST_LOCK.lock().unwrap_or_else(|error| error.into_inner());
        let manager =
            qubit_operation::OperationManager::new(qubit_operation::ManagerOptions::default()).expect("manager");
        let executor = LocalIoExecutor::new(manager.clone(), crate::operation::LocalExecutionConfig::default())
            .expect("local executor");
        let mut operation = stage_bytes(
            crate::LocalFileSystem::host().expect("filesystem"),
            executor.clone(),
            &manager.as_async(),
            OperationOptions::default(),
            b"shared cursor".to_vec(),
            LocalStageOptions::default(),
        )
        .expect("stage accepted");
        let OperationOutcome::Succeeded(lease) = block_on(operation.wait_outcome()).expect("stage outcome") else {
            panic!("stage should produce a lease");
        };
        let mut first = block_on(lease.export_read_handle(&executor)).expect("first export");
        let mut second = block_on(lease.export_read_handle(&executor)).expect("second export");

        let mut prefix = [0_u8; 7];
        first.file.read_exact(&mut prefix).expect("read first prefix");
        assert_eq!(&prefix, b"shared ");
        let mut suffix = Vec::new();
        second.file.read_to_end(&mut suffix).expect("read shared suffix");
        assert_eq!(suffix, b"cursor");

        drop(lease);
        drop(first);
        // The second export's lease still owns the resource after the other
        // lease clones are gone, so its file remains available until this drop.
        let mut tail = [0_u8; 1];
        assert_eq!(second.file.read(&mut tail).expect("read at shared EOF"), 0);
        drop(second);
        let settlement = block_on(operation.wait_settlement());
        assert_eq!(settlement.recovery_count, 0);
        assert!(settlement.cleanup_failures.is_empty());
    }

    #[test]
    fn test_command_error_preserves_each_worker_failure_category() {
        let cases = [
            (LocalTempCommandError::Capacity, "local worker queue capacity exhausted"),
            (
                LocalTempCommandError::ResourceUnavailable,
                "managed temp resource is unavailable",
            ),
            (
                LocalTempCommandError::UnsupportedKind,
                "managed temp resource kind is unsupported",
            ),
            (
                LocalTempCommandError::Panicked,
                "managed temp command panicked; completion is unconfirmed",
            ),
        ];
        for (error, expected) in cases {
            let LocalStageFailure::Command(message) = command_error(error) else {
                panic!("worker command errors must remain stage command failures");
            };
            assert_eq!(message, expected);
        }
    }

    #[cfg(feature = "test-support")]
    #[test]
    fn test_stop_after_queued_write_prevents_the_following_seal_command() {
        let _guard = SEAL_TEST_LOCK.lock().unwrap_or_else(|error| error.into_inner());
        let manager =
            qubit_operation::OperationManager::new(qubit_operation::ManagerOptions::default()).expect("manager");
        let executor = LocalIoExecutor::new(manager.clone(), crate::operation::LocalExecutionConfig::default())
            .expect("local executor");
        let filesystem = crate::LocalFileSystem::host().expect("filesystem");
        let gate = crate::test_support::install_test_gate_for_authority(
            "lf6-stage-bytes-after-write",
            Arc::as_ptr(&filesystem.core) as usize,
        )
        .expect("write boundary gate");
        SEAL_ATTEMPTS.store(0, std::sync::atomic::Ordering::SeqCst);
        let mut operation = stage_bytes(
            filesystem,
            executor,
            &manager.as_async(),
            OperationOptions::default(),
            b"payload".to_vec(),
            LocalStageOptions::default(),
        )
        .expect("stage admitted");
        gate.wait_until_entered();
        let _ = operation.request_stop(qubit_operation::StopReason::UserCancelled);
        gate.release();
        assert!(matches!(
            block_on(operation.wait_outcome()).expect("operation outcome"),
            OperationOutcome::Stopped(_) | OperationOutcome::Failed(LocalStageFailure::Stopped)
        ));
        assert_eq!(SEAL_ATTEMPTS.load(std::sync::atomic::Ordering::SeqCst), 0);
        assert_eq!(block_on(operation.wait_settlement()).recovery_count, 0);
    }

    #[cfg(feature = "test-support")]
    #[test]
    fn test_stop_after_native_seal_keeps_the_sealed_owner_for_managed_cleanup() {
        let _guard = SEAL_TEST_LOCK.lock().unwrap_or_else(|error| error.into_inner());
        let manager =
            qubit_operation::OperationManager::new(qubit_operation::ManagerOptions::default()).expect("manager");
        let executor = LocalIoExecutor::new(manager.clone(), crate::operation::LocalExecutionConfig::default())
            .expect("local executor");
        let filesystem = crate::LocalFileSystem::host().expect("filesystem");
        let gate = crate::test_support::install_test_gate_for_authority(
            "lf6-stage-bytes-after-native-seal",
            Arc::as_ptr(&filesystem.core) as usize,
        )
        .expect("post-seal gate");
        SEAL_ATTEMPTS.store(0, std::sync::atomic::Ordering::SeqCst);
        let mut operation = stage_bytes(
            filesystem,
            executor,
            &manager.as_async(),
            OperationOptions::default(),
            b"payload".to_vec(),
            LocalStageOptions::default(),
        )
        .expect("stage admitted");
        gate.wait_until_entered();
        let _ = operation.request_stop(qubit_operation::StopReason::UserCancelled);
        gate.release();
        assert!(matches!(
            block_on(operation.wait_outcome()).expect("operation outcome"),
            OperationOutcome::Stopped(_) | OperationOutcome::Failed(LocalStageFailure::Stopped)
        ));
        assert_eq!(SEAL_ATTEMPTS.load(std::sync::atomic::Ordering::SeqCst), 1);
        let settlement = block_on(operation.wait_settlement());
        assert_eq!(settlement.recovery_count, 0);
        assert!(settlement.cleanup_failures.is_empty());
    }
}
