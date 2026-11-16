//! Resource-managed temporary files for asynchronous local operations.

use std::future::Future;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Waker};

use qubit_operation::{OperationOptions, ResourceCapacityError, ResourceId, ResourceRef};

use super::executor::LocalIoExecutor;
#[path = "internal/local_cleanup.rs"]
mod local_cleanup;
use local_cleanup::LocalCleanup;

/// A thread-safe control port owning a native temporary file and its permits.
pub struct LocalTempResource {
    pub(crate) native: Mutex<Option<LocalTempNative>>,
    _disk: Mutex<Option<super::internal::permits::DiskPermit>>,
    _handle: Mutex<Option<super::internal::permits::Permit>>,
    pub(crate) cleanup_permit: Mutex<Option<super::internal::permits::Permit>>,
    pub(crate) id: Arc<Mutex<Option<ResourceId>>>,
}

pub(crate) enum LocalTempNative {
    File(crate::LocalTempFile),
    Directory(crate::LocalTempDirectory),
    Recovery(crate::LocalTempRecovery),
}

impl LocalTempResource {
    fn new(
        native: LocalTempNative,
        disk: super::internal::permits::DiskPermit,
        handle: super::internal::permits::Permit,
        cleanup_permit: super::internal::permits::Permit,
    ) -> Self {
        Self {
            native: Mutex::new(Some(native)),
            _disk: Mutex::new(Some(disk)),
            _handle: Mutex::new(Some(handle)),
            cleanup_permit: Mutex::new(Some(cleanup_permit)),
            id: Arc::new(Mutex::new(None)),
        }
    }
}

/// Typed result of an asynchronous temporary-file creation request.
pub type LocalTempCreateOperation =
    qubit_operation::AsyncOperationHandle<LocalTempFileAccess, LocalCreateFailure>;

/// Public read-only access token for a managed temporary file.
pub struct LocalTempFileAccess {
    resource: ResourceRef<LocalTempResource>,
}
impl LocalTempFileAccess {
    /// Returns the stable public resource identity.
    pub fn resource_id(&self) -> ResourceId {
        self.resource.id()
    }
}

/// Creation failure, including the resource identity when cleanup was retained.
#[derive(Debug)]
pub enum LocalCreateFailure {
    /// Scope capacity was unavailable before native creation.
    Reservation(ResourceCapacityError),
    /// Existing synchronous temp creation failed.
    Local(crate::LocalFileError),
    /// Native creation was partial and its cleanup owner was retained.
    PartialRetained {
        error: crate::LocalFileError,
        resource_id: ResourceId,
    },
    /// Commit was rejected and the original resource was retained for cleanup.
    Retained {
        reason: qubit_operation::ResourceManageErrorKind,
        resource_id: ResourceId,
    },
    /// Executor capacity was unavailable before native creation.
    Capacity,
}

/// Admission failure returned before a temporary-resource operation is accepted.
#[derive(Debug)]
pub struct LocalTempSubmitError<R> {
    /// Upstream operation admission rejection.
    pub reason: qubit_operation::AdmissionError,
    request: R,
}

impl<R> LocalTempSubmitError<R> {
    /// Returns the original options supplied for the rejected request.
    pub fn into_request(self) -> R {
        self.request
    }
}

/// Create a temporary file without performing blocking filesystem work on the caller executor.
pub fn submit_temp_file(
    filesystem: crate::LocalFileSystem,
    executor: LocalIoExecutor,
    manager: &qubit_operation::AsyncOperationManager,
    options: OperationOptions,
    temp_options: crate::options::LocalTempFileOptions,
) -> Result<LocalTempCreateOperation, LocalTempSubmitError<crate::options::LocalTempFileOptions>> {
    let rejected_request = temp_options.clone();
    let task_executor = executor.clone();
    let result = manager.try_submit(options, move |scope| async move {
        let reservation = scope
            .reserve_resource()
            .map_err(LocalCreateFailure::Reservation)?;
        let Some(handle) = task_executor.reserve_handle() else {
            return Err(LocalCreateFailure::Capacity);
        };
        // Keep one byte of domain disk capacity attached to the resource even
        // when the newly-created file is empty. Future staged writes may grow
        // this reservation before accepting any data.
        let Some(disk) = task_executor.reserve_disk(1) else {
            return Err(LocalCreateFailure::Capacity);
        };
        let Some(cleanup_permit) = task_executor.reserve_cleanup() else {
            return Err(LocalCreateFailure::Capacity);
        };
        let (tx, rx) = async_result();
        let fs = filesystem.clone();
        if task_executor
            .enqueue_task(move || {
                let _ = tx.send(fs.create_temp_file_with_options(&temp_options));
            })
            .is_err()
        {
            return Err(LocalCreateFailure::Capacity);
        }
        let file = match rx.await {
            Ok(file) => file,
            Err(error) => {
                let (primary, recovery) = error.into_temp_recovery_parts();
                if let Some(recovery) = recovery {
                    return Err(retain_partial(
                        reservation,
                        primary,
                        recovery,
                        task_executor,
                        disk,
                        handle,
                        cleanup_permit,
                    ));
                }
                return Err(LocalCreateFailure::Local(primary));
            }
        };
        let resource =
            LocalTempResource::new(LocalTempNative::File(file), disk, handle, cleanup_permit);
        let id_slot = Arc::clone(&resource.id);
        let cleanup = LocalCleanup {
            executor: task_executor,
        };
        match reservation.commit_with(resource, cleanup) {
            Ok(reference) => {
                *id_slot.lock().unwrap_or_else(|e| e.into_inner()) = Some(reference.id());
                Ok(LocalTempFileAccess {
                    resource: reference,
                })
            }
            Err(error) => {
                let (reason, owner, cleanup, reservation) = error.into_parts();
                let id_slot = Arc::clone(&owner.id);
                let mut id = id_slot.lock().unwrap_or_else(|e| e.into_inner());
                let resource_id = reservation.retain_with_for_cleanup(owner, cleanup);
                *id = Some(resource_id);
                Err(LocalCreateFailure::Retained {
                    reason,
                    resource_id,
                })
            }
        }
    });
    result.map_err(|error| {
        let (reason, _) = error.into_parts();
        LocalTempSubmitError {
            reason,
            request: rejected_request,
        }
    })
}

/// Creates a managed temporary directory on the local blocking worker queue.
pub fn submit_temp_directory(
    filesystem: crate::LocalFileSystem,
    executor: LocalIoExecutor,
    manager: &qubit_operation::AsyncOperationManager,
    options: OperationOptions,
    temp_options: crate::options::LocalTempDirectoryOptions,
) -> Result<LocalTempCreateOperation, LocalTempSubmitError<crate::options::LocalTempDirectoryOptions>>
{
    let rejected_request = temp_options.clone();
    let task_executor = executor.clone();
    let result = manager.try_submit(options, move |scope| async move {
        let reservation = scope
            .reserve_resource()
            .map_err(LocalCreateFailure::Reservation)?;
        let Some(handle) = task_executor.reserve_handle() else {
            return Err(LocalCreateFailure::Capacity);
        };
        // Directory creation also retains a minimum disk reservation for its
        // lifetime, including while cleanup recovery owns it.
        let Some(disk) = task_executor.reserve_disk(1) else {
            return Err(LocalCreateFailure::Capacity);
        };
        let Some(cleanup_permit) = task_executor.reserve_cleanup() else {
            return Err(LocalCreateFailure::Capacity);
        };
        let (tx, rx) = async_result();
        let fs = filesystem.clone();
        if task_executor
            .enqueue_task(move || {
                let _ = tx.send(fs.create_temp_directory_with_options(&temp_options));
            })
            .is_err()
        {
            return Err(LocalCreateFailure::Capacity);
        }
        let directory = match rx.await {
            Ok(directory) => directory,
            Err(error) => {
                let (primary, recovery) = error.into_temp_recovery_parts();
                if let Some(recovery) = recovery {
                    return Err(retain_partial(
                        reservation,
                        primary,
                        recovery,
                        task_executor,
                        disk,
                        handle,
                        cleanup_permit,
                    ));
                }
                return Err(LocalCreateFailure::Local(primary));
            }
        };
        let resource = LocalTempResource::new(
            LocalTempNative::Directory(directory),
            disk,
            handle,
            cleanup_permit,
        );
        let id_slot = Arc::clone(&resource.id);
        let cleanup = LocalCleanup {
            executor: task_executor,
        };
        match reservation.commit_with(resource, cleanup) {
            Ok(reference) => {
                *id_slot.lock().unwrap_or_else(|e| e.into_inner()) = Some(reference.id());
                Ok(LocalTempFileAccess {
                    resource: reference,
                })
            }
            Err(error) => {
                let (reason, owner, cleanup, reservation) = error.into_parts();
                let id_slot = Arc::clone(&owner.id);
                let mut id = id_slot.lock().unwrap_or_else(|e| e.into_inner());
                let resource_id = reservation.retain_with_for_cleanup(owner, cleanup);
                *id = Some(resource_id);
                Err(LocalCreateFailure::Retained {
                    reason,
                    resource_id,
                })
            }
        }
    });
    result.map_err(|error| {
        let (reason, _) = error.into_parts();
        LocalTempSubmitError {
            reason,
            request: rejected_request,
        }
    })
}

fn retain_partial(
    reservation: qubit_operation::ResourceReservation,
    error: crate::LocalFileError,
    recovery: crate::LocalTempRecovery,
    executor: LocalIoExecutor,
    disk: super::internal::permits::DiskPermit,
    handle: super::internal::permits::Permit,
    cleanup_permit: super::internal::permits::Permit,
) -> LocalCreateFailure {
    let owner = LocalTempResource::new(
        LocalTempNative::Recovery(recovery),
        disk,
        handle,
        cleanup_permit,
    );
    let id_slot = Arc::clone(&owner.id);
    let cleanup = LocalCleanup { executor };
    match reservation.commit_with(owner, cleanup) {
        Ok(reference) => {
            let resource_id = reference.id();
            *id_slot.lock().unwrap_or_else(|e| e.into_inner()) = Some(resource_id);
            drop(reference);
            LocalCreateFailure::PartialRetained { error, resource_id }
        }
        Err(commit_error) => {
            let (_reason, owner, cleanup, reservation) = commit_error.into_parts();
            let id_slot = Arc::clone(&owner.id);
            let mut id = id_slot.lock().unwrap_or_else(|e| e.into_inner());
            let resource_id = reservation.retain_with_for_cleanup(owner, cleanup);
            *id = Some(resource_id);
            LocalCreateFailure::PartialRetained { error, resource_id }
        }
    }
}

struct AsyncResult<T> {
    value: Mutex<Option<T>>,
    waker: Mutex<Option<Waker>>,
}
fn async_result<T: Send + 'static>() -> (AsyncSender<T>, AsyncReceiver<T>) {
    let state = Arc::new(AsyncResult {
        value: Mutex::new(None),
        waker: Mutex::new(None),
    });
    (AsyncSender(Arc::clone(&state)), AsyncReceiver(state))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::LocalFileOperation;
    use crate::LocalFileSystem;
    use crate::options::LocalTempFileOptions;
    use qubit_operation::{ManagerOptions, OperationManager, StopReason};

    #[test]
    fn partial_creation_recovery_is_retained_when_stopped_commit_is_rejected() {
        let parent = tempfile::tempdir().expect("temp parent");
        let manager = OperationManager::new(ManagerOptions::default()).expect("manager");
        let executor = LocalIoExecutor::new(
            manager.clone(),
            super::super::LocalExecutionConfig::default(),
        )
        .expect("local executor");
        let filesystem = LocalFileSystem::host().expect("filesystem");
        let parent_path = parent.path().to_path_buf();
        let id = Arc::new(Mutex::new(None));
        let completion = manager
            .run_and_settle(qubit_operation::OperationOptions::default(), {
                let id = Arc::clone(&id);
                move |scope| {
                    let reservation = scope
                        .reserve_resource()
                        .expect("reservation before native creation");
                    let file = filesystem
                        .create_temp_file_with_options(
                            &LocalTempFileOptions::new().with_parent(&parent_path),
                        )
                        .expect("construct partial native owner");
                    let recovery = crate::LocalTempRecovery::from_file(file);
                    let handle = executor.reserve_handle().expect("handle permit");
                    let disk = executor.reserve_disk(1).expect("disk permit");
                    let cleanup_permit = executor.reserve_cleanup().expect("cleanup slot");
                    let _stop_status = scope.request_stop(StopReason::UserCancelled);
                    let error = crate::LocalFileError::from_io(
                        LocalFileOperation::CreateTempFile,
                        None,
                        None,
                        std::io::Error::other("injected partial create failure"),
                    );
                    let failure = super::retain_partial(
                        reservation,
                        error,
                        recovery,
                        executor,
                        disk,
                        handle,
                        cleanup_permit,
                    );
                    match failure {
                        LocalCreateFailure::PartialRetained { resource_id, .. } => {
                            *id.lock().expect("id lock") = Some(resource_id);
                            Ok::<(), ()>(())
                        }
                        other => panic!("partial owner was not retained: {other:?}"),
                    }
                }
            })
            .expect("operation completion");
        assert!(id.lock().expect("id lock").is_some());
        assert_eq!(completion.settlement.recovery_count, 0);
        assert_eq!(
            std::fs::read_dir(parent.path())
                .expect("read parent")
                .count(),
            0
        );
    }
}
struct AsyncSender<T>(Arc<AsyncResult<T>>);
impl<T> AsyncSender<T> {
    fn send(self, value: T) -> Result<(), T> {
        *self.0.value.lock().unwrap_or_else(|e| e.into_inner()) = Some(value);
        if let Some(waker) = self
            .0
            .waker
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take()
        {
            waker.wake();
        }
        Ok(())
    }
}
struct AsyncReceiver<T>(Arc<AsyncResult<T>>);
impl<T> Future for AsyncReceiver<T> {
    type Output = T;
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<T> {
        if let Some(value) = self
            .0
            .value
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take()
        {
            return Poll::Ready(value);
        }
        *self.0.waker.lock().unwrap_or_else(|e| e.into_inner()) = Some(cx.waker().clone());
        if let Some(value) = self
            .0
            .value
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take()
        {
            Poll::Ready(value)
        } else {
            Poll::Pending
        }
    }
}
