//! Owned staging and read-only handoff for managed local temporary files.

use std::fs::File;
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

use super::executor::LocalIoExecutor;
use super::internal::permits::Permit;
use super::temp::LocalCreateFailure;
use super::temp::LocalTempCommandError;
use super::temp::LocalTempFileAccess;
use super::temp::LocalTempNative;
use super::temp::LocalTempResource;
use super::temp::submit_temp_file;

/// Bounds and controls for one staged payload.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LocalStageOptions {
    /// Maximum accepted payload length.
    pub max_content_bytes: usize,
    /// Disk reservation retained for the staged file.
    pub disk_reservation_bytes: u64,
    /// Privacy requested while creating the host temporary resource.
    pub privacy: crate::policy::LocalPrivacyRequirement,
    /// File synchronization requested when sealing the staged file.
    pub file_sync: crate::policy::LocalFileSyncRequirement,
    /// Explicitly permits exporting a Rooted resource's read-only handle.
    /// Defaults to false; no namespace or host path is exposed.
    pub export_read_handle: bool,
}

impl Default for LocalStageOptions {
    fn default() -> Self {
        Self {
            max_content_bytes: 64 * 1024 * 1024,
            disk_reservation_bytes: 64 * 1024 * 1024,
            privacy: crate::policy::LocalPrivacyRequirement::Required,
            file_sync: crate::policy::LocalFileSyncRequirement::Required,
            export_read_handle: false,
        }
    }
}

/// Managed resource identity shared by stage, helper, and export wrappers.
pub struct LocalReadResource {
    reference: ResourceRef<LocalTempResource>,
    export_read_handle: bool,
}

/// A clonable lease. Each clone keeps the same single registered resource
/// alive.
#[derive(Clone)]
pub struct LocalReadLease {
    resource: Arc<LocalReadResource>,
}

impl LocalReadLease {
    /// Builds a lease around the same registered temp resource, without
    /// creating a new resource entry.
    pub(crate) fn from_temp_access(access: &LocalTempFileAccess, export_rooted: bool) -> Self {
        Self {
            resource: Arc::new(LocalReadResource {
                reference: access.clone_access().clone_resource_ref(),
                export_read_handle: export_rooted,
            }),
        }
    }

    /// Returns the original registered resource identity.
    pub fn resource_id(&self) -> qubit_operation::ResourceId {
        self.resource.reference.id()
    }

    /// Exports a clone of the already-open sealed handle and retains this
    /// lease. Rooted exports require the explicit option supplied at
    /// staging time.
    pub async fn export_read_handle(&self, executor: &LocalIoExecutor) -> Result<LocalReadExport, LocalStageFailure> {
        let reference = self.resource.reference.clone();
        let allow_rooted = self.resource.export_read_handle;
        let result = enqueue_resource_command(executor, reference, move |native| match native {
            LocalTempNative::Sealed(file) => {
                if file.native_path().is_none() && !allow_rooted {
                    return Err(LocalStageFailure::RootedExportDenied);
                }
                file.try_clone_readonly_file().map_err(LocalStageFailure::Local)
            }
            _ => Err(LocalStageFailure::NotSealed),
        })
        .map_err(command_error)?
        .await
        .map_err(command_error)?;
        Ok(LocalReadExport {
            file: result?,
            lease: self.clone(),
        })
    }
}

/// Read-only file handle and the lease that must accompany its use.
/// Any additional `File::try_clone` used by a helper must be paired with an
/// additional cloned lease for the entire duration of that helper's use.
pub struct LocalReadExport {
    /// Read-only handle cloned from the managed sealed file.
    pub file: File,
    /// Keeps the managed owner and cleanup responsibility alive.
    pub lease: LocalReadLease,
}

/// Typed operation returned by [`stage_bytes`].
pub type LocalStageOperation = AsyncOperationHandle<LocalReadLease, LocalStageFailure>;

/// Failure after a stage operation has been accepted.
#[derive(Debug)]
pub enum LocalStageFailure {
    Capacity,
    Child(LocalCreateFailure),
    ChildOutcome,
    Command(String),
    Local(crate::LocalFileError),
    Io(std::io::Error),
    Seal {
        cause: crate::LocalFileError,
        access_state: crate::outcome::LocalTempAccessState,
    },
    ContentTooLarge,
    Stopped,
    RootedExportDenied,
    NotSealed,
    Retained {
        reason: qubit_operation::ResourceManageErrorKind,
        resource_id: qubit_operation::ResourceId,
    },
}

/// Rejection before stage admission, preserving the original owned allocation.
#[derive(Debug)]
pub enum LocalStageSubmitError {
    Budget {
        bytes: Vec<u8>,
    },
    Admission {
        reason: qubit_operation::AdmissionError,
        bytes: Vec<u8>,
    },
}

impl LocalStageSubmitError {
    /// Returns the original payload after pre-admission rejection.
    pub fn into_bytes(self) -> Vec<u8> {
        match self {
            Self::Budget { bytes } | Self::Admission { bytes, .. } => bytes,
        }
    }
}

/// Stages owned bytes, seals the managed temporary file, and returns a lease.
/// The owned byte capacity is reserved before admission and returned on any
/// pre-admission rejection.
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
            child_operation,
            bytes,
            stage,
            owned,
            disk,
            temp_options,
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

async fn stage_bytes_in_scope(
    filesystem: crate::LocalFileSystem,
    executor: LocalIoExecutor,
    scope: qubit_operation::OperationScope,
    operation: OperationOptions,
    bytes: Vec<u8>,
    stage: LocalStageOptions,
    owned: Permit,
    disk: super::internal::permits::DiskPermit,
    temp_options: crate::options::LocalTempFileOptions,
) -> Result<LocalReadLease, LocalStageFailure> {
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
    crate::test_support::hit_test_gate("lf6-stage-bytes-after-write");
    if context.stop_reason().is_some() {
        return Err(LocalStageFailure::Stopped);
    }
    drop(owned);
    seal_access(access, executor, stage, context).await
}

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

async fn seal_access(
    access: LocalTempFileAccess,
    executor: LocalIoExecutor,
    stage: LocalStageOptions,
    context: qubit_operation::OperationContext,
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
            #[cfg(test)]
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
            crate::test_support::hit_test_gate("lf6-stage-bytes-after-native-seal");
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
        seal_access(access, task_executor, stage, context).await
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

/// Failure to admit a stream stage while preserving its owned input.
pub enum LocalStageStreamSubmitError<I> {
    Budget {
        input: I,
    },
    Admission {
        reason: qubit_operation::AdmissionError,
        input: I,
    },
}

impl<I> std::fmt::Debug for LocalStageStreamSubmitError<I> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Budget { .. } => formatter.write_str("LocalStageStreamSubmitError::Budget { input: <owned> }"),
            Self::Admission { reason, .. } => formatter
                .debug_struct("LocalStageStreamSubmitError::Admission")
                .field("reason", reason)
                .field("input", &"<owned>")
                .finish(),
        }
    }
}

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
    use std::sync::atomic::AtomicUsize;
    use std::task::Context;
    use std::task::Poll;
    use std::task::Wake;
    use std::task::Waker;

    use super::*;

    pub(super) static SEAL_ATTEMPTS: AtomicUsize = AtomicUsize::new(0);

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
    fn lease_from_temp_access_reuses_the_registered_resource() {
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

    #[cfg(feature = "test-support")]
    #[test]
    fn stop_after_queued_write_prevents_the_following_seal_command() {
        let manager =
            qubit_operation::OperationManager::new(qubit_operation::ManagerOptions::default()).expect("manager");
        let executor = LocalIoExecutor::new(manager.clone(), crate::operation::LocalExecutionConfig::default())
            .expect("local executor");
        let filesystem = crate::LocalFileSystem::host().expect("filesystem");
        let gate = crate::test_support::install_test_gate("lf6-stage-bytes-after-write").expect("write boundary gate");
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
    fn stop_after_native_seal_keeps_the_sealed_owner_for_managed_cleanup() {
        let manager =
            qubit_operation::OperationManager::new(qubit_operation::ManagerOptions::default()).expect("manager");
        let executor = LocalIoExecutor::new(manager.clone(), crate::operation::LocalExecutionConfig::default())
            .expect("local executor");
        let filesystem = crate::LocalFileSystem::host().expect("filesystem");
        let gate = crate::test_support::install_test_gate("lf6-stage-bytes-after-native-seal").expect("post-seal gate");
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
