// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use qubit_operation::ResourceId;
use qubit_operation::ResourceRef;

use super::LocalTempCleanupAnchor;
use super::LocalTempCommandError;
use super::LocalTempCommandReceiver;
use super::LocalTempNative;
use super::LocalTempResource;
use super::arm_native_cleanup_on_drop;
use super::async_result;
use crate::operation::executor::LocalIoExecutor;

/// Public read-only access token for a managed temporary file.
///
/// The token keeps the registered owner alive while a command is queued. Native
/// file access is serialized on the local worker instead of running in callers.
pub struct LocalTempFileAccess {
    /// Registered owner shared with cleanup and other operation helpers.
    pub(super) resource: ResourceRef<LocalTempResource>,
}
impl LocalTempFileAccess {
    /// Returns the stable public resource identity.
    ///
    /// # Parameters
    ///
    /// * `self` — access token whose registered identity is returned.
    pub fn resource_id(&self) -> ResourceId {
        self.resource.id()
    }

    /// Clones this access token without registering another resource.
    ///
    /// # Parameters
    ///
    /// * `self` — access token whose resource reference is retained.
    pub(crate) fn clone_access(&self) -> Self {
        Self {
            resource: self.resource.clone(),
        }
    }

    /// Retains the same registered owner for a related operation/helper.
    ///
    /// # Parameters
    ///
    /// * `self` — access token whose owner reference is retained.
    pub(crate) fn clone_resource_ref(&self) -> ResourceRef<LocalTempResource> {
        self.resource.clone()
    }

    /// Retains an additional disk reservation on the registered native owner.
    ///
    /// # Parameters
    ///
    /// * `self` — token identifying the owner that retains the reservation.
    /// * `permit` — capacity to retain until the native resource is released.
    pub(crate) fn retain_disk_permit(&self, permit: crate::operation::internal::permits::DiskPermit) {
        self.resource
            ._disk
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .push(permit);
    }

    /// Queues one serialized native-resource command on the local I/O worker.
    /// The cloned resource reference remains alive until the command returns.
    ///
    /// # Parameters
    ///
    /// * `self` — access token identifying the resource to mutate or inspect.
    /// * `executor` — worker that serializes native operations.
    /// * `command` — crate-controlled action run while the native owner is
    ///   locked.
    ///
    /// # Type Parameters
    ///
    /// * `T` — value returned by the native command.
    /// * `F` — one-shot command closure executed on the worker.
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
    /// File or Directory owner is disarmed while the callback runs, and its
    /// cleanup-only anchor is retained if the callback panics. A terminal
    /// publication callback must still avoid panicking after publication has
    /// moved the source away from its cleanup coordinate.
    ///
    /// # Parameters
    ///
    /// * `self` — token identifying the native resource being transitioned.
    /// * `executor` — worker that runs the consuming transition.
    /// * `command` — crate-controlled conversion that returns the owner on
    ///   ordinary failure and preserves cleanup authority during unwinding.
    ///
    /// # Type Parameters
    ///
    /// * `T` — value produced after a successful ownership transition.
    /// * `E` — ordinary transition error returned with the original owner.
    /// * `F` — consuming transition closure run on the worker.
    pub(crate) fn enqueue_native_transition<T, E, F>(
        &self,
        executor: &LocalIoExecutor,
        command: F,
    ) -> Result<LocalTempCommandReceiver<Result<T, E>>, LocalTempCommandError>
    where
        T: Send + 'static,
        E: Send + 'static,
        F: FnOnce(LocalTempNative) -> Result<(Option<LocalTempNative>, T), (LocalTempNative, E)> + Send + 'static,
    {
        let resource = self.clone_resource_ref();
        let (sender, receiver) = async_result();
        executor
            .enqueue_task(move || {
                let mut native = resource.native.lock().unwrap_or_else(|e| e.into_inner());
                // Preserve a cleanup-only copy before moving the owner into a
                // consuming callback. Disarm the original so its unwind Drop
                // cannot race the retained anchor.
                let fallback = match native.as_mut() {
                    Some(LocalTempNative::File(file)) => {
                        let anchor = file.cleanup_anchor();
                        file.disarm_cleanup_on_drop();
                        LocalTempCleanupAnchor::File(anchor)
                    }
                    Some(LocalTempNative::Directory(directory)) => {
                        let anchor = directory.cleanup_anchor();
                        directory.disarm_cleanup_on_drop();
                        LocalTempCleanupAnchor::Directory(anchor)
                    }
                    Some(LocalTempNative::Sealed(sealed)) => {
                        let anchor = sealed.cleanup_anchor();
                        sealed.disarm_cleanup_on_drop();
                        LocalTempCleanupAnchor::File(anchor)
                    }
                    Some(LocalTempNative::Recovery(_)) => {
                        // Recovery owners remain registered for the lifecycle
                        // adapter's typed Unsupported result.
                        drop(native);
                        let _ = sender.send(Err(LocalTempCommandError::UnsupportedKind));
                        drop(resource);
                        return;
                    }
                    None | Some(LocalTempNative::Unconfirmed(_)) => {
                        drop(native);
                        let _ = sender.send(Err(LocalTempCommandError::ResourceUnavailable));
                        drop(resource);
                        return;
                    }
                };
                let outcome = match native.take() {
                    None => Err(LocalTempCommandError::ResourceUnavailable),
                    Some(owner) => match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| command(owner))) {
                        Ok(Ok((mut owner, value))) => {
                            if let Some(owner) = owner.as_mut() {
                                arm_native_cleanup_on_drop(owner);
                            }
                            *native = owner;
                            Ok(Ok(value))
                        }
                        Ok(Err((owner, error))) => {
                            let mut owner = owner;
                            arm_native_cleanup_on_drop(&mut owner);
                            *native = Some(owner);
                            Ok(Err(error))
                        }
                        Err(payload) => {
                            if let Err(payload) =
                                std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| drop(payload)))
                            {
                                std::mem::forget(payload);
                            }
                            *native = Some(LocalTempNative::Unconfirmed(fallback));
                            Err(LocalTempCommandError::Panicked)
                        }
                    },
                };
                drop(native);
                let _ = sender.send(outcome);
                drop(resource);
            })
            .map_err(|()| LocalTempCommandError::Capacity)?;
        Ok(LocalTempCommandReceiver(receiver))
    }
}
