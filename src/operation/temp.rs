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

pub(super) fn safe_clone_waker(waker: &Waker) -> Option<Waker> {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| waker.clone())) {
        Ok(clone) => Some(clone),
        Err(payload) => {
            std::mem::forget(payload);
            None
        }
    }
}

pub(super) fn safe_drop_waker(waker: Option<Waker>) {
    if let Err(payload) = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| drop(waker))) {
        std::mem::forget(payload);
    }
}

fn safe_wake(waker: Waker) {
    if let Err(payload) = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| waker.wake())) {
        std::mem::forget(payload);
    }
}

/// A thread-safe control port owning a native temporary file and its permits.
pub struct LocalTempResource {
    pub(crate) native: Mutex<Option<LocalTempNative>>,
    _disk: Mutex<Vec<super::internal::permits::DiskPermit>>,
    _handle: Mutex<Option<super::internal::permits::Permit>>,
    pub(crate) cleanup_permit: Mutex<Option<super::internal::permits::Permit>>,
    pub(crate) id: Arc<Mutex<Option<ResourceId>>>,
}

pub(crate) enum LocalTempNative {
    File(crate::LocalTempFile),
    Sealed(crate::LocalSealedTempFile),
    Directory(crate::LocalTempDirectory),
    Recovery(crate::LocalTempRecovery),
    /// A consuming command panicked after taking its owner; cleanup cannot
    /// claim that the native resource was released.
    Unconfirmed,
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
            _disk: Mutex::new(vec![disk]),
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

    /// Clones this access token without registering another resource.
    pub(crate) fn clone_access(&self) -> Self {
        Self {
            resource: self.resource.clone(),
        }
    }

    /// Retains the same registered owner for a related operation/helper.
    pub(crate) fn clone_resource_ref(&self) -> ResourceRef<LocalTempResource> {
        self.resource.clone()
    }

    /// Retains an additional disk reservation on the registered native owner.
    pub(crate) fn retain_disk_permit(&self, permit: super::internal::permits::DiskPermit) {
        self.resource
            ._disk
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(permit);
    }

    /// Queues one serialized native-resource command on the local I/O worker.
    /// The cloned resource reference remains alive until the command returns.
    pub(crate) fn enqueue_native_command<T, F>(
        &self,
        executor: &LocalIoExecutor,
        command: F,
    ) -> Result<LocalTempCommandReceiver<T>, LocalTempCommandError>
    where
        T: Send + 'static,
        F: FnOnce(&mut LocalTempNative) -> T + Send + 'static,
    {
        let resource = self.clone_resource_ref();
        let (sender, receiver) = async_result();
        executor
            .enqueue_task(move || {
                let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    let mut native = resource.native.lock().unwrap_or_else(|e| e.into_inner());
                    let Some(native) = native.as_mut() else {
                        return Err(LocalTempCommandError::ResourceUnavailable);
                    };
                    Ok(command(native))
                }))
                .unwrap_or(Err(LocalTempCommandError::Panicked));
                let _ = sender.send(result);
                drop(resource);
            })
            .map_err(|()| LocalTempCommandError::Capacity)?;
        Ok(LocalTempCommandReceiver(receiver))
    }

    /// Queues an internal consuming command. Callbacks are crate-controlled,
    /// never caller-supplied; ordinary error paths must return the owner. A
    /// callback must not panic after taking ownership of the native value.
    pub(crate) fn enqueue_native_transition<T, E, F>(
        &self,
        executor: &LocalIoExecutor,
        command: F,
    ) -> Result<LocalTempCommandReceiver<Result<T, E>>, LocalTempCommandError>
    where
        T: Send + 'static,
        E: Send + 'static,
        F: FnOnce(LocalTempNative) -> Result<(Option<LocalTempNative>, T), (LocalTempNative, E)>
            + Send
            + 'static,
    {
        let resource = self.clone_resource_ref();
        let (sender, receiver) = async_result();
        executor
            .enqueue_task(move || {
                let mut native = resource.native.lock().unwrap_or_else(|e| e.into_inner());
                let outcome =
                    match native.take() {
                        None => Err(LocalTempCommandError::ResourceUnavailable),
                        Some(owner) => {
                            match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                                command(owner)
                            })) {
                                Ok(Ok((owner, value))) => {
                                    *native = owner;
                                    Ok(Ok(value))
                                }
                                Ok(Err((owner, error))) => {
                                    *native = Some(owner);
                                    Ok(Err(error))
                                }
                                Err(payload) => {
                                    if let Err(payload) = std::panic::catch_unwind(
                                        std::panic::AssertUnwindSafe(|| drop(payload)),
                                    ) {
                                        std::mem::forget(payload);
                                    }
                                    *native = Some(LocalTempNative::Unconfirmed);
                                    Err(LocalTempCommandError::Panicked)
                                }
                            }
                        }
                    };
                drop(native);
                let _ = sender.send(outcome);
                drop(resource);
            })
            .map_err(|()| LocalTempCommandError::Capacity)?;
        Ok(LocalTempCommandReceiver(receiver))
    }
}

pub(super) fn enqueue_resource_command<T, F>(
    executor: &LocalIoExecutor,
    resource: ResourceRef<LocalTempResource>,
    command: F,
) -> Result<LocalTempCommandReceiver<T>, LocalTempCommandError>
where
    T: Send + 'static,
    F: FnOnce(&mut LocalTempNative) -> T + Send + 'static,
{
    let (sender, receiver) = async_result();
    executor
        .enqueue_task(move || {
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let mut native = resource.native.lock().unwrap_or_else(|e| e.into_inner());
                let Some(native) = native.as_mut() else {
                    return Err(LocalTempCommandError::ResourceUnavailable);
                };
                Ok(command(native))
            }))
            .unwrap_or(Err(LocalTempCommandError::Panicked));
            let _ = sender.send(result);
            drop(resource);
        })
        .map_err(|()| LocalTempCommandError::Capacity)?;
    Ok(LocalTempCommandReceiver(receiver))
}

/// Failure to schedule or execute a managed native-resource command.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) enum LocalTempCommandError {
    Capacity,
    ResourceUnavailable,
    Panicked,
}

/// Future receiving a local worker command result without doing native I/O in poll.
pub(crate) struct LocalTempCommandReceiver<T>(AsyncReceiver<Result<T, LocalTempCommandError>>);

impl<T> Future for LocalTempCommandReceiver<T> {
    type Output = Result<T, LocalTempCommandError>;

    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        Pin::new(&mut self.0).poll(cx)
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
    fn cloning_access_keeps_the_same_registered_resource_until_last_clone_drops() {
        let parent = tempfile::tempdir().expect("temp parent");
        let manager = OperationManager::new(ManagerOptions::default()).expect("manager");
        let executor = LocalIoExecutor::new(
            manager.clone(),
            super::super::LocalExecutionConfig::default(),
        )
        .expect("local executor");
        let filesystem = LocalFileSystem::host().expect("filesystem");
        let parent_path = parent.path().to_path_buf();
        let completion = manager
            .run_and_settle(qubit_operation::OperationOptions::default(), move |scope| {
                let reservation = scope.reserve_resource().expect("resource reservation");
                let file = filesystem
                    .create_temp_file_with_options(
                        &LocalTempFileOptions::new().with_parent(&parent_path),
                    )
                    .expect("temporary file");
                let owner = LocalTempResource::new(
                    LocalTempNative::File(file),
                    executor.reserve_disk(1).expect("disk permit"),
                    executor.reserve_handle().expect("handle permit"),
                    executor.reserve_cleanup().expect("cleanup permit"),
                );
                let id_slot = Arc::clone(&owner.id);
                let reference = reservation
                    .commit_with(owner, LocalCleanup { executor })
                    .expect("managed temp resource");
                *id_slot.lock().unwrap_or_else(|e| e.into_inner()) = Some(reference.id());
                let original = LocalTempFileAccess {
                    resource: reference,
                };
                let clone = original.clone_access();
                assert_eq!(original.resource_id(), clone.resource_id());

                drop(original);
                assert_eq!(std::fs::read_dir(&parent_path).unwrap().count(), 1);
                drop(clone);
                Ok::<(), ()>(())
            })
            .expect("operation completed");
        assert_eq!(completion.settlement.recovery_count, 0);
        assert_eq!(std::fs::read_dir(parent.path()).unwrap().count(), 0);
    }

    #[test]
    fn panicked_consuming_transition_cannot_settle_as_released() {
        let parent = tempfile::tempdir().expect("temp parent");
        let manager = OperationManager::new(ManagerOptions::default()).expect("manager");
        let executor = LocalIoExecutor::new(
            manager.clone(),
            super::super::LocalExecutionConfig::default(),
        )
        .expect("local executor");
        let filesystem = LocalFileSystem::host().expect("filesystem");
        let parent_path = parent.path().to_path_buf();
        let completion = manager
            .run_and_settle(qubit_operation::OperationOptions::default(), move |scope| {
                let reservation = scope.reserve_resource().expect("resource reservation");
                let file = filesystem
                    .create_temp_file_with_options(
                        &LocalTempFileOptions::new().with_parent(&parent_path),
                    )
                    .expect("temporary file");
                let owner = LocalTempResource::new(
                    LocalTempNative::File(file),
                    executor.reserve_disk(1).expect("disk permit"),
                    executor.reserve_handle().expect("handle permit"),
                    executor.reserve_cleanup().expect("cleanup permit"),
                );
                let id_slot = Arc::clone(&owner.id);
                let reference = reservation
                    .commit_with(
                        owner,
                        LocalCleanup {
                            executor: executor.clone(),
                        },
                    )
                    .expect("managed temp resource");
                *id_slot.lock().unwrap_or_else(|e| e.into_inner()) = Some(reference.id());
                let access = LocalTempFileAccess {
                    resource: reference,
                };
                let receiver = access
                    .enqueue_native_transition(
                        &executor,
                        |_owner| -> Result<(Option<LocalTempNative>, ()), (LocalTempNative, ())> {
                            panic!("injected transition panic")
                        },
                    )
                    .expect("command queued");
                assert!(matches!(
                    block_on(receiver),
                    Err(LocalTempCommandError::Panicked)
                ));
                drop(access);
                Ok::<(), ()>(())
            })
            .expect("operation completed");
        assert!(completion.settlement.recovery_count > 0);
        assert!(!completion.settlement.cleanup_failures.is_empty());
    }

    fn block_on<F: std::future::Future>(future: F) -> F::Output {
        let waker = Waker::from(Arc::new(ThreadWake(std::thread::current())));
        let mut context = Context::from_waker(&waker);
        let mut future = Box::pin(future);
        loop {
            if let Poll::Ready(output) = future.as_mut().poll(&mut context) {
                return output;
            }
            std::thread::park();
        }
    }

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

    struct WakePanics;
    struct ThreadWake(std::thread::Thread);
    impl std::task::Wake for ThreadWake {
        fn wake(self: Arc<Self>) {
            self.0.unpark();
        }
        fn wake_by_ref(self: &Arc<Self>) {
            self.0.unpark();
        }
    }
    impl std::task::Wake for WakePanics {
        fn wake(self: Arc<Self>) {
            panic!("custom wake panic");
        }
    }

    #[test]
    fn async_sender_wake_panic_does_not_hide_the_sent_result() {
        let (sender, mut receiver) = async_result();
        *receiver.0.waker.lock().expect("waker lock") = Some(Waker::from(Arc::new(WakePanics)));
        assert!(sender.send(42_u8).is_ok());
        let waker = Waker::from(Arc::new(ThreadWake(std::thread::current())));
        let mut context = Context::from_waker(&waker);
        assert!(matches!(
            Pin::new(&mut receiver).poll(&mut context),
            Poll::Ready(42)
        ));
    }
}
struct AsyncSender<T>(Arc<AsyncResult<T>>);
impl<T> AsyncSender<T> {
    fn send(self, value: T) -> Result<(), T> {
        *self.0.value.lock().unwrap_or_else(|e| e.into_inner()) = Some(value);
        let waker = self
            .0
            .waker
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .take();
        if let Some(waker) = waker {
            safe_wake(waker);
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
        let Some(new_waker) = safe_clone_waker(cx.waker()) else {
            return self
                .0
                .value
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .take()
                .map_or(Poll::Pending, Poll::Ready);
        };
        let old_waker = self
            .0
            .waker
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .replace(new_waker);
        safe_drop_waker(old_waker);
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
