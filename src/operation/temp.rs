// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

//! Resource-managed temporary files for asynchronous local operations.

use std::sync::Arc;
use std::task::Waker;

use qubit_operation::OperationOptions;
use qubit_operation::ResourceRef;

use super::executor::LocalIoExecutor;
#[path = "internal/local_cleanup.rs"]
mod local_cleanup;
pub(crate) use local_cleanup::LocalCleanup;

mod async_receiver;
mod async_result;
mod async_sender;
mod local_create_failure;
mod local_temp_cleanup_anchor;
mod local_temp_command_error;
mod local_temp_command_receiver;
mod local_temp_file_access;
mod local_temp_native;
mod local_temp_resource;
mod local_temp_submit_error;

use async_receiver::AsyncReceiver;
pub(super) use async_result::AsyncResult;
use async_result::async_result;
use async_sender::AsyncSender;
pub use local_create_failure::LocalCreateFailure;
pub(crate) use local_temp_cleanup_anchor::LocalTempCleanupAnchor;
pub(crate) use local_temp_command_error::LocalTempCommandError;
pub(crate) use local_temp_command_receiver::LocalTempCommandReceiver;
pub use local_temp_file_access::LocalTempFileAccess;
pub(crate) use local_temp_native::LocalTempNative;
pub use local_temp_resource::LocalTempResource;
pub use local_temp_submit_error::LocalTempSubmitError;

/// Clones a task waker without allowing a hostile implementation to unwind.
///
/// # Parameters
///
/// * `waker` — task notification handle to retain for a later poll.
pub(super) fn safe_clone_waker(waker: &Waker) -> Option<Waker> {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| waker.clone())) {
        Ok(clone) => Some(clone),
        Err(payload) => {
            std::mem::forget(payload);
            None
        }
    }
}

/// Drops a stored waker while containing panics from custom waker data.
///
/// # Parameters
///
/// * `waker` — optional task notification handle being replaced or cleared.
pub(super) fn safe_drop_waker(waker: Option<Waker>) {
    if let Err(payload) = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| drop(waker))) {
        std::mem::forget(payload);
    }
}

/// Wakes a task while preventing custom waker panics from escaping the worker.
///
/// # Parameters
///
/// * `waker` — task notification handle taken from a pending receiver.
fn safe_wake(waker: Waker) {
    if let Err(payload) = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| waker.wake())) {
        std::mem::forget(payload);
    }
}

/// Typed result of an asynchronous temporary-file creation request.
pub type LocalTempCreateOperation = qubit_operation::AsyncOperationHandle<LocalTempFileAccess, LocalCreateFailure>;

/// Arms the native owner's fallback cleanup before it is restored to the
/// manager.
///
/// # Parameters
///
/// * `native` — owner whose destructor cleanup is enabled for panic safety.
fn arm_native_cleanup_on_drop(native: &mut LocalTempNative) {
    match native {
        LocalTempNative::File(file) => file.arm_cleanup_on_drop(),
        LocalTempNative::Sealed(file) => file.arm_cleanup_on_drop(),
        LocalTempNative::Directory(directory) => directory.arm_cleanup_on_drop(),
        LocalTempNative::Recovery(recovery) => recovery.arm_cleanup_on_drop(),
        _ => {}
    }
}

/// Queues one serialized access command and retains the resource through
/// completion.
///
/// # Type Parameters
///
/// * `T` — value produced by the command closure.
/// * `F` — one-shot action executed while the native owner is locked.
///
/// # Parameters
///
/// * `executor` — bounded worker that serializes native operations.
/// * `resource` — registered owner retained until the queued command returns.
/// * `command` — crate-controlled operation applied to the native owner.
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

/// Creates a managed temporary file without blocking the caller's executor.
/// Capacity and ownership transfer happen before the native creation task runs.
///
/// # Parameters
///
/// * `filesystem` — local filesystem that creates and later cleans the file.
/// * `executor` — bounded worker that performs blocking filesystem operations.
/// * `manager` — asynchronous operation manager handling admission and cleanup.
/// * `options` — cancellation and resource options for the operation.
/// * `temp_options` — filesystem-specific privacy and creation options.
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
        let reservation = scope.reserve_resource().map_err(LocalCreateFailure::Reservation)?;
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
        let resource = LocalTempResource::new(LocalTempNative::File(file), disk, handle, cleanup_permit);
        let id_slot = Arc::clone(&resource.id);
        let cleanup = LocalCleanup {
            executor: task_executor,
        };
        match reservation.commit_with(resource, cleanup) {
            Ok(reference) => {
                *id_slot.lock().unwrap_or_else(|e| e.into_inner()) = Some(reference.id());
                Ok(LocalTempFileAccess { resource: reference })
            }
            Err(error) => {
                let (reason, owner, cleanup, reservation) = error.into_parts();
                let id_slot = Arc::clone(&owner.id);
                let mut id = id_slot.lock().unwrap_or_else(|e| e.into_inner());
                let resource_id = reservation.retain_with_for_cleanup(owner, cleanup);
                *id = Some(resource_id);
                Err(LocalCreateFailure::Retained { reason, resource_id })
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

/// Creates a managed temporary directory on the bounded local worker queue.
/// The directory remains registered until its cleanup future releases it.
///
/// # Parameters
///
/// * `filesystem` — local filesystem that creates and later cleans the
///   directory.
/// * `executor` — bounded worker that performs blocking filesystem operations.
/// * `manager` — asynchronous operation manager handling admission and cleanup.
/// * `options` — cancellation and resource options for the operation.
/// * `temp_options` — filesystem-specific privacy and creation options.
pub fn submit_temp_directory(
    filesystem: crate::LocalFileSystem,
    executor: LocalIoExecutor,
    manager: &qubit_operation::AsyncOperationManager,
    options: OperationOptions,
    temp_options: crate::options::LocalTempDirectoryOptions,
) -> Result<LocalTempCreateOperation, LocalTempSubmitError<crate::options::LocalTempDirectoryOptions>> {
    let rejected_request = temp_options.clone();
    let task_executor = executor.clone();
    let result = manager.try_submit(options, move |scope| async move {
        let reservation = scope.reserve_resource().map_err(LocalCreateFailure::Reservation)?;
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
        let resource = LocalTempResource::new(LocalTempNative::Directory(directory), disk, handle, cleanup_permit);
        let id_slot = Arc::clone(&resource.id);
        let cleanup = LocalCleanup {
            executor: task_executor,
        };
        match reservation.commit_with(resource, cleanup) {
            Ok(reference) => {
                *id_slot.lock().unwrap_or_else(|e| e.into_inner()) = Some(reference.id());
                Ok(LocalTempFileAccess { resource: reference })
            }
            Err(error) => {
                let (reason, owner, cleanup, reservation) = error.into_parts();
                let id_slot = Arc::clone(&owner.id);
                let mut id = id_slot.lock().unwrap_or_else(|e| e.into_inner());
                let resource_id = reservation.retain_with_for_cleanup(owner, cleanup);
                *id = Some(resource_id);
                Err(LocalCreateFailure::Retained { reason, resource_id })
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

/// Registers a recovery owner after native creation partially failed.
///
/// This keeps the failed resource and its cleanup reservations visible to the
/// manager rather than dropping a potentially live filesystem object.
///
/// # Parameters
///
/// * `reservation` — resource slot reserved before native creation.
/// * `error` — original creation failure returned to the caller.
/// * `recovery` — native cleanup authority for the partial object.
/// * `executor` — worker responsible for eventual cleanup.
/// * `disk` — disk reservation retained with the owner.
/// * `handle` — handle reservation retained with the owner.
/// * `cleanup_permit` — pre-reserved capacity for cleanup scheduling.
fn retain_partial(
    reservation: qubit_operation::ResourceReservation,
    error: crate::LocalFileError,
    recovery: crate::LocalTempRecovery,
    executor: LocalIoExecutor,
    disk: super::internal::permits::DiskPermit,
    handle: super::internal::permits::Permit,
    cleanup_permit: super::internal::permits::Permit,
) -> LocalCreateFailure {
    let owner = LocalTempResource::new(LocalTempNative::Recovery(recovery), disk, handle, cleanup_permit);
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

#[cfg(test)]
mod tests {
    use std::future::Future;
    use std::pin::Pin;
    use std::sync::Arc;
    use std::sync::Mutex;
    use std::task::Context;
    use std::task::Poll;
    use std::task::RawWaker;
    use std::task::RawWakerVTable;
    use std::task::Waker;

    use qubit_operation::ManagerOptions;
    use qubit_operation::OperationManager;
    use qubit_operation::OperationOutcome;
    use qubit_operation::StopReason;

    use super::LocalCleanup;
    use super::LocalCreateFailure;
    #[cfg(feature = "test-support")]
    use super::LocalTempCleanupAnchor;
    use super::LocalTempCommandError;
    use super::LocalTempFileAccess;
    use super::LocalTempNative;
    use super::LocalTempResource;
    use super::async_result;

    fn panic_on_clone(_: *const ()) -> RawWaker {
        panic!("injected waker clone panic");
    }

    fn panic_on_wake(_: *const ()) {
        panic!("injected waker wake panic");
    }

    fn no_op(_: *const ()) {}

    fn panic_on_drop(_: *const ()) {
        panic!("injected waker drop panic");
    }

    static PANIC_CLONE_VTABLE: RawWakerVTable = RawWakerVTable::new(panic_on_clone, no_op, no_op, no_op);
    static PANIC_WAKE_VTABLE: RawWakerVTable = RawWakerVTable::new(
        |data| RawWaker::new(data, &PANIC_WAKE_VTABLE),
        panic_on_wake,
        no_op,
        no_op,
    );
    static PANIC_DROP_VTABLE: RawWakerVTable = RawWakerVTable::new(
        |data| RawWaker::new(data, &PANIC_DROP_VTABLE),
        no_op,
        no_op,
        panic_on_drop,
    );

    fn raw_waker(vtable: &'static RawWakerVTable) -> Waker {
        // The static vtables use a null data pointer and never dereference it.
        unsafe { Waker::from_raw(RawWaker::new(std::ptr::null(), vtable)) }
    }

    #[test]
    fn test_waker_panics_are_contained_and_failed_clone_leaves_receiver_pending() {
        let clone_panics = raw_waker(&PANIC_CLONE_VTABLE);
        assert!(super::safe_clone_waker(&clone_panics).is_none());

        let (sender, mut receiver) = async_result::<u8>();
        let mut context = Context::from_waker(&clone_panics);
        assert!(Pin::new(&mut receiver).poll(&mut context).is_pending());
        assert!(sender.send(9).is_ok());
        let mut normal_context = Context::from_waker(Waker::noop());
        assert!(matches!(
            Pin::new(&mut receiver).poll(&mut normal_context),
            Poll::Ready(9)
        ));

        assert!(std::panic::catch_unwind(|| super::safe_wake(raw_waker(&PANIC_WAKE_VTABLE))).is_ok());
        assert!(std::panic::catch_unwind(|| super::safe_drop_waker(Some(raw_waker(&PANIC_DROP_VTABLE)))).is_ok());
    }
    use crate::LocalFileOperation;
    use crate::LocalFileSystem;
    use crate::operation::LocalExecutionConfig;
    use crate::operation::LocalIoExecutor;
    use crate::options::LocalTempDirectoryOptions;
    use crate::options::LocalTempFileOptions;
    #[cfg(feature = "test-support")]
    use crate::outcome::LocalTempSourceState;

    #[test]
    fn test_cloning_access_keeps_the_same_registered_resource_until_last_clone_drops() {
        let parent = tempfile::tempdir().expect("temp parent");
        let manager = OperationManager::new(ManagerOptions::default()).expect("manager");
        let executor = LocalIoExecutor::new(manager.clone(), super::super::LocalExecutionConfig::default())
            .expect("local executor");
        let filesystem = LocalFileSystem::host().expect("filesystem");
        let parent_path = parent.path().to_path_buf();
        let completion = manager
            .run_and_settle(qubit_operation::OperationOptions::default(), move |scope| {
                let reservation = scope.reserve_resource().expect("resource reservation");
                let file = filesystem
                    .create_temp_file_with_options(&LocalTempFileOptions::new().with_parent(&parent_path))
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
                let original = LocalTempFileAccess { resource: reference };
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
    fn test_async_temp_file_and_directory_submission_create_managed_resources() {
        let parent = tempfile::tempdir().expect("temp parent");
        let manager = OperationManager::new(ManagerOptions::default()).expect("manager");
        let executor = LocalIoExecutor::new(manager.clone(), LocalExecutionConfig::default()).expect("local executor");
        let filesystem = LocalFileSystem::host().expect("filesystem");
        let async_manager = manager.as_async();

        let mut file_operation = super::submit_temp_file(
            filesystem.clone(),
            executor.clone(),
            &async_manager,
            Default::default(),
            LocalTempFileOptions::new().with_parent(parent.path()),
        )
        .expect("file request accepted");
        let file_access = match block_on(file_operation.wait_outcome()).expect("file outcome") {
            OperationOutcome::Succeeded(access) => access,
            _ => panic!("expected temporary file creation"),
        };
        assert_eq!(std::fs::read_dir(parent.path()).unwrap().count(), 1);
        drop(file_access);
        let file_settlement = block_on(file_operation.wait_settlement());
        assert_eq!(file_settlement.recovery_count, 0);
        assert!(file_settlement.cleanup_failures.is_empty());
        assert_eq!(std::fs::read_dir(parent.path()).unwrap().count(), 0);

        let mut directory_operation = super::submit_temp_directory(
            filesystem,
            executor,
            &async_manager,
            Default::default(),
            LocalTempDirectoryOptions::new().with_parent(parent.path()),
        )
        .expect("directory request accepted");
        let directory_access = match block_on(directory_operation.wait_outcome()).expect("directory outcome") {
            OperationOutcome::Succeeded(access) => access,
            _ => panic!("expected temporary directory creation"),
        };
        assert_eq!(std::fs::read_dir(parent.path()).unwrap().count(), 1);
        drop(directory_access);
        let directory_settlement = block_on(directory_operation.wait_settlement());
        assert_eq!(directory_settlement.recovery_count, 0);
        assert!(directory_settlement.cleanup_failures.is_empty());
        assert_eq!(std::fs::read_dir(parent.path()).unwrap().count(), 0);
    }

    #[test]
    fn test_panicking_managed_native_command_reports_failure_and_cleanup_completes() {
        let parent = tempfile::tempdir().expect("temp parent");
        let manager = OperationManager::new(ManagerOptions::default()).expect("manager");
        let executor = LocalIoExecutor::new(manager.clone(), LocalExecutionConfig::default()).expect("executor");
        let mut operation = super::submit_temp_file(
            LocalFileSystem::host().expect("filesystem"),
            executor.clone(),
            &manager.as_async(),
            Default::default(),
            LocalTempFileOptions::new().with_parent(parent.path()),
        )
        .expect("request accepted");
        let access = match block_on(operation.wait_outcome()).expect("outcome") {
            OperationOutcome::Succeeded(access) => access,
            _ => panic!("temporary file should be created"),
        };

        let command = access
            .enqueue_native_command(&executor, |_| -> () { panic!("injected native command panic") })
            .expect("command queued");
        assert_eq!(block_on(command), Err(LocalTempCommandError::Panicked));
        assert_eq!(std::fs::read_dir(parent.path()).unwrap().count(), 1);

        drop(access);
        let settlement = block_on(operation.wait_settlement());
        assert_eq!(settlement.recovery_count, 0);
        assert!(settlement.cleanup_failures.is_empty());
        assert_eq!(std::fs::read_dir(parent.path()).unwrap().count(), 0);
    }

    #[test]
    fn test_temp_submission_reports_native_creation_failures_without_retaining_resources() {
        let parent = tempfile::tempdir().expect("temp parent");
        let missing_parent = parent.path().join("missing");
        let manager = OperationManager::new(ManagerOptions::default()).expect("manager");
        let executor = LocalIoExecutor::new(manager.clone(), LocalExecutionConfig::default()).expect("local executor");
        let filesystem = LocalFileSystem::host().expect("filesystem");
        let async_manager = manager.as_async();

        let mut file_operation = super::submit_temp_file(
            filesystem.clone(),
            executor.clone(),
            &async_manager,
            Default::default(),
            LocalTempFileOptions::new().with_parent(&missing_parent),
        )
        .expect("file request accepted before native creation");
        assert!(matches!(
            block_on(file_operation.wait_outcome()).expect("file outcome"),
            OperationOutcome::Failed(LocalCreateFailure::Local(_))
        ));
        let file_settlement = block_on(file_operation.wait_settlement());
        assert_eq!(file_settlement.recovery_count, 0);
        assert!(!missing_parent.exists());

        let mut directory_operation = super::submit_temp_directory(
            filesystem,
            executor,
            &async_manager,
            Default::default(),
            LocalTempDirectoryOptions::new().with_parent(&missing_parent),
        )
        .expect("directory request accepted before native creation");
        assert!(matches!(
            block_on(directory_operation.wait_outcome()).expect("directory outcome"),
            OperationOutcome::Failed(LocalCreateFailure::Local(_))
        ));
        let directory_settlement = block_on(directory_operation.wait_settlement());
        assert_eq!(directory_settlement.recovery_count, 0);
        assert!(!missing_parent.exists());
    }

    #[test]
    fn test_temp_creation_capacity_is_retained_until_resource_cleanup_then_retry_succeeds() {
        let parent = tempfile::tempdir().expect("temp parent");
        let manager = OperationManager::new(ManagerOptions::default()).expect("manager");
        let executor = LocalIoExecutor::new(
            manager.clone(),
            LocalExecutionConfig {
                max_disk_bytes: 1,
                ..LocalExecutionConfig::default()
            },
        )
        .expect("local executor");
        let filesystem = LocalFileSystem::host().expect("filesystem");
        let async_manager = manager.as_async();

        let mut first = super::submit_temp_file(
            filesystem.clone(),
            executor.clone(),
            &async_manager,
            Default::default(),
            LocalTempFileOptions::new().with_parent(parent.path()),
        )
        .expect("first request accepted");
        let first_access = match block_on(first.wait_outcome()).expect("first outcome") {
            OperationOutcome::Succeeded(access) => access,
            _ => panic!("expected first temporary file"),
        };

        let mut rejected = super::submit_temp_directory(
            filesystem.clone(),
            executor.clone(),
            &async_manager,
            Default::default(),
            LocalTempDirectoryOptions::new().with_parent(parent.path()),
        )
        .expect("operation admission is independent of disk capacity");
        assert!(matches!(
            block_on(rejected.wait_outcome()).expect("rejected outcome"),
            OperationOutcome::Failed(LocalCreateFailure::Capacity)
        ));
        let rejected_settlement = block_on(rejected.wait_settlement());
        assert_eq!(rejected_settlement.recovery_count, 0);
        assert_eq!(std::fs::read_dir(parent.path()).unwrap().count(), 1);

        drop(first_access);
        assert_eq!(block_on(first.wait_settlement()).recovery_count, 0);
        assert_eq!(std::fs::read_dir(parent.path()).unwrap().count(), 0);

        let mut retry = super::submit_temp_directory(
            filesystem,
            executor,
            &async_manager,
            Default::default(),
            LocalTempDirectoryOptions::new().with_parent(parent.path()),
        )
        .expect("retry admitted after capacity release");
        let retry_access = match block_on(retry.wait_outcome()).expect("retry outcome") {
            OperationOutcome::Succeeded(access) => access,
            _ => panic!("expected retry to create directory"),
        };
        drop(retry_access);
        assert_eq!(block_on(retry.wait_settlement()).recovery_count, 0);
        assert_eq!(std::fs::read_dir(parent.path()).unwrap().count(), 0);
    }

    #[test]
    fn test_normal_transition_rearms_the_returned_file_owner() {
        let parent = tempfile::tempdir().expect("temp parent");
        let manager = OperationManager::new(ManagerOptions::default()).expect("manager");
        let executor = LocalIoExecutor::new(manager.clone(), super::super::LocalExecutionConfig::default())
            .expect("local executor");
        let filesystem = LocalFileSystem::host().expect("filesystem");
        let parent_path = parent.path().to_path_buf();
        let completion = manager
            .run_and_settle(qubit_operation::OperationOptions::default(), move |scope| {
                let reservation = scope.reserve_resource().expect("resource reservation");
                let file = filesystem
                    .create_temp_file_with_options(&LocalTempFileOptions::new().with_parent(&parent_path))
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
                let access = LocalTempFileAccess { resource: reference };
                let receiver = access
                    .enqueue_native_transition(&executor, |owner| Ok::<_, (LocalTempNative, ())>((Some(owner), ())))
                    .expect("command queued");
                block_on(receiver).expect("command result").expect("transition result");
                let owner = access
                    .resource
                    .native
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .take()
                    .expect("transition should return the owner");
                drop(owner);
                assert_eq!(std::fs::read_dir(&parent_path).unwrap().count(), 0);
                drop(access);
                Ok::<(), ()>(())
            })
            .expect("operation completed");
        assert_eq!(completion.settlement.recovery_count, 0);
        assert!(completion.settlement.cleanup_failures.is_empty());
    }

    #[test]
    fn test_sealed_access_returns_unsupported_without_consuming_its_owner() {
        let parent = tempfile::tempdir().expect("temp parent");
        let manager = OperationManager::new(ManagerOptions::default()).expect("manager");
        let executor = LocalIoExecutor::new(manager.clone(), super::super::LocalExecutionConfig::default())
            .expect("local executor");
        let filesystem = LocalFileSystem::host().expect("filesystem");
        let parent_path = parent.path().to_path_buf();
        let invoked = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let completion = manager
            .run_and_settle(qubit_operation::OperationOptions::default(), move |scope| {
                let reservation = scope.reserve_resource().expect("resource reservation");
                let file = filesystem
                    .create_temp_file_with_options(&LocalTempFileOptions::new().with_parent(&parent_path))
                    .expect("temporary file");
                let sealed = file
                    .seal_with_options(&crate::options::LocalSealOptions::new())
                    .expect("temp file should seal");
                let owner = LocalTempResource::new(
                    LocalTempNative::Sealed(sealed),
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
                let access = LocalTempFileAccess { resource: reference };
                let resource_id = access.resource_id();
                for _ in 0..3 {
                    let invoked = Arc::clone(&invoked);
                    let receiver = access
                        .enqueue_native_transition(
                            &executor,
                            move |owner| -> Result<
                                (Option<LocalTempNative>, ()),
                                (LocalTempNative, crate::LocalFileError),
                            > {
                                invoked.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                                let cause = crate::LocalFileError::new(
                                    crate::LocalFileErrorKind::Unsupported,
                                    LocalFileOperation::PersistTemp,
                                );
                                match owner {
                                    LocalTempNative::Sealed(sealed) => Err((LocalTempNative::Sealed(sealed), cause)),
                                    owner => Err((owner, cause)),
                                }
                            },
                        )
                        .expect("unsupported command dispatch");
                    let Err(cause) = block_on(receiver).expect("callback should run") else {
                        panic!("sealed lifecycle command should report typed Unsupported");
                    };
                    assert_eq!(cause.kind(), crate::LocalFileErrorKind::Unsupported);
                    assert_eq!(access.resource_id(), resource_id);
                    let native = access.resource.native.lock().expect("native lock");
                    assert!(matches!(native.as_ref(), Some(LocalTempNative::Sealed(_))));
                }
                assert_eq!(invoked.load(std::sync::atomic::Ordering::SeqCst), 3);
                drop(access);
                Ok::<(), ()>(())
            })
            .expect("operation completed");
        assert_eq!(completion.settlement.recovery_count, 0);
        assert!(completion.settlement.cleanup_failures.is_empty());
        assert_eq!(std::fs::read_dir(parent.path()).unwrap().count(), 0);
    }

    fn assert_panicked_transition_cleans_anchor(filesystem: LocalFileSystem, parent_path: std::path::PathBuf) {
        let manager = OperationManager::new(ManagerOptions::default()).expect("manager");
        let executor = LocalIoExecutor::new(manager.clone(), super::super::LocalExecutionConfig::default())
            .expect("local executor");
        let completion = manager
            .run_and_settle(qubit_operation::OperationOptions::default(), move |scope| {
                let reservation = scope.reserve_resource().expect("resource reservation");
                let file = filesystem
                    .create_temp_file_with_options(&LocalTempFileOptions::new().with_parent(&parent_path))
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
                let access = LocalTempFileAccess { resource: reference };
                let receiver = access
                    .enqueue_native_transition(
                        &executor,
                        |_owner| -> Result<(Option<LocalTempNative>, ()), (LocalTempNative, ())> {
                            panic!("injected transition panic")
                        },
                    )
                    .expect("command queued");
                assert!(matches!(block_on(receiver), Err(LocalTempCommandError::Panicked)));
                let retry_receiver = access
                    .enqueue_native_transition(
                        &executor,
                        |_owner| -> Result<(Option<LocalTempNative>, ()), (LocalTempNative, ())> {
                            panic!("Unconfirmed resources must not reach another transition");
                        },
                    )
                    .expect("Unconfirmed preflight queued");
                assert!(matches!(
                    block_on(retry_receiver),
                    Err(LocalTempCommandError::ResourceUnavailable)
                ));
                #[cfg(feature = "test-support")]
                {
                    let _fault = crate::test_support::install_test_fault("temp-file-sandbox-remove")
                        .expect("first sandbox cleanup should fail");
                    let mut native = access.resource.native.lock().expect("native lock");
                    let Some(LocalTempNative::Unconfirmed(LocalTempCleanupAnchor::File(anchor))) = native.as_mut()
                    else {
                        panic!("a panicked transition should retain its cleanup anchor");
                    };
                    assert!(anchor.cleanup().is_err());
                    anchor.cleanup().expect("retained anchor should retry cleanup");
                    assert_eq!(anchor.source_state(), LocalTempSourceState::Released);
                }
                drop(access);
                Ok::<(), ()>(())
            })
            .expect("operation completed");
        assert_eq!(completion.settlement.recovery_count, 0);
        assert!(completion.settlement.cleanup_failures.is_empty());
    }

    #[test]
    fn test_panicked_host_transition_retains_and_cleans_identity_checked_anchor() {
        let parent = tempfile::tempdir().expect("temp parent");
        assert_panicked_transition_cleans_anchor(
            LocalFileSystem::host().expect("Host filesystem"),
            parent.path().to_path_buf(),
        );
        assert_eq!(std::fs::read_dir(parent.path()).unwrap().count(), 0);
    }

    #[test]
    fn test_panicked_rooted_transition_retains_and_cleans_root_authority_anchor() {
        let root = tempfile::tempdir().expect("Rooted root");
        assert_panicked_transition_cleans_anchor(
            LocalFileSystem::rooted(root.path()).expect("Rooted filesystem"),
            std::path::PathBuf::from("/"),
        );
        assert_eq!(std::fs::read_dir(root.path()).unwrap().count(), 0);
    }

    fn assert_panicked_directory_transition_cleans_anchor(
        filesystem: LocalFileSystem,
        parent_path: std::path::PathBuf,
    ) {
        let manager = OperationManager::new(ManagerOptions::default()).expect("manager");
        let executor = LocalIoExecutor::new(manager.clone(), super::super::LocalExecutionConfig::default())
            .expect("local executor");
        let completion = manager
            .run_and_settle(qubit_operation::OperationOptions::default(), move |scope| {
                let reservation = scope.reserve_resource().expect("resource reservation");
                let directory = filesystem
                    .create_temp_directory_with_options(&LocalTempDirectoryOptions::new().with_parent(&parent_path))
                    .expect("temporary directory");
                let owner = LocalTempResource::new(
                    LocalTempNative::Directory(directory),
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
                let access = LocalTempFileAccess { resource: reference };
                let receiver = access
                    .enqueue_native_transition(
                        &executor,
                        |_owner| -> Result<(Option<LocalTempNative>, ()), (LocalTempNative, ())> {
                            panic!("injected directory transition panic")
                        },
                    )
                    .expect("directory transition queued");
                assert!(matches!(block_on(receiver), Err(LocalTempCommandError::Panicked)));
                let retry_receiver = access
                    .enqueue_native_transition(
                        &executor,
                        |_owner| -> Result<(Option<LocalTempNative>, ()), (LocalTempNative, ())> {
                            panic!("Unconfirmed resources must not reach another transition");
                        },
                    )
                    .expect("Unconfirmed preflight queued");
                assert!(matches!(
                    block_on(retry_receiver),
                    Err(LocalTempCommandError::ResourceUnavailable)
                ));
                #[cfg(feature = "test-support")]
                {
                    let _fault = crate::test_support::install_test_fault("temp-directory-sandbox-remove")
                        .expect("first directory sandbox cleanup should fail");
                    let mut native = access.resource.native.lock().expect("native lock");
                    let Some(LocalTempNative::Unconfirmed(LocalTempCleanupAnchor::Directory(anchor))) = native.as_mut()
                    else {
                        panic!("a panicked directory transition should retain its anchor");
                    };
                    assert!(anchor.cleanup().is_err());
                    anchor.cleanup().expect("directory anchor should retry cleanup");
                    assert_eq!(anchor.source_state(), LocalTempSourceState::Released);
                }
                drop(access);
                Ok::<(), ()>(())
            })
            .expect("operation completed");
        assert_eq!(completion.settlement.recovery_count, 0);
        assert!(completion.settlement.cleanup_failures.is_empty());
    }

    #[test]
    fn test_panicked_host_and_rooted_directory_transitions_retain_cleanup_authority() {
        let host_parent = tempfile::tempdir().expect("Host parent");
        assert_panicked_directory_transition_cleans_anchor(
            LocalFileSystem::host().expect("Host filesystem"),
            host_parent.path().to_path_buf(),
        );
        assert_eq!(std::fs::read_dir(host_parent.path()).unwrap().count(), 0);

        let rooted_parent = tempfile::tempdir().expect("Rooted parent");
        assert_panicked_directory_transition_cleans_anchor(
            LocalFileSystem::rooted(rooted_parent.path()).expect("Rooted filesystem"),
            std::path::PathBuf::from("/"),
        );
        assert_eq!(std::fs::read_dir(rooted_parent.path()).unwrap().count(), 0);
    }

    fn run_directory_publication_transition(
        filesystem: LocalFileSystem,
        parent_path: std::path::PathBuf,
        target: Option<std::path::PathBuf>,
    ) -> std::path::PathBuf {
        let manager = OperationManager::new(ManagerOptions::default()).expect("manager");
        let executor = LocalIoExecutor::new(manager.clone(), super::super::LocalExecutionConfig::default())
            .expect("local executor");
        let completion = manager
            .run_and_settle(qubit_operation::OperationOptions::default(), move |scope| {
                let reservation = scope.reserve_resource().expect("resource reservation");
                let directory = filesystem
                    .create_temp_directory_with_options(&LocalTempDirectoryOptions::new().with_parent(&parent_path))
                    .expect("temporary directory");
                let owner = LocalTempResource::new(
                    LocalTempNative::Directory(directory),
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
                let access = LocalTempFileAccess { resource: reference };
                let receiver = access
                    .enqueue_native_transition(&executor, move |native| {
                        let LocalTempNative::Directory(directory) = native else {
                            return Err((
                                native,
                                crate::LocalFileError::from_io(
                                    LocalFileOperation::PersistTemp,
                                    None,
                                    None,
                                    std::io::Error::other("expected a temporary directory owner"),
                                ),
                            ));
                        };
                        let result = match target {
                            Some(target) => directory.persist(target),
                            None => directory.keep(),
                        };
                        match result {
                            Ok(outcome) => Ok((None, outcome)),
                            Err(error) => {
                                let parts = error.into_parts();
                                Err((LocalTempNative::Directory(parts.resource), parts.error))
                            }
                        }
                    })
                    .expect("directory publication queued");
                let outcome = block_on(receiver)
                    .expect("directory publication command")
                    .expect("directory publication outcome");
                let published = outcome.path().to_path_buf();
                assert!(outcome.cleanup_error().is_none());
                drop(access);
                Ok::<_, ()>(published)
            })
            .expect("operation completed");
        assert_eq!(completion.settlement.recovery_count, 0);
        assert!(completion.settlement.cleanup_failures.is_empty());
        match completion.outcome {
            qubit_operation::OperationOutcome::Succeeded(path) => path,
            _ => panic!("directory publication should succeed"),
        }
    }

    #[test]
    fn test_directory_keep_and_persist_succeed_through_native_transition() {
        let keep_parent = tempfile::tempdir().expect("Keep parent");
        let kept_path = run_directory_publication_transition(
            LocalFileSystem::host().expect("Host filesystem"),
            keep_parent.path().to_path_buf(),
            None,
        );
        assert!(kept_path.exists());
        assert_eq!(std::fs::read_dir(keep_parent.path()).unwrap().count(), 1);

        let persist_parent = tempfile::tempdir().expect("Persist parent");
        let target = persist_parent.path().join("published");
        let persisted_path = run_directory_publication_transition(
            LocalFileSystem::host().expect("Host filesystem"),
            persist_parent.path().to_path_buf(),
            Some(target.clone()),
        );
        assert_eq!(persisted_path, target);
        assert!(target.is_dir());
        assert_eq!(std::fs::read_dir(persist_parent.path()).unwrap().count(), 1);

        let rooted_keep_parent = tempfile::tempdir().expect("Rooted Keep parent");
        let rooted_keep_path = run_directory_publication_transition(
            LocalFileSystem::rooted(rooted_keep_parent.path()).expect("Rooted filesystem"),
            std::path::PathBuf::from("/"),
            None,
        );
        let rooted_keep_relative = rooted_keep_path
            .strip_prefix(std::path::Path::new("/"))
            .expect("Rooted Keep result should be virtual absolute");
        assert!(rooted_keep_parent.path().join(rooted_keep_relative).is_dir());
        assert_eq!(std::fs::read_dir(rooted_keep_parent.path()).unwrap().count(), 1);

        let rooted_persist_parent = tempfile::tempdir().expect("Rooted Persist parent");
        let rooted_target = std::path::PathBuf::from("/published");
        let rooted_persist_path = run_directory_publication_transition(
            LocalFileSystem::rooted(rooted_persist_parent.path()).expect("Rooted filesystem"),
            std::path::PathBuf::from("/"),
            Some(rooted_target),
        );
        assert_eq!(rooted_persist_path, std::path::PathBuf::from("/published"));
        assert!(rooted_persist_parent.path().join("published").is_dir());
        assert_eq!(std::fs::read_dir(rooted_persist_parent.path()).unwrap().count(), 1);
    }

    #[test]
    fn test_normal_directory_transition_rearms_the_returned_owner() {
        let parent = tempfile::tempdir().expect("temp parent");
        let manager = OperationManager::new(ManagerOptions::default()).expect("manager");
        let executor = LocalIoExecutor::new(manager.clone(), super::super::LocalExecutionConfig::default())
            .expect("local executor");
        let filesystem = LocalFileSystem::host().expect("filesystem");
        let parent_path = parent.path().to_path_buf();
        let completion = manager
            .run_and_settle(qubit_operation::OperationOptions::default(), move |scope| {
                let reservation = scope.reserve_resource().expect("resource reservation");
                let directory = filesystem
                    .create_temp_directory_with_options(&LocalTempDirectoryOptions::new().with_parent(&parent_path))
                    .expect("temporary directory");
                let owner = LocalTempResource::new(
                    LocalTempNative::Directory(directory),
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
                let access = LocalTempFileAccess { resource: reference };
                let receiver = access
                    .enqueue_native_transition(&executor, |owner| Ok::<_, (LocalTempNative, ())>((Some(owner), ())))
                    .expect("directory transition queued");
                block_on(receiver).expect("command result").expect("transition result");
                let owner = access
                    .resource
                    .native
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .take()
                    .expect("transition should return the directory owner");
                drop(owner);
                assert_eq!(std::fs::read_dir(&parent_path).unwrap().count(), 0);
                drop(access);
                Ok::<(), ()>(())
            })
            .expect("operation completed");
        assert_eq!(completion.settlement.recovery_count, 0);
        assert!(completion.settlement.cleanup_failures.is_empty());
    }

    #[cfg(unix)]
    #[test]
    fn test_directory_cleanup_anchor_refuses_symlink_replacement() {
        let parent = tempfile::tempdir().expect("temp parent");
        let target = tempfile::tempdir().expect("symlink target");
        let mut directory = LocalFileSystem::host()
            .expect("filesystem")
            .create_temp_directory_with_options(&LocalTempDirectoryOptions::new().with_parent(parent.path()))
            .expect("temporary directory");
        let path = directory.path().to_path_buf();
        let mut anchor = directory.cleanup_anchor();
        directory.disarm_cleanup_on_drop();
        drop(directory);

        let moved = parent.path().join("moved-original");
        std::fs::rename(&path, &moved).expect("move original directory");
        std::os::unix::fs::symlink(target.path(), &path).expect("install symlink replacement");
        assert!(anchor.cleanup().is_err());
        assert!(std::fs::symlink_metadata(&path).unwrap().file_type().is_symlink());
        assert!(target.path().is_dir());
        assert!(moved.is_dir());
    }

    #[cfg(feature = "test-support")]
    fn assert_cleanup_anchor_retries(filesystem: LocalFileSystem, parent_path: std::path::PathBuf) {
        let mut file = filesystem
            .create_temp_file_with_options(&LocalTempFileOptions::new().with_parent(&parent_path))
            .expect("temporary file");
        let mut anchor = file.cleanup_anchor();
        file.disarm_cleanup_on_drop();
        drop(file);

        let _fault =
            crate::test_support::install_test_fault("temp-file-sandbox-remove").expect("sandbox cleanup fault");
        assert!(anchor.cleanup().is_err());
        anchor.cleanup().expect("anchor should retry sandbox cleanup");
    }

    #[cfg(feature = "test-support")]
    #[test]
    fn test_host_and_rooted_cleanup_anchors_remain_retryable() {
        let host_parent = tempfile::tempdir().expect("Host parent");
        assert_cleanup_anchor_retries(
            LocalFileSystem::host().expect("Host filesystem"),
            host_parent.path().to_path_buf(),
        );
        assert_eq!(std::fs::read_dir(host_parent.path()).unwrap().count(), 0);

        let rooted_parent = tempfile::tempdir().expect("Rooted parent");
        assert_cleanup_anchor_retries(
            LocalFileSystem::rooted(rooted_parent.path()).expect("Rooted filesystem"),
            std::path::PathBuf::from("/"),
        );
        assert_eq!(std::fs::read_dir(rooted_parent.path()).unwrap().count(), 0);
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
    fn test_partial_creation_recovery_is_retained_when_stopped_commit_is_rejected() {
        let parent = tempfile::tempdir().expect("temp parent");
        let manager = OperationManager::new(ManagerOptions::default()).expect("manager");
        let executor = LocalIoExecutor::new(manager.clone(), super::super::LocalExecutionConfig::default())
            .expect("local executor");
        let filesystem = LocalFileSystem::host().expect("filesystem");
        let parent_path = parent.path().to_path_buf();
        let id = Arc::new(Mutex::new(None));
        let completion = manager
            .run_and_settle(qubit_operation::OperationOptions::default(), {
                let id = Arc::clone(&id);
                move |scope| {
                    let reservation = scope.reserve_resource().expect("reservation before native creation");
                    let file = filesystem
                        .create_temp_file_with_options(&LocalTempFileOptions::new().with_parent(&parent_path))
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
                    let failure =
                        super::retain_partial(reservation, error, recovery, executor, disk, handle, cleanup_permit);
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
        assert_eq!(std::fs::read_dir(parent.path()).expect("read parent").count(), 0);
    }

    #[test]
    fn test_partial_creation_recovery_is_committed_and_cleaned_after_admission() {
        let parent = tempfile::tempdir().expect("temp parent");
        let manager = OperationManager::new(ManagerOptions::default()).expect("manager");
        let executor =
            super::super::LocalIoExecutor::new(manager.clone(), super::super::LocalExecutionConfig::default())
                .expect("local executor");
        let filesystem = LocalFileSystem::host().expect("filesystem");
        let parent_path = parent.path().to_path_buf();
        let id = Arc::new(Mutex::new(None));
        let completion = manager
            .run_and_settle(qubit_operation::OperationOptions::default(), {
                let id = Arc::clone(&id);
                move |scope| {
                    let reservation = scope.reserve_resource().expect("reservation before native creation");
                    let file = filesystem
                        .create_temp_file_with_options(&LocalTempFileOptions::new().with_parent(&parent_path))
                        .expect("construct partial native owner");
                    let recovery = crate::LocalTempRecovery::from_file(file);
                    let handle = executor.reserve_handle().expect("handle permit");
                    let disk = executor.reserve_disk(1).expect("disk permit");
                    let cleanup_permit = executor.reserve_cleanup().expect("cleanup slot");
                    let error = crate::LocalFileError::from_io(
                        LocalFileOperation::CreateTempFile,
                        None,
                        None,
                        std::io::Error::other("injected partial create failure"),
                    );
                    let failure =
                        super::retain_partial(reservation, error, recovery, executor, disk, handle, cleanup_permit);
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
        assert_eq!(std::fs::read_dir(parent.path()).expect("read parent").count(), 0);
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
            std::panic::panic_any(());
        }
    }

    #[test]
    fn test_async_sender_wake_panic_does_not_hide_the_sent_result() {
        let (sender, mut receiver) = async_result();
        *receiver.0.waker.lock().expect("waker lock") = Some(Waker::from(Arc::new(WakePanics)));
        assert!(sender.send(42_u8).is_ok());
        let waker = Waker::from(Arc::new(ThreadWake(std::thread::current())));
        let mut context = Context::from_waker(&waker);
        assert!(matches!(Pin::new(&mut receiver).poll(&mut context), Poll::Ready(42)));
    }
}
