// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::io::IoSlice;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::MutexGuard;

use qubit_io::Output;
use qubit_operation::EffectFact;
use qubit_operation::OperationOptions;
use qubit_operation::OperationOutcome;

use super::super::LocalDurabilityState;
use super::super::LocalFactObserver;
use super::super::LocalOperation;
use super::super::LocalOperationOptions;
use super::super::LocalPublicationState;
use super::super::LocalSubmitError;
#[cfg(feature = "digest")]
use super::super::publish::LocalConditionalWriterCommandError;
#[cfg(feature = "digest")]
use super::super::publish::LocalConditionalWriterCommitRequest;
#[cfg(feature = "digest")]
use super::super::publish::LocalConditionalWriterOperation;
use super::super::publish::LocalManagedAbortCleanup;
use super::super::publish::LocalManagedAbortResource;
use super::super::publish::LocalStagedTreeDiskReservation;
use super::super::publish::LocalStagedTreeFile;
use super::super::publish::LocalStagedTreePublishError;
use super::super::publish::LocalStagedTreePublishRequest;
use super::super::publish::LocalStagedTreeRecovery;
use super::super::publish::LocalStagedTreeReservedError;
use super::super::publish::LocalTempLifecycleCommand;
use super::super::publish::LocalTempLifecycleError;
use super::super::publish::LocalTempLifecycleOperation;
use super::super::publish::LocalTempLifecycleRequest;
use super::super::publish::LocalWriterCommandError;
use super::super::publish::LocalWriterCommitRequest;
use super::AsyncLocalFileSystem;
use super::OwnedSubmission;
use crate::LocalFileError;
use crate::LocalWriteOutcome;
#[cfg(feature = "digest")]
use crate::operation::publish::abort_conditional_writer;
#[cfg(feature = "digest")]
use crate::operation::publish::abort_managed_conditional_after_error;
#[cfg(feature = "digest")]
use crate::operation::publish::abort_managed_conditional_after_stop;
use crate::operation::publish::abort_managed_file_writer;
use crate::operation::publish::abort_managed_writer_after_error;
use crate::operation::publish::abort_managed_writer_after_stop;
use crate::operation::publish::map_temp_command_error;
use crate::operation::publish::record_append_writer_bytes;
use crate::operation::publish::record_local_effect;
use crate::operation::publish::record_stage_write;
use crate::operation::publish::record_temp_failure;
use crate::operation::publish::record_writer_effect;
use crate::operation::publish::stop_report;
use crate::operation::publish::transition_temp_native;
use crate::operation::publish::update_writer_effect_observer;

/// Locks publication state while recovering its value after a prior panic.
///
/// Poisoning records that a thread unwound while holding the mutex, but these
/// slots only protect ownership transfer bookkeeping; retaining the contained
/// state allows the operation's cleanup path to settle the native resource.
///
/// # Type Parameters
///
/// * `T` is the publication state protected by the mutex.
///
/// # Parameters
///
/// * `mutex` protects the ownership-transfer state being recovered.
fn lock_publication_state<T>(mutex: &Mutex<T>) -> MutexGuard<'_, T> {
    match mutex.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    }
}

impl AsyncLocalFileSystem {
    /// Sets the default symlink policy on this facade's owned filesystem value.
    ///
    /// This changes only the filesystem value retained by this facade; it does
    /// not modify the manager or any other facade. Conditional publication
    /// requires [`crate::policy::LocalSymlinkPolicy::Reject`] because it
    /// validates the destination without following symbolic links.
    ///
    /// # Parameters
    ///
    /// * `policy` - Symlink traversal policy stored on the returned facade.
    pub fn with_symlink_policy(mut self, policy: crate::policy::LocalSymlinkPolicy) -> crate::LocalResult<Self> {
        self.filesystem.set_symlink_policy(policy)?;
        Ok(self)
    }

    /// Commits owned bytes through the local writer on a bounded I/O worker.
    ///
    /// The operation checks cancellation immediately before opening and
    /// writing. Once the native commit begins, it waits for the writer's
    /// actual result.
    ///
    /// # Parameters
    ///
    /// * `manager` - Operation manager that owns the submission.
    /// * `operation` - Cancellation and deadline policy for this commit.
    /// * `request` - Owned destination, write options, and bytes to publish.
    /// * `limits` - Per-operation memory, I/O, and resource limits.
    pub fn submit_writer_commit(
        &self,
        manager: &qubit_operation::OperationManager,
        operation: OperationOptions,
        request: LocalWriterCommitRequest,
        limits: LocalOperationOptions,
    ) -> Result<
        (
            LocalOperation<LocalWriteOutcome, LocalWriterCommandError>,
            LocalFactObserver,
        ),
        LocalSubmitError<LocalWriterCommitRequest>,
    > {
        let relative = request.path.is_relative();
        let destination = match (&self.host_base, relative) {
            (Some(base), true) => base.join(&request.path),
            _ => request.path.clone(),
        };
        let filesystem = self.filesystem.clone();
        let executor = self.executor.clone();
        let owned_capacity = request.bytes.capacity();
        self.submit_owned_with_bytes(
            manager,
            operation,
            request,
            OwnedSubmission {
                path_is_relative: relative,

                required_handles: 1,

                limits,

                initial_owned_bytes: owned_capacity,
            },
            move |request, scope, observer, _budget, _maximum| {
                if let Some(stop) = stop_report(scope) {
                    return OperationOutcome::Stopped(stop);
                }
                let reservation = match scope.reserve_resource() {
                    Ok(reservation) => reservation,
                    Err(qubit_operation::ResourceCapacityError::Stopped) => {
                        return OperationOutcome::Stopped(
                            stop_report(scope).expect("scope stopped before writer reservation"),
                        );
                    }
                    Err(error) => {
                        return OperationOutcome::Failed(LocalWriterCommandError::ResourceCapacity(error));
                    }
                };
                let Some(cleanup_permit) = executor.reserve_cleanup() else {
                    return OperationOutcome::Failed(LocalWriterCommandError::Open(LocalFileError::new(
                        crate::LocalFileErrorKind::ResourceLimit,
                        crate::LocalFileOperation::OpenWriter,
                    )));
                };
                let resource = LocalManagedAbortResource::new(cleanup_permit, abort_managed_file_writer);
                let writer_resource = match reservation.commit_with(
                    resource,
                    LocalManagedAbortCleanup::<crate::LocalFileWriter>::new(executor.cleanup_executor()),
                ) {
                    Ok(resource) => resource,
                    Err(error) => {
                        let (reason, _resource, _cleanup, _reservation) = error.into_parts();
                        return match reason {
                            qubit_operation::ResourceManageErrorKind::Stopped => {
                                OperationOutcome::Stopped(stop_report(scope).expect("scope stopped before writer open"))
                            }
                            reason => OperationOutcome::Failed(LocalWriterCommandError::ResourceCommit(reason)),
                        };
                    }
                };
                let resource_id = writer_resource.id();
                *lock_publication_state(&writer_resource.id) = Some(resource_id);
                let writer = match filesystem.open_writer_with_options(&destination, &request.options) {
                    Ok(writer) => writer,
                    Err(error) => {
                        return OperationOutcome::Failed(LocalWriterCommandError::Open(error));
                    }
                };
                *lock_publication_state(&writer_resource.native) = Some(writer);
                let submitted_bytes = request.bytes.len() as u64;
                let append_mode = request.options.mode() == crate::LocalWriteMode::Append;
                let mut append_effect_recorded = false;
                let mut offset = 0;
                while offset < request.bytes.len() {
                    if let Some(stop) = stop_report(scope) {
                        return abort_managed_writer_after_stop(
                            &writer_resource,
                            stop,
                            offset as u64,
                            scope,
                            &observer,
                            append_effect_recorded,
                        );
                    }
                    let slices = [IoSlice::new(&request.bytes[offset..])];
                    let write_result = {
                        let mut native = lock_publication_state(&writer_resource.native);
                        native
                            .as_mut()
                            .expect("managed writer is available")
                            .write_vectored(&slices)
                    };
                    match write_result {
                        Ok(0) => {
                            let error = LocalFileError::new(
                                crate::LocalFileErrorKind::DataCorruption,
                                crate::LocalFileOperation::Commit,
                            )
                            .with_path(destination.clone());
                            return OperationOutcome::Failed(abort_managed_writer_after_error(
                                &writer_resource,
                                error,
                                offset as u64,
                                scope,
                                &observer,
                                append_effect_recorded,
                            ));
                        }
                        Ok(written) => {
                            offset += written;
                            if append_mode {
                                record_append_writer_bytes(
                                    scope,
                                    &observer,
                                    written as u64,
                                    &mut append_effect_recorded,
                                );
                            }
                        }
                        Err(source) => {
                            let error = LocalFileError::from_io(
                                crate::LocalFileOperation::Commit,
                                Some(destination.clone()),
                                None,
                                source,
                            );
                            return OperationOutcome::Failed(abort_managed_writer_after_error(
                                &writer_resource,
                                error,
                                offset as u64,
                                scope,
                                &observer,
                                append_effect_recorded,
                            ));
                        }
                    }
                }
                let flush_result = {
                    let mut native = lock_publication_state(&writer_resource.native);
                    Output::flush(native.as_mut().expect("managed writer is available"))
                };
                if let Err(source) = flush_result {
                    let error = LocalFileError::from_io(
                        crate::LocalFileOperation::Commit,
                        Some(destination.clone()),
                        None,
                        source,
                    );
                    return OperationOutcome::Failed(abort_managed_writer_after_error(
                        &writer_resource,
                        error,
                        submitted_bytes,
                        scope,
                        &observer,
                        append_effect_recorded,
                    ));
                }
                #[cfg(feature = "test-support")]
                if append_effect_recorded && crate::local::test_support_enabled("lf7-append-panic-after-write") {
                    let _ = crate::local::test_fault_error();
                    panic!("injected panic after direct append");
                }
                // Cancellation is honored up to this gate. There is no stop check
                // after entering commit: the native publication result is authoritative.
                #[cfg(feature = "test-support")]
                crate::test_support::hit_test_gate("lf7-writer-before-publish");
                if let Some(stop) = stop_report(scope) {
                    return abort_managed_writer_after_stop(
                        &writer_resource,
                        stop,
                        submitted_bytes,
                        scope,
                        &observer,
                        append_effect_recorded,
                    );
                }
                let writer = lock_publication_state(&writer_resource.native)
                    .take()
                    .expect("managed writer is available at publication");
                match writer.commit() {
                    Ok(outcome) => {
                        let durability = if outcome.durable() {
                            LocalDurabilityState::Durable
                        } else {
                            LocalDurabilityState::Unknown
                        };
                        if append_effect_recorded {
                            update_writer_effect_observer(
                                &observer,
                                0,
                                0,
                                LocalPublicationState::Published,
                                durability,
                            );
                        } else {
                            record_writer_effect(
                                scope,
                                &observer,
                                outcome.bytes_written(),
                                LocalPublicationState::Published,
                                durability,
                            );
                        }
                        OperationOutcome::Succeeded(outcome)
                    }
                    Err(error) => {
                        let publication = match error.state() {
                            crate::LocalWriteFailureState::NotPublished => LocalPublicationState::NotPublished,
                            crate::LocalWriteFailureState::Published => LocalPublicationState::Published,
                            crate::LocalWriteFailureState::Indeterminate => LocalPublicationState::Unknown,
                        };
                        if append_effect_recorded {
                            let durability = match error.state() {
                                crate::LocalWriteFailureState::Published => LocalDurabilityState::Failed,
                                crate::LocalWriteFailureState::NotPublished
                                | crate::LocalWriteFailureState::Indeterminate => LocalDurabilityState::Unknown,
                            };
                            update_writer_effect_observer(
                                &observer,
                                0,
                                0,
                                LocalPublicationState::Published,
                                durability,
                            );
                        } else if publication != LocalPublicationState::NotPublished {
                            record_writer_effect(
                                scope,
                                &observer,
                                submitted_bytes,
                                publication,
                                LocalDurabilityState::Failed,
                            );
                        }
                        OperationOutcome::Failed(LocalWriterCommandError::Commit(error))
                    }
                }
            },
        )
    }
}

impl AsyncLocalFileSystem {
    /// Creates files beneath a private stage, verifies the exact tree, then
    /// publishes it.
    ///
    /// # Parameters
    ///
    /// * `manager` - Operation manager that owns the staged publication.
    /// * `operation` - Cancellation and deadline policy for this publication.
    /// * `request` - Parent, stage policy, file contents, and publish policy.
    /// * `limits` - Per-operation memory, disk, I/O, and resource limits.
    pub fn submit_staged_tree_publish(
        &self,
        manager: &qubit_operation::OperationManager,
        operation: OperationOptions,
        request: LocalStagedTreePublishRequest,
        limits: LocalOperationOptions,
    ) -> Result<
        (
            LocalOperation<crate::outcome::LocalTreePublishOutcome, LocalStagedTreePublishError>,
            LocalFactObserver,
        ),
        LocalSubmitError<LocalStagedTreePublishRequest>,
    > {
        let relative = request.parent.is_relative();
        let parent_path = match (&self.host_base, relative) {
            (Some(base), true) => base.join(&request.parent),
            _ => request.parent.clone(),
        };
        let owned_bytes = request
            .files
            .capacity()
            .saturating_mul(std::mem::size_of::<LocalStagedTreeFile>())
            .saturating_add(request.parent.as_os_str().len())
            .saturating_add(request.name.len())
            .saturating_add(request.files.iter().fold(0usize, |sum, file| {
                sum.saturating_add(file.bytes.capacity())
                    .saturating_add(file.path.as_os_str().len())
            }));
        let disk_bytes = request.files.iter().fold(0u64, |sum, file| {
            sum.saturating_add(u64::try_from(file.bytes.len()).unwrap_or(u64::MAX))
        });
        let disk_bytes = disk_bytes.max(1);
        let disk_limit = limits
            .max_disk_bytes
            .unwrap_or(self.executor.inner.config.max_disk_bytes);
        if disk_limit == 0 || disk_limit > self.executor.inner.config.max_disk_bytes || disk_bytes > disk_limit {
            return Err(LocalSubmitError::new(
                request,
                super::super::LocalSubmitRejection::CapacityExceeded,
            ));
        }
        let Some(disk) = self.executor.reserve_disk(disk_bytes) else {
            return Err(LocalSubmitError::new(
                request,
                super::super::LocalSubmitRejection::CapacityExceeded,
            ));
        };
        let reservation = LocalStagedTreeDiskReservation(Arc::new(disk));
        let filesystem = self.filesystem.clone();
        self.submit_owned_with_bytes(
            manager,
            operation,
            request,
            OwnedSubmission {
                path_is_relative: relative,

                required_handles: 2,

                limits,

                initial_owned_bytes: owned_bytes,
            },
            move |request, scope, observer, _budget, _maximum| {
                if let Some(stop) = stop_report(scope) {
                    return OperationOutcome::Stopped(stop);
                }
                let parent = match filesystem
                    .open_directory_with_options(&parent_path, &crate::options::LocalDirectoryOpenOptions::new())
                {
                    Ok(parent) => parent,
                    Err(error) => {
                        return OperationOutcome::Failed(LocalStagedTreePublishError::Parent(error));
                    }
                };
                let stage_options = request
                    .stage_options
                    .clone()
                    .with_privacy(crate::policy::LocalPrivacyRequirement::Required);
                let mut stage = match parent.create_staged_directory(&stage_options) {
                    Ok(stage) => stage,
                    Err(error) => {
                        return OperationOutcome::Failed(LocalStagedTreePublishError::Create(
                            LocalStagedTreeReservedError::new(error, reservation.clone()),
                        ));
                    }
                };
                let mut expected = Vec::with_capacity(request.files.len());
                for file in &request.files {
                    if let Some(stop) = stop_report(scope) {
                        return match stage.cleanup() {
                            Ok(()) => OperationOutcome::Stopped(stop),
                            Err(error) => OperationOutcome::Failed(LocalStagedTreePublishError::Cleanup(
                                LocalStagedTreeReservedError::new(error, reservation.clone()),
                            )),
                        };
                    }
                    let entry =
                        match crate::directory::LocalTreeEntry::new(file.path.clone(), crate::LocalFileKind::File) {
                            Ok(entry) => entry,
                            Err(cause) => {
                                return OperationOutcome::Failed(LocalStagedTreePublishError::Write {
                                    cause,
                                    stage: LocalStagedTreeRecovery::new(stage, reservation.clone()),
                                });
                            }
                        };
                    let mut input = file.bytes.as_slice();
                    if let Err(cause) = stage.write_file_new(&file.path, &mut input, file.bytes.len() as u64) {
                        record_stage_write(scope, &observer, 0, EffectFact::Unknown);
                        return OperationOutcome::Failed(LocalStagedTreePublishError::Write {
                            cause,
                            stage: LocalStagedTreeRecovery::new(stage, reservation.clone()),
                        });
                    }
                    record_stage_write(scope, &observer, file.bytes.len() as u64, EffectFact::Known);
                    expected.push(entry);
                }
                let validation = crate::directory::LocalTreeValidationOptions::new().with_exact_entries(expected);
                let publish_options = request.publish_options.clone().with_validation(validation);
                if let Some(stop) = stop_report(scope) {
                    return match stage.cleanup() {
                        Ok(()) => OperationOutcome::Stopped(stop),
                        Err(error) => OperationOutcome::Failed(LocalStagedTreePublishError::Cleanup(
                            LocalStagedTreeReservedError::new(error, reservation.clone()),
                        )),
                    };
                }
                match stage.publish_new(&parent, &request.name, &publish_options) {
                    Ok(outcome) => {
                        if outcome.publication() == crate::outcome::LocalTreePublication::Published {
                            record_local_effect(
                                scope,
                                &observer,
                                1,
                                0,
                                LocalPublicationState::Published,
                                if outcome.durable() {
                                    LocalDurabilityState::Durable
                                } else {
                                    LocalDurabilityState::Unknown
                                },
                            );
                        }
                        OperationOutcome::Succeeded(outcome)
                    }
                    Err(error) => {
                        match error.publication() {
                            crate::outcome::LocalTreePublication::Published => record_local_effect(
                                scope,
                                &observer,
                                1,
                                0,
                                LocalPublicationState::Published,
                                LocalDurabilityState::Failed,
                            ),
                            crate::outcome::LocalTreePublication::Indeterminate => record_local_effect(
                                scope,
                                &observer,
                                1,
                                0,
                                LocalPublicationState::Unknown,
                                LocalDurabilityState::Unknown,
                            ),
                            _ => {}
                        }
                        OperationOutcome::Failed(LocalStagedTreePublishError::Publish(
                            LocalStagedTreeReservedError::new(error, reservation.clone()),
                        ))
                    }
                }
            },
        )
    }
}

impl AsyncLocalFileSystem {
    /// Commits bytes only when the digest precondition still holds.
    ///
    /// # Type Parameters
    ///
    /// * `D` - Digest algorithm used to validate the destination contents.
    ///
    /// # Parameters
    ///
    /// * `manager` - Operation manager that owns the conditional commit.
    /// * `operation` - Cancellation and deadline policy for the commit.
    /// * `request` - Destination precondition, options, and owned candidate
    ///   bytes.
    /// * `limits` - Per-operation memory, I/O, and resource limits.
    #[cfg(feature = "digest")]
    pub fn submit_conditional_writer_commit<D: digest::Digest + Send + 'static>(
        &self,
        manager: &qubit_operation::OperationManager,
        operation: OperationOptions,
        request: LocalConditionalWriterCommitRequest<D>,
        limits: LocalOperationOptions,
    ) -> LocalConditionalWriterOperation<D> {
        let relative = request.path.is_relative();
        let destination = match (&self.host_base, relative) {
            (Some(base), true) => base.join(&request.path),
            _ => request.path.clone(),
        };
        let owned_capacity = request.bytes.capacity();
        let filesystem = self.filesystem.clone();
        let executor = self.executor.clone();
        self.submit_owned_with_bytes(
            manager,
            operation,
            request,
            OwnedSubmission {
                path_is_relative: relative,

                required_handles: 1,

                limits,

                initial_owned_bytes: owned_capacity,
            },
            move |request, scope, observer, _budget, _maximum| {
                if let Some(stop) = stop_report(scope) {
                    return OperationOutcome::Stopped(stop);
                }
                let reservation = match scope.reserve_resource() {
                    Ok(reservation) => reservation,
                    Err(qubit_operation::ResourceCapacityError::Stopped) => {
                        return OperationOutcome::Stopped(
                            stop_report(scope).expect("scope stopped before conditional writer"),
                        );
                    }
                    Err(error) => {
                        return OperationOutcome::Failed(LocalConditionalWriterCommandError::ResourceCapacity(error));
                    }
                };
                let Some(cleanup_permit) = executor.reserve_cleanup() else {
                    return OperationOutcome::Failed(LocalConditionalWriterCommandError::Open(LocalFileError::new(
                        crate::LocalFileErrorKind::ResourceLimit,
                        crate::LocalFileOperation::OpenWriter,
                    )));
                };
                let resource = LocalManagedAbortResource::new(cleanup_permit, abort_conditional_writer::<D>);
                let writer_resource = match reservation.commit_with(
                    resource,
                    LocalManagedAbortCleanup::<crate::LocalConditionalWriter<D>>::new(executor.cleanup_executor()),
                ) {
                    Ok(resource) => resource,
                    Err(error) => {
                        let (reason, _resource, _cleanup, _reservation) = error.into_parts();
                        return match reason {
                            qubit_operation::ResourceManageErrorKind::Stopped => {
                                OperationOutcome::Stopped(stop_report(scope).expect("scope stopped before writer open"))
                            }
                            reason => {
                                OperationOutcome::Failed(LocalConditionalWriterCommandError::ResourceCommit(reason))
                            }
                        };
                    }
                };
                let resource_id = writer_resource.id();
                *lock_publication_state(&writer_resource.id) = Some(resource_id);
                let writer =
                    match filesystem.open_conditional_writer::<D>(&destination, request.precondition, &request.options)
                    {
                        Ok(writer) => writer,
                        Err(error) => {
                            return OperationOutcome::Failed(LocalConditionalWriterCommandError::Open(error));
                        }
                    };
                *lock_publication_state(&writer_resource.native) = Some(writer);
                let mut offset = 0;
                while offset < request.bytes.len() {
                    if let Some(stop) = stop_report(scope) {
                        return abort_managed_conditional_after_stop(&writer_resource, stop);
                    }
                    let write_result = {
                        let mut native = lock_publication_state(&writer_resource.native);
                        Output::write(
                            native.as_mut().expect("managed conditional writer is available"),
                            &request.bytes[offset..],
                        )
                    };
                    match write_result {
                        Ok(0) => {
                            let error = LocalFileError::new(
                                crate::LocalFileErrorKind::DataCorruption,
                                crate::LocalFileOperation::Commit,
                            )
                            .with_path(destination.clone());
                            return OperationOutcome::Failed(abort_managed_conditional_after_error(
                                &writer_resource,
                                error,
                            ));
                        }
                        Ok(written) => offset += written,
                        Err(source) => {
                            let error = LocalFileError::from_io(
                                crate::LocalFileOperation::Commit,
                                Some(destination.clone()),
                                None,
                                source,
                            );
                            return OperationOutcome::Failed(abort_managed_conditional_after_error(
                                &writer_resource,
                                error,
                            ));
                        }
                    }
                }
                let flush_result = {
                    let mut native = lock_publication_state(&writer_resource.native);
                    Output::flush(native.as_mut().expect("managed conditional writer is available"))
                };
                if let Err(source) = flush_result {
                    let error = LocalFileError::from_io(
                        crate::LocalFileOperation::Commit,
                        Some(destination.clone()),
                        None,
                        source,
                    );
                    return OperationOutcome::Failed(abort_managed_conditional_after_error(&writer_resource, error));
                }
                if let Some(stop) = stop_report(scope) {
                    return abort_managed_conditional_after_stop(&writer_resource, stop);
                }
                #[cfg(feature = "test-support")]
                crate::test_support::hit_test_gate("lf7-conditional-before-publish");
                if let Some(stop) = stop_report(scope) {
                    return abort_managed_conditional_after_stop(&writer_resource, stop);
                }
                let writer = lock_publication_state(&writer_resource.native)
                    .take()
                    .expect("managed conditional writer is available before commit");
                match writer.commit() {
                    Ok(outcome) => {
                        record_writer_effect(
                            scope,
                            &observer,
                            outcome.write_outcome().bytes_written(),
                            LocalPublicationState::Published,
                            if outcome.write_outcome().durable() {
                                LocalDurabilityState::Durable
                            } else {
                                LocalDurabilityState::Unknown
                            },
                        );
                        OperationOutcome::Succeeded(outcome)
                    }
                    Err(error) => {
                        match error.publication() {
                            crate::outcome::LocalConditionalPublicationState::Published => record_writer_effect(
                                scope,
                                &observer,
                                request.bytes.len() as u64,
                                LocalPublicationState::Published,
                                LocalDurabilityState::Failed,
                            ),
                            crate::outcome::LocalConditionalPublicationState::Indeterminate => record_local_effect(
                                scope,
                                &observer,
                                1,
                                0,
                                LocalPublicationState::Unknown,
                                LocalDurabilityState::Unknown,
                            ),
                            _ => {}
                        }
                        OperationOutcome::Failed(LocalConditionalWriterCommandError::Commit(error))
                    }
                }
            },
        )
    }
}

impl AsyncLocalFileSystem {
    /// Serializes temporary keep/persist/seal work on the local worker and
    /// awaits it from the operation future, keeping the resource's existing
    /// identity.
    ///
    /// # Parameters
    ///
    /// * `manager` - Operation manager that owns the transition.
    /// * `operation` - Cancellation and deadline policy for the transition.
    /// * `request` - Existing access token and serialized lifecycle command.
    pub fn submit_temp_lifecycle(
        &self,
        manager: &qubit_operation::OperationManager,
        operation: OperationOptions,
        request: LocalTempLifecycleRequest,
    ) -> Result<
        (
            LocalTempLifecycleOperation,
            super::super::temp::LocalTempFileAccess,
            LocalFactObserver,
        ),
        LocalSubmitError<LocalTempLifecycleRequest>,
    > {
        let scoped = match manager.with_group(&self.executor.inner.group) {
            Ok(scoped) => scoped,
            Err(error) => {
                let rejection = match error {
                    qubit_operation::GroupBindingError::WrongManager => {
                        super::super::LocalSubmitRejection::WrongManager
                    }
                    qubit_operation::GroupBindingError::ScopeEnded => super::super::LocalSubmitRejection::ScopeEnded,
                    qubit_operation::GroupBindingError::Closed => super::super::LocalSubmitRejection::Closed,
                    qubit_operation::GroupBindingError::CapacityExceeded => {
                        super::super::LocalSubmitRejection::CapacityExceeded
                    }
                };
                return Err(LocalSubmitError::new(request, rejection));
            }
        };
        let Some(fact_permit) = self.executor.reserve_fact() else {
            return Err(LocalSubmitError::new(
                request,
                super::super::LocalSubmitRejection::CapacityExceeded,
            ));
        };
        let fact_permit: Arc<dyn Send + Sync> = Arc::new(fact_permit);
        let observer = LocalFactObserver::with_permit(Arc::clone(&fact_permit));
        let observer_for_task = observer.clone();
        let returned_access = request.access.clone_access();
        let request_owner = Arc::new(Mutex::new(Some(request)));
        let request_for_body = Arc::clone(&request_owner);
        let executor = self.executor.clone();
        let submit = scoped.as_async().try_submit(operation, move |scope| async move {
            let request = lock_publication_state(&request_for_body)
                .take()
                .expect("accepted local temp command retains its request");
            if scope.context().stop_reason().is_some() {
                return Err(LocalTempLifecycleError::Stopped);
            }
            let lease_access = request.access.clone_access();
            let context = scope.context();
            let unsupported_operation = match request.command {
                LocalTempLifecycleCommand::Seal { .. } => crate::LocalFileOperation::Commit,
                LocalTempLifecycleCommand::Keep | LocalTempLifecycleCommand::Persist { .. } => {
                    crate::LocalFileOperation::PersistTemp
                }
            };
            let command = request.command;
            let receiver = request.access.enqueue_native_transition(&executor, move |native| {
                #[cfg(feature = "test-support")]
                crate::test_support::hit_test_gate("lf7-temp-before-persist-primitive");
                transition_temp_native(native, command, lease_access, context)
            });
            let receiver = match receiver {
                Ok(receiver) => receiver,
                Err(error) => {
                    return Err(map_temp_command_error(error, unsupported_operation));
                }
            };
            match receiver.await {
                Err(error) => Err(map_temp_command_error(error, unsupported_operation)),
                Ok(Err(error)) => {
                    record_temp_failure(&scope, &observer_for_task, &error);
                    Err(error)
                }
                Ok(Ok((result, publication, durability))) => {
                    record_writer_effect(&scope, &observer_for_task, 0, publication, durability);
                    Ok(result)
                }
            }
        });
        match submit {
            Ok(handle) => Ok((handle, returned_access, observer)),
            Err(error) => {
                let (reason, _body) = error.into_parts();
                let request = lock_publication_state(&request_owner)
                    .take()
                    .expect("rejected temp command returns its original request");
                let rejection = match reason {
                    qubit_operation::AdmissionError::Closed | qubit_operation::AdmissionError::ExecutorUnavailable => {
                        super::super::LocalSubmitRejection::Closed
                    }
                    qubit_operation::AdmissionError::CapacityExceeded | qubit_operation::AdmissionError::Busy => {
                        super::super::LocalSubmitRejection::CapacityExceeded
                    }
                    qubit_operation::AdmissionError::ScopeEnded => super::super::LocalSubmitRejection::ScopeEnded,
                    qubit_operation::AdmissionError::Stopped => super::super::LocalSubmitRejection::Stopped,
                    _ => super::super::LocalSubmitRejection::Closed,
                };
                Err(LocalSubmitError::new(request, rejection))
            }
        }
    }
}

#[cfg(test)]
mod relative_host_publish_tests {
    use std::path::PathBuf;
    use std::sync::Arc;
    use std::sync::Mutex;
    use std::task::Context;
    use std::task::Poll;
    use std::task::Wake;
    use std::task::Waker;

    use qubit_operation::ManagerOptions;
    use qubit_operation::OperationManager;
    use qubit_operation::OperationOutcome;

    use super::AsyncLocalFileSystem;
    use super::lock_publication_state;
    use crate::operation::LocalExecutionConfig;
    use crate::operation::LocalIoExecutor;
    use crate::operation::LocalOperationOptions;
    use crate::operation::LocalPublicationState;
    #[cfg(feature = "digest")]
    use crate::operation::publish::LocalConditionalWriterCommitRequest;
    use crate::operation::publish::LocalStagedTreeFile;
    use crate::operation::publish::LocalStagedTreePublishRequest;
    use crate::operation::publish::LocalTempLifecycleCommand;
    use crate::operation::publish::LocalTempLifecycleError;
    use crate::operation::publish::LocalTempLifecycleRequest;
    use crate::operation::publish::LocalTempLifecycleResult;
    use crate::operation::publish::LocalWriterCommitRequest;
    use crate::operation::submit_temp_file;
    use crate::options::LocalPersistOptions;
    use crate::options::LocalTempDirectoryOptions;
    use crate::options::LocalTempFileOptions;
    use crate::options::LocalTreePublishOptions;
    use crate::options::LocalWriteMode;
    use crate::options::LocalWriteOptions;
    #[cfg(feature = "digest")]
    use crate::options::LocalWritePrecondition;

    fn filesystem(manager: &OperationManager, root: &std::path::Path) -> AsyncLocalFileSystem {
        let executor = LocalIoExecutor::new(manager.clone(), LocalExecutionConfig::default()).unwrap();
        AsyncLocalFileSystem::new(
            crate::LocalFileSystem::host().unwrap(),
            executor,
            Some(root.to_path_buf()),
        )
    }

    fn create_temp_file_access(
        manager: &OperationManager,
        root: &std::path::Path,
    ) -> (crate::operation::temp::LocalTempFileAccess, LocalIoExecutor) {
        let executor = LocalIoExecutor::new(manager.clone(), LocalExecutionConfig::default()).unwrap();
        let mut creation = submit_temp_file(
            crate::LocalFileSystem::host().unwrap(),
            executor.clone(),
            &manager.as_async(),
            Default::default(),
            LocalTempFileOptions::new().with_parent(root),
        )
        .unwrap();
        let access = match block_on(creation.wait_outcome()).unwrap() {
            OperationOutcome::Succeeded(access) => access,
            _ => panic!("temporary file creation failed"),
        };
        (access, executor)
    }

    fn block_on<F: std::future::Future>(future: F) -> F::Output {
        struct ThreadWake(std::thread::Thread);
        impl Wake for ThreadWake {
            fn wake(self: Arc<Self>) {
                self.0.unpark();
            }
            fn wake_by_ref(self: &Arc<Self>) {
                self.0.unpark();
            }
        }

        let waker = Waker::from(Arc::new(ThreadWake(std::thread::current())));
        let mut context = Context::from_waker(&waker);
        let mut future = std::pin::pin!(future);
        loop {
            match future.as_mut().poll(&mut context) {
                Poll::Ready(value) => return value,
                Poll::Pending => std::thread::park(),
            }
        }
    }

    #[test]
    fn test_temp_lifecycle_persist_failure_retains_resource_for_keep() {
        let directory = tempfile::tempdir().unwrap();
        let target = directory.path().join("occupied");
        std::fs::write(&target, b"keep existing target").unwrap();
        let manager = OperationManager::new(ManagerOptions::default()).unwrap();
        let (access, executor) = create_temp_file_access(&manager, directory.path());
        let filesystem = AsyncLocalFileSystem::new(
            crate::LocalFileSystem::host().unwrap(),
            executor,
            Some(directory.path().to_path_buf()),
        );

        let (mut persist, access, facts) = filesystem
            .submit_temp_lifecycle(
                &manager,
                Default::default(),
                LocalTempLifecycleRequest {
                    access,
                    command: LocalTempLifecycleCommand::Persist {
                        target: target.clone(),
                        options: LocalPersistOptions::new(),
                    },
                },
            )
            .unwrap();
        assert!(matches!(
            block_on(persist.wait_outcome()).unwrap(),
            OperationOutcome::Failed(LocalTempLifecycleError::Persist(_))
        ));
        assert_eq!(std::fs::read(&target).unwrap(), b"keep existing target");
        assert_eq!(facts.snapshot().publication, LocalPublicationState::NotPublished);

        let (mut keep, _, facts) = filesystem
            .submit_temp_lifecycle(
                &manager,
                Default::default(),
                LocalTempLifecycleRequest {
                    access,
                    command: LocalTempLifecycleCommand::Keep,
                },
            )
            .unwrap();
        let kept = match block_on(keep.wait_outcome()).unwrap() {
            OperationOutcome::Succeeded(LocalTempLifecycleResult::Persisted(outcome)) => outcome,
            _ => panic!("temporary resource should remain available for keep"),
        };
        assert!(kept.path().exists());
        assert_eq!(facts.snapshot().publication, LocalPublicationState::Published);
        std::fs::remove_file(kept.path()).unwrap();
    }

    #[test]
    fn test_temp_lifecycle_seal_preserves_resource_identity_for_read_lease() {
        let directory = tempfile::tempdir().unwrap();
        let manager = OperationManager::new(ManagerOptions::default()).unwrap();
        let (access, executor) = create_temp_file_access(&manager, directory.path());
        let resource_id = access.resource_id();
        let filesystem = AsyncLocalFileSystem::new(
            crate::LocalFileSystem::host().unwrap(),
            executor.clone(),
            Some(directory.path().to_path_buf()),
        );
        let (mut seal, access, _) = filesystem
            .submit_temp_lifecycle(
                &manager,
                Default::default(),
                LocalTempLifecycleRequest {
                    access,
                    command: LocalTempLifecycleCommand::Seal {
                        options: crate::LocalSealOptions::default(),
                    },
                },
            )
            .unwrap();
        let lease = match block_on(seal.wait_outcome()).unwrap() {
            OperationOutcome::Succeeded(LocalTempLifecycleResult::Sealed(lease)) => lease,
            _ => panic!("temporary file sealing failed"),
        };
        assert_eq!(lease.resource_id(), resource_id);

        let (mut repeated, _, _) = filesystem
            .submit_temp_lifecycle(
                &manager,
                Default::default(),
                LocalTempLifecycleRequest {
                    access,
                    command: LocalTempLifecycleCommand::Keep,
                },
            )
            .unwrap();
        assert!(matches!(
            block_on(repeated.wait_outcome()).unwrap(),
            OperationOutcome::Failed(LocalTempLifecycleError::Unsupported(_))
        ));
        let exported = block_on(lease.export_read_handle(&executor)).unwrap();
        assert_eq!(exported.lease.resource_id(), resource_id);
    }

    #[test]
    fn test_temp_lifecycle_closed_executor_returns_original_resource_request() {
        let directory = tempfile::tempdir().unwrap();
        let manager = OperationManager::new(ManagerOptions::default()).unwrap();
        let (access, executor) = create_temp_file_access(&manager, directory.path());
        let resource_id = access.resource_id();
        let filesystem = AsyncLocalFileSystem::new(
            crate::LocalFileSystem::host().unwrap(),
            executor.clone(),
            Some(directory.path().to_path_buf()),
        );
        let executor_shutdown = executor
            .request_shutdown(qubit_operation::ShutdownMode::Drain)
            .wait()
            .expect("executor group should drain before lifecycle submission");
        assert_eq!(executor_shutdown.snapshot.active_operations, 0);

        let rejection = match filesystem.submit_temp_lifecycle(
            &manager,
            Default::default(),
            LocalTempLifecycleRequest {
                access,
                command: LocalTempLifecycleCommand::Keep,
            },
        ) {
            Err(rejection) => rejection,
            Ok(_) => panic!("a drained executor must reject temp lifecycle work"),
        };
        assert_eq!(rejection.rejection(), crate::operation::LocalSubmitRejection::Closed);
        let request = rejection.into_request();
        assert_eq!(request.access.resource_id(), resource_id);
        drop(request);
        drop(filesystem);
        drop(executor);
        let manager_shutdown = manager
            .request_shutdown(qubit_operation::ShutdownMode::Drain)
            .wait()
            .expect("manager cleanup should finish after rejected resource is dropped");
        assert_eq!(manager_shutdown.snapshot.active_operations, 0);
        assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 0);
    }

    #[test]
    fn test_temp_lifecycle_manager_capacity_rejection_returns_original_request() {
        let directory = tempfile::tempdir().unwrap();
        let mut manager_options = ManagerOptions::default();
        manager_options.limits.active_operations = 1;
        manager_options.limits.queued_operations = 1;
        let manager = OperationManager::new(manager_options).unwrap();
        let (access, executor) = create_temp_file_access(&manager, directory.path());
        let resource_id = access.resource_id();
        let filesystem = AsyncLocalFileSystem::new(
            crate::LocalFileSystem::host().unwrap(),
            executor.clone(),
            Some(directory.path().to_path_buf()),
        );

        let rejection = match filesystem.submit_temp_lifecycle(
            &manager,
            Default::default(),
            LocalTempLifecycleRequest {
                access,
                command: LocalTempLifecycleCommand::Keep,
            },
        ) {
            Err(rejection) => rejection,
            Ok(_) => panic!("an exhausted manager must reject another active operation"),
        };
        assert_eq!(
            rejection.rejection(),
            crate::operation::LocalSubmitRejection::CapacityExceeded
        );
        let request = rejection.into_request();
        assert_eq!(request.access.resource_id(), resource_id);

        drop(request);
        drop(filesystem);
        drop(executor);
        let manager_shutdown = manager
            .request_shutdown(qubit_operation::ShutdownMode::Drain)
            .wait()
            .expect("manager cleanup should finish after rejected resource is dropped");
        assert_eq!(manager_shutdown.snapshot.active_operations, 0);
        assert_eq!(std::fs::read_dir(directory.path()).unwrap().count(), 0);
    }

    #[test]
    #[cfg(panic = "unwind")]
    fn test_publication_state_lock_recovers_owned_state_after_panic() {
        let state = Arc::new(Mutex::new(Some(17_u8)));
        let poison_state = Arc::clone(&state);
        let poisoner = std::thread::spawn(move || {
            let _guard = poison_state.lock().expect("publication state lock");
            panic!("poison publication state lock");
        });
        assert!(poisoner.join().is_err());

        assert_eq!(Some(17), lock_publication_state(&state).take());
        assert!(lock_publication_state(&state).is_none());
    }

    #[test]
    fn test_relative_writer_commit_publishes_under_captured_host_base() {
        let directory = tempfile::tempdir().unwrap();
        let manager = OperationManager::new(ManagerOptions::default()).unwrap();
        let filesystem = filesystem(&manager, directory.path());
        let (mut operation, observer) = filesystem
            .submit_writer_commit(
                &manager,
                Default::default(),
                LocalWriterCommitRequest {
                    path: PathBuf::from("published"),
                    options: LocalWriteOptions::new(LocalWriteMode::CreateOrReplace),
                    bytes: b"relative writer".to_vec(),
                },
                LocalOperationOptions::default(),
            )
            .unwrap();

        assert!(matches!(
            operation.wait_outcome().unwrap(),
            OperationOutcome::Succeeded(_)
        ));
        assert_eq!(
            std::fs::read(directory.path().join("published")).unwrap(),
            b"relative writer"
        );
        assert_eq!(observer.snapshot().publication, LocalPublicationState::Published);
    }

    #[test]
    fn test_writer_commit_with_empty_payload_still_publishes_empty_file() {
        let directory = tempfile::tempdir().unwrap();
        let manager = OperationManager::new(ManagerOptions::default()).unwrap();
        let filesystem = filesystem(&manager, directory.path());
        let (mut operation, observer) = filesystem
            .submit_writer_commit(
                &manager,
                Default::default(),
                LocalWriterCommitRequest {
                    path: PathBuf::from("empty"),
                    options: LocalWriteOptions::new(LocalWriteMode::CreateOrReplace),
                    bytes: Vec::new(),
                },
                LocalOperationOptions::default(),
            )
            .unwrap();

        assert!(matches!(
            operation.wait_outcome().unwrap(),
            OperationOutcome::Succeeded(_)
        ));
        assert_eq!(std::fs::read(directory.path().join("empty")).unwrap(), b"");
        assert_eq!(observer.snapshot().publication, LocalPublicationState::Published);
    }

    #[test]
    fn test_writer_commit_appends_to_existing_file() {
        let directory = tempfile::tempdir().unwrap();
        let destination = directory.path().join("append");
        std::fs::write(&destination, b"before:").unwrap();
        let manager = OperationManager::new(ManagerOptions::default()).unwrap();
        let filesystem = filesystem(&manager, directory.path());
        let (mut operation, observer) = filesystem
            .submit_writer_commit(
                &manager,
                Default::default(),
                LocalWriterCommitRequest {
                    path: PathBuf::from("append"),
                    options: LocalWriteOptions::new(LocalWriteMode::Append),
                    bytes: b"after".to_vec(),
                },
                LocalOperationOptions::default(),
            )
            .unwrap();

        assert!(matches!(
            operation.wait_outcome().unwrap(),
            OperationOutcome::Succeeded(_)
        ));
        assert_eq!(std::fs::read(destination).unwrap(), b"before:after");
        assert_eq!(observer.snapshot().publication, LocalPublicationState::Published);
    }

    #[test]
    fn test_writer_commit_reports_missing_parent_without_publishing() {
        let directory = tempfile::tempdir().unwrap();
        let manager = OperationManager::new(ManagerOptions::default()).unwrap();
        let filesystem = filesystem(&manager, directory.path());
        let (mut operation, observer) = filesystem
            .submit_writer_commit(
                &manager,
                Default::default(),
                LocalWriterCommitRequest {
                    path: PathBuf::from("missing/output"),
                    options: LocalWriteOptions::new(LocalWriteMode::CreateOrReplace),
                    bytes: b"cannot publish".to_vec(),
                },
                LocalOperationOptions::default(),
            )
            .unwrap();

        assert!(matches!(operation.wait_outcome().unwrap(), OperationOutcome::Failed(_)));
        assert!(!directory.path().join("missing/output").exists());
        assert_eq!(observer.snapshot().publication, LocalPublicationState::NotPublished);
    }

    #[test]
    fn test_relative_staged_tree_publish_uses_captured_host_base_as_parent() {
        let directory = tempfile::tempdir().unwrap();
        let manager = OperationManager::new(ManagerOptions::default()).unwrap();
        let filesystem = filesystem(&manager, directory.path());
        let (mut operation, observer) = filesystem
            .submit_staged_tree_publish(
                &manager,
                Default::default(),
                LocalStagedTreePublishRequest {
                    parent: PathBuf::from("."),
                    name: "relative-tree".into(),
                    stage_options: LocalTempDirectoryOptions::new(),
                    files: vec![LocalStagedTreeFile {
                        path: PathBuf::from("item"),
                        bytes: b"relative tree".to_vec(),
                    }],
                    publish_options: LocalTreePublishOptions::new(),
                },
                LocalOperationOptions::default(),
            )
            .unwrap();

        assert!(matches!(
            operation.wait_outcome().unwrap(),
            OperationOutcome::Succeeded(_)
        ));
        assert_eq!(
            std::fs::read(directory.path().join("relative-tree/item")).unwrap(),
            b"relative tree"
        );
        assert_eq!(observer.snapshot().publication, LocalPublicationState::Published);
    }

    #[test]
    fn test_staged_tree_publish_accepts_an_empty_tree() {
        let directory = tempfile::tempdir().unwrap();
        let manager = OperationManager::new(ManagerOptions::default()).unwrap();
        let filesystem = filesystem(&manager, directory.path());
        let (mut operation, observer) = filesystem
            .submit_staged_tree_publish(
                &manager,
                Default::default(),
                LocalStagedTreePublishRequest {
                    parent: PathBuf::from("."),
                    name: "empty-tree".into(),
                    stage_options: LocalTempDirectoryOptions::new(),
                    files: Vec::new(),
                    publish_options: LocalTreePublishOptions::new(),
                },
                LocalOperationOptions::default(),
            )
            .unwrap();

        assert!(matches!(
            operation.wait_outcome().unwrap(),
            OperationOutcome::Succeeded(_)
        ));
        assert!(directory.path().join("empty-tree").is_dir());
        assert_eq!(
            std::fs::read_dir(directory.path().join("empty-tree")).unwrap().count(),
            0
        );
        assert_eq!(observer.snapshot().publication, LocalPublicationState::Published);
    }

    #[test]
    fn test_staged_tree_publish_preserves_existing_destination() {
        let directory = tempfile::tempdir().unwrap();
        let destination = directory.path().join("existing-tree");
        std::fs::create_dir(&destination).unwrap();
        std::fs::write(destination.join("original"), b"keep me").unwrap();
        let manager = OperationManager::new(ManagerOptions::default()).unwrap();
        let filesystem = filesystem(&manager, directory.path());
        let (mut operation, observer) = filesystem
            .submit_staged_tree_publish(
                &manager,
                Default::default(),
                LocalStagedTreePublishRequest {
                    parent: PathBuf::from("."),
                    name: "existing-tree".into(),
                    stage_options: LocalTempDirectoryOptions::new(),
                    files: vec![LocalStagedTreeFile {
                        path: PathBuf::from("replacement"),
                        bytes: b"must not publish".to_vec(),
                    }],
                    publish_options: LocalTreePublishOptions::new(),
                },
                LocalOperationOptions::default(),
            )
            .unwrap();

        assert!(matches!(operation.wait_outcome().unwrap(), OperationOutcome::Failed(_)));
        assert_eq!(std::fs::read(destination.join("original")).unwrap(), b"keep me");
        assert!(!destination.join("replacement").exists());
        assert_eq!(observer.snapshot().publication, LocalPublicationState::NotPublished);
    }

    #[test]
    fn test_staged_tree_publish_rejects_parent_traversal_entry() {
        let directory = tempfile::tempdir().unwrap();
        let manager = OperationManager::new(ManagerOptions::default()).unwrap();
        let filesystem = filesystem(&manager, directory.path());
        let (mut operation, observer) = filesystem
            .submit_staged_tree_publish(
                &manager,
                Default::default(),
                LocalStagedTreePublishRequest {
                    parent: PathBuf::from("."),
                    name: "tree".into(),
                    stage_options: LocalTempDirectoryOptions::new(),
                    files: vec![LocalStagedTreeFile {
                        path: PathBuf::from("../outside"),
                        bytes: b"must remain staged".to_vec(),
                    }],
                    publish_options: LocalTreePublishOptions::new(),
                },
                LocalOperationOptions::default(),
            )
            .unwrap();

        assert!(matches!(operation.wait_outcome().unwrap(), OperationOutcome::Failed(_)));
        assert!(!directory.path().join("tree").exists());
        assert!(!directory.path().join("outside").exists());
        assert_eq!(observer.snapshot().publication, LocalPublicationState::NotPublished);
    }

    #[test]
    fn test_staged_tree_publish_reports_missing_parent_without_publishing() {
        let directory = tempfile::tempdir().unwrap();
        let missing_parent = directory.path().join("missing-parent");
        let manager = OperationManager::new(ManagerOptions::default()).unwrap();
        let filesystem = filesystem(&manager, directory.path());
        let (mut operation, observer) = filesystem
            .submit_staged_tree_publish(
                &manager,
                Default::default(),
                LocalStagedTreePublishRequest {
                    parent: missing_parent.clone(),
                    name: "tree".into(),
                    stage_options: LocalTempDirectoryOptions::new(),
                    files: vec![LocalStagedTreeFile {
                        path: PathBuf::from("item"),
                        bytes: b"must not publish".to_vec(),
                    }],
                    publish_options: LocalTreePublishOptions::new(),
                },
                LocalOperationOptions::default(),
            )
            .unwrap();

        assert!(matches!(operation.wait_outcome().unwrap(), OperationOutcome::Failed(_)));
        assert!(!missing_parent.exists());
        assert_eq!(observer.snapshot().publication, LocalPublicationState::NotPublished);
    }

    #[test]
    fn test_staged_tree_publish_rejects_disk_limit_before_submission() {
        let directory = tempfile::tempdir().unwrap();
        let manager = OperationManager::new(ManagerOptions::default()).unwrap();
        let filesystem = filesystem(&manager, directory.path());
        let request = LocalStagedTreePublishRequest {
            parent: PathBuf::from("."),
            name: "limited-tree".into(),
            stage_options: LocalTempDirectoryOptions::new(),
            files: vec![LocalStagedTreeFile {
                path: PathBuf::from("item"),
                bytes: b"payload".to_vec(),
            }],
            publish_options: LocalTreePublishOptions::new(),
        };

        let submission = filesystem.submit_staged_tree_publish(
            &manager,
            Default::default(),
            request,
            LocalOperationOptions {
                max_disk_bytes: Some(1),
                ..LocalOperationOptions::default()
            },
        );
        let rejected = match submission {
            Err(error) => error,
            Ok(_) => panic!("disk limit below the staged payload must reject submission"),
        };

        assert_eq!(
            rejected.rejection(),
            crate::operation::LocalSubmitRejection::CapacityExceeded
        );
        assert!(!directory.path().join("limited-tree").exists());
    }

    #[cfg(feature = "digest")]
    #[test]
    fn test_relative_conditional_writer_commit_publishes_under_captured_host_base() {
        use sha2::Sha256;

        let directory = tempfile::tempdir().unwrap();
        let manager = OperationManager::new(ManagerOptions::default()).unwrap();
        let filesystem = filesystem(&manager, directory.path())
            .with_symlink_policy(crate::policy::LocalSymlinkPolicy::Reject)
            .unwrap();
        let (mut operation, observer) = filesystem
            .submit_conditional_writer_commit::<Sha256>(
                &manager,
                Default::default(),
                LocalConditionalWriterCommitRequest {
                    path: PathBuf::from("conditional"),
                    precondition: LocalWritePrecondition::Absent,
                    options: LocalWriteOptions::new(LocalWriteMode::CreateOrReplace),
                    bytes: b"relative conditional".to_vec(),
                },
                LocalOperationOptions::default(),
            )
            .unwrap();

        assert!(matches!(
            operation.wait_outcome().unwrap(),
            OperationOutcome::Succeeded(_)
        ));
        assert_eq!(
            std::fs::read(directory.path().join("conditional")).unwrap(),
            b"relative conditional"
        );
        assert_eq!(observer.snapshot().publication, LocalPublicationState::Published);
    }

    #[cfg(feature = "digest")]
    #[test]
    fn test_conditional_writer_rejects_absent_precondition_when_destination_exists() {
        use sha2::Sha512;

        let directory = tempfile::tempdir().unwrap();
        let destination = directory.path().join("already-present");
        std::fs::write(&destination, b"original").unwrap();
        let manager = OperationManager::new(ManagerOptions::default()).unwrap();
        let filesystem = filesystem(&manager, directory.path())
            .with_symlink_policy(crate::policy::LocalSymlinkPolicy::Reject)
            .unwrap();
        let (mut operation, observer) = filesystem
            .submit_conditional_writer_commit::<Sha512>(
                &manager,
                Default::default(),
                LocalConditionalWriterCommitRequest {
                    path: PathBuf::from("already-present"),
                    precondition: LocalWritePrecondition::Absent,
                    options: LocalWriteOptions::new(LocalWriteMode::CreateOrReplace),
                    bytes: b"replacement".to_vec(),
                },
                LocalOperationOptions::default(),
            )
            .unwrap();

        assert!(matches!(operation.wait_outcome().unwrap(), OperationOutcome::Failed(_)));
        assert_eq!(std::fs::read(destination).unwrap(), b"original");
        assert_eq!(observer.snapshot().publication, LocalPublicationState::NotPublished);
    }

    #[cfg(feature = "test-support")]
    #[cfg(feature = "digest")]
    #[cfg(unix)]
    #[test]
    fn test_managed_conditional_write_error_aborts_candidate_and_preserves_destination() {
        use sha2::Digest;
        use sha2::Sha256;

        use crate::test_support::TestConditionalHooks;
        use crate::test_support::TestConditionalPoint;

        let directory = tempfile::tempdir().unwrap();
        let destination = directory.path().join("conditional-write-error");
        std::fs::write(&destination, b"original").unwrap();
        let manager = OperationManager::new(ManagerOptions::default()).unwrap();
        let executor = LocalIoExecutor::new(manager.clone(), LocalExecutionConfig::default()).unwrap();
        let filesystem = AsyncLocalFileSystem::new(
            crate::LocalFileSystem::host()
                .unwrap()
                .with_conditional_test_hooks(TestConditionalHooks::once(TestConditionalPoint::BeforeWrite, || {
                    Err(std::io::Error::new(
                        std::io::ErrorKind::PermissionDenied,
                        "injected write failure",
                    ))
                })),
            executor,
            Some(directory.path().to_path_buf()),
        )
        .with_symlink_policy(crate::policy::LocalSymlinkPolicy::Reject)
        .unwrap();
        let (mut operation, observer) = filesystem
            .submit_conditional_writer_commit::<Sha256>(
                &manager,
                Default::default(),
                LocalConditionalWriterCommitRequest {
                    path: PathBuf::from("conditional-write-error"),
                    precondition: LocalWritePrecondition::Content(Sha256::digest(b"original")),
                    options: LocalWriteOptions::new(LocalWriteMode::CreateOrReplace),
                    bytes: b"replacement".to_vec(),
                },
                LocalOperationOptions::default(),
            )
            .unwrap();

        match operation.wait_outcome().unwrap() {
            OperationOutcome::Failed(crate::LocalConditionalWriterCommandError::Write {
                cause,
                cleanup_error: None,
                recovery_resource: None,
                retryable_writer: None,
            }) => {
                assert_eq!(cause.operation(), crate::LocalFileOperation::Commit);
                assert_eq!(cause.kind(), crate::LocalFileErrorKind::PermissionDenied);
            }
            other => panic!("expected managed conditional write failure, got {other:?}"),
        }
        assert_eq!(std::fs::read(destination).unwrap(), b"original");
        assert_eq!(observer.snapshot().publication, LocalPublicationState::NotPublished);
    }
}
