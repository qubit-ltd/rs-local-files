// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

//! Owned publication commands for local writers.

use std::sync::Arc;

use qubit_operation::DiagnosticSummary;
use qubit_operation::EffectFact;
use qubit_operation::EffectRecord;
use qubit_operation::OperationOutcome;
use qubit_operation::ResourceRef;
use qubit_operation::StopReport;

use super::LocalDurabilityState;
use super::LocalFactObserver;
use super::LocalFactSnapshot;
use super::LocalPublicationState;
use super::handoff::LocalReadLease;
use super::temp::LocalTempCommandError;
use super::temp::LocalTempNative;
use crate::LocalFileError;
use crate::LocalWriteOutcome;

#[path = "publish/local_writer_command_error.rs"]
mod local_writer_command_error;
pub use local_writer_command_error::LocalWriterCommandError;

#[path = "publish/local_writer_commit_request.rs"]
mod local_writer_commit_request;
pub use local_writer_commit_request::LocalWriterCommitRequest;
#[path = "publish/local_abort_cleanup_wait.rs"]
mod local_abort_cleanup_wait;
#[path = "publish/local_managed_abort_cleanup_policy.rs"]
mod local_managed_abort_cleanup_policy;
pub(in crate::operation) use local_abort_cleanup_wait::LocalAbortCleanupWait;
#[path = "publish/local_abort_cleanup_state.rs"]
mod local_abort_cleanup_state;
pub(in crate::operation) use local_abort_cleanup_state::LocalAbortCleanupState;
#[path = "publish/local_managed_abort_cleanup.rs"]
mod local_managed_abort_cleanup;
pub(in crate::operation) use local_managed_abort_cleanup::LocalManagedAbortCleanup;
#[path = "publish/local_managed_abort_resource.rs"]
mod local_managed_abort_resource;
pub(in crate::operation) use local_managed_abort_resource::LocalManagedAbortResource;
#[path = "publish/local_staged_tree_recovery.rs"]
mod local_staged_tree_recovery;
pub use local_staged_tree_recovery::LocalStagedTreeRecovery;
#[path = "publish/local_staged_tree_reserved_error.rs"]
mod local_staged_tree_reserved_error;
pub use local_staged_tree_reserved_error::LocalStagedTreeReservedError;
#[path = "publish/local_staged_tree_disk_reservation.rs"]
mod local_staged_tree_disk_reservation;
pub use local_staged_tree_disk_reservation::LocalStagedTreeDiskReservation;

#[path = "publish/local_staged_tree_file.rs"]
mod local_staged_tree_file;
pub use local_staged_tree_file::LocalStagedTreeFile;
#[path = "publish/local_staged_tree_publish_request.rs"]
mod local_staged_tree_publish_request;
pub use local_staged_tree_publish_request::LocalStagedTreePublishRequest;
#[path = "publish/local_staged_tree_publish_error.rs"]
mod local_staged_tree_publish_error;
pub use local_staged_tree_publish_error::LocalStagedTreePublishError;
#[cfg(feature = "digest")]
#[path = "publish/local_conditional_writer_command_error.rs"]
mod local_conditional_writer_command_error;
#[cfg(feature = "digest")]
pub use local_conditional_writer_command_error::LocalConditionalWriterCommandError;
#[cfg(feature = "digest")]
#[path = "publish/local_conditional_writer_commit_request.rs"]
mod local_conditional_writer_commit_request;
#[cfg(feature = "digest")]
pub use local_conditional_writer_commit_request::LocalConditionalWriterCommitRequest;
#[path = "publish/local_temp_lifecycle_command.rs"]
mod local_temp_lifecycle_command;
pub use local_temp_lifecycle_command::LocalTempLifecycleCommand;
#[path = "publish/local_temp_lifecycle_result.rs"]
mod local_temp_lifecycle_result;
pub use local_temp_lifecycle_result::LocalTempLifecycleResult;
#[path = "publish/local_temp_persist_failure.rs"]
mod local_temp_persist_failure;
pub use local_temp_persist_failure::LocalTempPersistFailure;
#[path = "publish/local_temp_lifecycle_error.rs"]
mod local_temp_lifecycle_error;
pub use local_temp_lifecycle_error::LocalTempLifecycleError;
#[path = "publish/local_temp_worker_command_failure.rs"]
mod local_temp_worker_command_failure;
pub use local_temp_worker_command_failure::LocalTempWorkerCommandFailure;
#[path = "publish/local_temp_lifecycle_operation.rs"]
mod local_temp_lifecycle_operation;
pub use local_temp_lifecycle_operation::LocalTempLifecycleOperation;
#[path = "publish/local_temp_lifecycle_request.rs"]
mod local_temp_lifecycle_request;
pub use local_temp_lifecycle_request::LocalTempLifecycleRequest;
#[path = "publish/local_temp_native_transition.rs"]
mod local_temp_native_transition;
use local_temp_native_transition::LocalTempNativeTransition;
#[cfg(feature = "digest")]
#[path = "publish/local_conditional_writer_operation.rs"]
mod local_conditional_writer_operation;
#[cfg(feature = "digest")]
pub use local_conditional_writer_operation::LocalConditionalWriterOperation;

/// Builds the cleanup failure returned when a native writer cannot abort.
///
/// # Type Parameters
///
/// * `W` - Native writer retained by the managed cleanup owner.
///
/// # Parameters
///
/// * `resource` - Managed owner retained for a later cleanup attempt.
/// * `summary` - Stable description of the cleanup failure.
/// * `source` - Optional native error retained as the failure source.
pub(super) fn local_abort_cleanup_failure<W: Send + 'static>(
    resource: &LocalManagedAbortResource<W>,
    summary: &str,
    source: Option<Arc<dyn std::error::Error + Send + Sync>>,
) -> qubit_operation::CleanupFailure {
    qubit_operation::CleanupFailure {
        resource_id: resource
            .id
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .expect("managed local writer has a resource id before cleanup"),
        stage: qubit_operation::CleanupStage::Drop,
        summary: DiagnosticSummary::new(summary),
        source,
    }
}

/// Aborts a native writer and discards its completion outcome.
///
/// # Parameters
///
/// * `writer` - Native writer whose unpublished staging must be aborted.
pub(super) fn abort_managed_file_writer(writer: &mut crate::LocalFileWriter) -> Result<(), LocalFileError> {
    writer.abort().map(|_| ())
}

/// Aborts a conditional writer using its native cleanup path.
///
/// # Type Parameters
///
/// * `D` - Digest algorithm used by the writer's precondition.
///
/// # Parameters
///
/// * `writer` - Conditional writer whose candidate publication is abandoned.
#[cfg(feature = "digest")]
pub(super) fn abort_conditional_writer<D: digest::Digest + Send + 'static>(
    writer: &mut crate::LocalConditionalWriter<D>,
) -> Result<(), LocalFileError> {
    #[cfg(feature = "test-support")]
    if crate::local::test_support_enabled("lf7-conditional-abort") {
        return Err(LocalFileError::from_io(
            crate::LocalFileOperation::Abort,
            None,
            None,
            crate::local::test_fault_error(),
        ));
    }
    writer.abort().map(|_| ())
}

/// Converts a failed conditional write into a structured error after aborting.
///
/// # Type Parameters
///
/// * `D` - Digest algorithm used by the conditional writer.
///
/// # Parameters
///
/// * `resource` - Managed writer owner retained by the operation scope.
/// * `cause` - Original write or flush failure.
#[cfg(feature = "digest")]
pub(super) fn abort_managed_conditional_after_error<D: digest::Digest + Send + 'static>(
    resource: &ResourceRef<LocalManagedAbortResource<crate::LocalConditionalWriter<D>>>,
    cause: LocalFileError,
) -> LocalConditionalWriterCommandError<D> {
    let mut native = resource.native.lock().unwrap_or_else(|error| error.into_inner());
    let writer = native.as_mut().expect("managed conditional writer is available");
    match (resource.abort)(writer) {
        Ok(()) => {
            native.take();
            LocalConditionalWriterCommandError::Write {
                cause,
                cleanup_error: None,
                recovery_resource: None,
                retryable_writer: None,
            }
        }
        Err(cleanup_error) => LocalConditionalWriterCommandError::Write {
            cause,
            cleanup_error: Some(cleanup_error),
            recovery_resource: Some(resource.id()),
            retryable_writer: None,
        },
    }
}

/// Aborts a conditional writer when cancellation stops publication.
///
/// # Type Parameters
///
/// * `D` - Digest algorithm used by the conditional writer.
///
/// # Parameters
///
/// * `resource` - Managed writer owner retained by the operation scope.
/// * `stop` - Cancellation or deadline report returned to the caller.
#[cfg(feature = "digest")]
pub(super) fn abort_managed_conditional_after_stop<D: digest::Digest + Send + 'static>(
    resource: &ResourceRef<LocalManagedAbortResource<crate::LocalConditionalWriter<D>>>,
    stop: StopReport,
) -> OperationOutcome<crate::outcome::LocalConditionalWriteOutcome<D>, LocalConditionalWriterCommandError<D>> {
    let mut native = resource.native.lock().unwrap_or_else(|error| error.into_inner());
    let Some(writer) = native.as_mut() else {
        return OperationOutcome::Stopped(stop);
    };
    if (resource.abort)(writer).is_ok() {
        native.take();
    }
    OperationOutcome::Stopped(stop)
}

/// Aborts a managed writer after I/O failure and preserves recovery facts.
///
/// # Parameters
///
/// * `resource` - Operation-managed native writer owner.
/// * `cause` - Original write or flush failure.
/// * `accepted_bytes` - Bytes accepted before the failure boundary.
/// * `scope` - Operation scope used to record confirmed side effects.
/// * `observer` - Shared publication facts for this operation.
/// * `effect_already_recorded` - Whether append facts were recorded earlier.
pub(super) fn abort_managed_writer_after_error(
    resource: &ResourceRef<LocalManagedAbortResource<crate::LocalFileWriter>>,
    cause: LocalFileError,
    accepted_bytes: u64,
    scope: &qubit_operation::OperationScope,
    observer: &LocalFactObserver,
    effect_already_recorded: bool,
) -> LocalWriterCommandError {
    let mut native = resource.native.lock().unwrap_or_else(|error| error.into_inner());
    let writer = native.as_mut().expect("managed writer is available");
    match writer.abort() {
        Ok(outcome) => {
            native.take();
            if !effect_already_recorded && outcome.failure_state() == Some(crate::LocalWriteFailureState::Published) {
                record_writer_effect(
                    scope,
                    observer,
                    outcome.bytes_written(),
                    LocalPublicationState::Published,
                    LocalDurabilityState::Unknown,
                );
            }
            LocalWriterCommandError::Write {
                cause,
                cleanup_error: None,
                retryable_writer: None,
                recovery_resource: None,
            }
        }
        Err(cleanup_error) => {
            if effect_already_recorded {
                return LocalWriterCommandError::Write {
                    cause,
                    cleanup_error: Some(Box::new(cleanup_error)),
                    retryable_writer: None,
                    recovery_resource: Some(resource.id()),
                };
            }
            match writer.failure_state() {
                Some(crate::LocalWriteFailureState::Published) => record_writer_effect(
                    scope,
                    observer,
                    accepted_bytes,
                    LocalPublicationState::Published,
                    LocalDurabilityState::Unknown,
                ),
                Some(crate::LocalWriteFailureState::Indeterminate) => record_writer_effect(
                    scope,
                    observer,
                    accepted_bytes,
                    LocalPublicationState::Unknown,
                    LocalDurabilityState::Unknown,
                ),
                _ => record_stage_write(scope, observer, accepted_bytes, EffectFact::Unknown),
            }
            LocalWriterCommandError::Write {
                cause,
                cleanup_error: Some(Box::new(cleanup_error)),
                retryable_writer: None,
                recovery_resource: Some(resource.id()),
            }
        }
    }
}

/// Aborts a managed writer when cancellation wins before publication.
///
/// # Parameters
///
/// * `resource` - Operation-managed native writer owner.
/// * `stop` - Cancellation or deadline report returned to the caller.
/// * `accepted_bytes` - Bytes accepted before the stop boundary.
/// * `scope` - Operation scope used to record confirmed side effects.
/// * `observer` - Shared publication facts for this operation.
/// * `effect_already_recorded` - Whether append facts were recorded earlier.
pub(super) fn abort_managed_writer_after_stop(
    resource: &ResourceRef<LocalManagedAbortResource<crate::LocalFileWriter>>,
    stop: StopReport,
    accepted_bytes: u64,
    scope: &qubit_operation::OperationScope,
    observer: &LocalFactObserver,
    effect_already_recorded: bool,
) -> OperationOutcome<LocalWriteOutcome, LocalWriterCommandError> {
    let mut native = resource.native.lock().unwrap_or_else(|error| error.into_inner());
    let Some(writer) = native.as_mut() else {
        return OperationOutcome::Stopped(stop);
    };
    match writer.abort() {
        Ok(outcome) => {
            native.take();
            if !effect_already_recorded && outcome.failure_state() == Some(crate::LocalWriteFailureState::Published) {
                record_writer_effect(
                    scope,
                    observer,
                    outcome.bytes_written(),
                    LocalPublicationState::Published,
                    LocalDurabilityState::Unknown,
                );
            }
            OperationOutcome::Stopped(stop)
        }
        Err(_cleanup_error) => {
            if effect_already_recorded {
                return OperationOutcome::Stopped(stop);
            }
            match writer.failure_state() {
                Some(crate::LocalWriteFailureState::Published) => record_writer_effect(
                    scope,
                    observer,
                    accepted_bytes,
                    LocalPublicationState::Published,
                    LocalDurabilityState::Unknown,
                ),
                Some(crate::LocalWriteFailureState::Indeterminate) => record_writer_effect(
                    scope,
                    observer,
                    accepted_bytes,
                    LocalPublicationState::Unknown,
                    LocalDurabilityState::Unknown,
                ),
                _ => {}
            }
            OperationOutcome::Stopped(stop)
        }
    }
}

/// Records publication and durability facts for a completed writer action.
///
/// # Parameters
///
/// * `scope` - Operation scope receiving the effect record.
/// * `observer` - Snapshot updated with the same confirmed facts.
/// * `bytes` - Number of modified bytes observed by the worker.
/// * `publication` - Publication state confirmed by the operation.
/// * `durability` - Durability state confirmed by the operation.
pub(super) fn record_writer_effect(
    scope: &qubit_operation::OperationScope,
    observer: &LocalFactObserver,
    bytes: u64,
    publication: LocalPublicationState,
    durability: LocalDurabilityState,
) {
    record_local_effect(scope, observer, 1, bytes, publication, durability);
}

/// Records append bytes without duplicating a previously registered effect.
///
/// # Parameters
///
/// * `scope` - Operation scope receiving the initial effect record.
/// * `observer` - Snapshot updated with confirmed append facts.
/// * `bytes` - Newly written byte count.
/// * `effect_recorded` - Tracks whether this operation already registered an
///   effect.
pub(super) fn record_append_writer_bytes(
    scope: &qubit_operation::OperationScope,
    observer: &LocalFactObserver,
    bytes: u64,
    effect_recorded: &mut bool,
) {
    if *effect_recorded {
        update_writer_effect_observer(
            observer,
            0,
            bytes,
            LocalPublicationState::Published,
            LocalDurabilityState::Unknown,
        );
    } else {
        record_writer_effect(
            scope,
            observer,
            bytes,
            LocalPublicationState::Published,
            LocalDurabilityState::Unknown,
        );
        *effect_recorded = true;
    }
}

/// Updates the observer after an effect record has already been written.
///
/// # Parameters
///
/// * `observer` - Snapshot receiving publication facts.
/// * `modified_count` - Number of confirmed modifications to add.
/// * `bytes` - Confirmed bytes to add.
/// * `publication` - Publication state after the worker action.
/// * `durability` - Durability state after the worker action.
pub(super) fn update_writer_effect_observer(
    observer: &LocalFactObserver,
    modified_count: u64,
    bytes: u64,
    publication: LocalPublicationState,
    durability: LocalDurabilityState,
) {
    let mut snapshot: LocalFactSnapshot = observer.snapshot();
    snapshot.modified_count = snapshot.modified_count.saturating_add(modified_count);
    snapshot.modified_bytes = snapshot.modified_bytes.saturating_add(bytes);
    snapshot.modified_range = None;
    snapshot.range_unknown = true;
    snapshot.publication = publication;
    snapshot.durability = durability;
    observer.update(snapshot);
}

/// Records an effect and synchronizes the observer's publication facts.
///
/// # Parameters
///
/// * `scope` - Operation scope receiving the effect record.
/// * `observer` - Snapshot updated with operation facts.
/// * `modified_count` - Number of filesystem entries modified.
/// * `bytes` - Number of bytes modified.
/// * `publication` - Publication state established by the operation.
/// * `durability` - Durability state established by the operation.
pub(super) fn record_local_effect(
    scope: &qubit_operation::OperationScope,
    observer: &LocalFactObserver,
    modified_count: u64,
    bytes: u64,
    publication: LocalPublicationState,
    durability: LocalDurabilityState,
) {
    let effect_record_error = scope
        .record_effect(EffectRecord {
            operation_id: scope.id(),
            resource_id: None,
            fact: if publication == LocalPublicationState::Unknown {
                EffectFact::Unknown
            } else {
                EffectFact::Known
            },
            description: DiagnosticSummary::new("local writer publication"),
        })
        .err();
    update_writer_effect_observer(observer, modified_count, bytes, publication, durability);
    if let Some(effect_record_error) = effect_record_error {
        let mut snapshot = observer.snapshot();
        snapshot.effect_record_error = Some(effect_record_error);
        observer.update(snapshot);
    }
}

/// Records a write to a staged tree that has not yet been published.
///
/// # Parameters
///
/// * `scope` - Operation scope receiving the stage-write effect.
/// * `observer` - Snapshot updated with unpublished write facts.
/// * `bytes` - Bytes written to the staged file.
/// * `fact` - Certainty of the worker's write result.
pub(super) fn record_stage_write(
    scope: &qubit_operation::OperationScope,
    observer: &LocalFactObserver,
    bytes: u64,
    fact: EffectFact,
) {
    let effect_record_error = scope
        .record_effect(EffectRecord {
            operation_id: scope.id(),
            resource_id: None,
            fact,
            description: DiagnosticSummary::new("local staged-tree file write"),
        })
        .err();
    let mut snapshot = observer.snapshot();
    snapshot.modified_count = snapshot.modified_count.saturating_add(1);
    snapshot.modified_bytes = snapshot.modified_bytes.saturating_add(bytes);
    snapshot.modified_range = None;
    snapshot.range_unknown |= fact == EffectFact::Unknown;
    snapshot.publication = LocalPublicationState::NotPublished;
    snapshot.durability = LocalDurabilityState::Unknown;
    snapshot.effect_record_error = effect_record_error.or(snapshot.effect_record_error);
    observer.update(snapshot);
}

/// Captures the scope's stop reason for a stopped operation result.
///
/// # Parameters
///
/// * `scope` - Operation scope whose cancellation state is inspected.
pub(super) fn stop_report(scope: &qubit_operation::OperationScope) -> Option<StopReport> {
    scope.context().stop_reason().map(|reason| StopReport {
        reason,
        accepted_at: None,
    })
}

/// Maps internal worker admission errors to a stable lifecycle error value.
///
/// # Parameters
///
/// * `error` - Internal worker result for a temporary-resource command.
pub(super) fn map_temp_worker_error(error: LocalTempCommandError) -> LocalTempWorkerCommandFailure {
    match error {
        LocalTempCommandError::Capacity => LocalTempWorkerCommandFailure::Capacity,
        LocalTempCommandError::ResourceUnavailable => LocalTempWorkerCommandFailure::ResourceUnavailable,
        LocalTempCommandError::UnsupportedKind => LocalTempWorkerCommandFailure::ResourceUnavailable,
        LocalTempCommandError::Panicked => LocalTempWorkerCommandFailure::Panicked,
    }
}

/// Maps temporary worker results into lifecycle errors for the caller.
///
/// # Parameters
///
/// * `error` - Internal worker result for the lifecycle command.
/// * `operation` - Filesystem operation used to classify unsupported kinds.
pub(super) fn map_temp_command_error(
    error: LocalTempCommandError,
    operation: crate::LocalFileOperation,
) -> LocalTempLifecycleError {
    if error == LocalTempCommandError::UnsupportedKind {
        LocalTempLifecycleError::Unsupported(LocalFileError::new(crate::LocalFileErrorKind::Unsupported, operation))
    } else {
        LocalTempLifecycleError::Worker(map_temp_worker_error(error))
    }
}

/// Applies one lifecycle command and returns the retained owner on failure.
///
/// # Parameters
///
/// * `native` - Managed temporary file or directory being transitioned.
/// * `command` - Keep, persist, or seal action requested by the caller.
/// * `lease_access` - Access token retained if sealing creates a read lease.
pub(super) fn transition_temp_native(
    native: LocalTempNative,
    command: LocalTempLifecycleCommand,
    lease_access: super::LocalTempFileAccess,
    context: qubit_operation::OperationContext,
) -> LocalTempNativeTransition {
    if context.stop_reason().is_some() {
        return Err((native, LocalTempLifecycleError::Stopped));
    }
    match (native, command) {
        (LocalTempNative::File(file), LocalTempLifecycleCommand::Keep) => match file.keep() {
            Ok(outcome) => {
                let durability = if outcome.durable() {
                    LocalDurabilityState::Durable
                } else {
                    LocalDurabilityState::Unknown
                };
                Ok((
                    None,
                    (
                        LocalTempLifecycleResult::Persisted(Box::new(outcome)),
                        LocalPublicationState::Published,
                        durability,
                    ),
                ))
            }
            Err(error) => Err(persist_failure(error, LocalTempNative::File)),
        },
        (LocalTempNative::File(file), LocalTempLifecycleCommand::Persist { target, options }) => {
            match file.persist_with(&target, options) {
                Ok(outcome) => {
                    let durability = if outcome.durable() {
                        LocalDurabilityState::Durable
                    } else {
                        LocalDurabilityState::Unknown
                    };
                    Ok((
                        None,
                        (
                            LocalTempLifecycleResult::Persisted(Box::new(outcome)),
                            LocalPublicationState::Published,
                            durability,
                        ),
                    ))
                }
                Err(error) => Err(persist_failure(error, LocalTempNative::File)),
            }
        }
        (LocalTempNative::File(file), LocalTempLifecycleCommand::Seal { options }) => {
            match file.seal_with_options(&options) {
                Ok(sealed) => Ok((
                    Some(LocalTempNative::Sealed(sealed)),
                    (
                        LocalTempLifecycleResult::Sealed(LocalReadLease::from_temp_access(&lease_access, false)),
                        LocalPublicationState::NotPublished,
                        LocalDurabilityState::Unknown,
                    ),
                )),
                Err(error) => {
                    let (cause, access_state, file) = error.into_parts();
                    Err((
                        LocalTempNative::File(file),
                        LocalTempLifecycleError::Seal {
                            error: cause,
                            access_state,
                        },
                    ))
                }
            }
        }
        (LocalTempNative::Directory(directory), LocalTempLifecycleCommand::Keep) => match directory.keep() {
            Ok(outcome) => {
                let durability = if outcome.durable() {
                    LocalDurabilityState::Durable
                } else {
                    LocalDurabilityState::Unknown
                };
                Ok((
                    None,
                    (
                        LocalTempLifecycleResult::Persisted(Box::new(outcome)),
                        LocalPublicationState::Published,
                        durability,
                    ),
                ))
            }
            Err(error) => Err(persist_failure(error, LocalTempNative::Directory)),
        },
        (LocalTempNative::Directory(directory), LocalTempLifecycleCommand::Persist { target, options }) => {
            match directory.persist_with(&target, options) {
                Ok(outcome) => {
                    let durability = if outcome.durable() {
                        LocalDurabilityState::Durable
                    } else {
                        LocalDurabilityState::Unknown
                    };
                    Ok((
                        None,
                        (
                            LocalTempLifecycleResult::Persisted(Box::new(outcome)),
                            LocalPublicationState::Published,
                            durability,
                        ),
                    ))
                }
                Err(error) => Err(persist_failure(error, LocalTempNative::Directory)),
            }
        }
        (native, LocalTempLifecycleCommand::Seal { .. }) => Err((
            native,
            LocalTempLifecycleError::Unsupported(LocalFileError::new(
                crate::LocalFileErrorKind::Unsupported,
                crate::LocalFileOperation::Commit,
            )),
        )),
        (native, _) => Err((
            native,
            LocalTempLifecycleError::Unsupported(LocalFileError::new(
                crate::LocalFileErrorKind::Unsupported,
                crate::LocalFileOperation::PersistTemp,
            )),
        )),
    }
}

/// Converts a failed persistence attempt while retaining its native resource.
///
/// # Type Parameters
///
/// * `T` - Native temporary resource type returned by the failed operation.
///
/// # Parameters
///
/// * `error` - Lossless native persistence error and recovery state.
/// * `wrap` - Converts the retained native owner into the lifecycle enum.
fn persist_failure<T>(
    error: crate::local::LocalPersistError<T>,
    wrap: impl FnOnce(T) -> LocalTempNative,
) -> (LocalTempNative, LocalTempLifecycleError) {
    let parts = error.into_parts();
    let native = wrap(parts.resource);
    let failure = LocalTempPersistFailure {
        error: parts.error,
        requested_target: parts.requested_target,
        resolved_target: parts.resolved_target,
        stage: parts.stage,
        state: parts.state,
        source_state: parts.source_state,
    };
    (native, LocalTempLifecycleError::Persist(failure))
}

/// Records publication facts established by a failed temporary transition.
///
/// # Parameters
///
/// * `scope` - Operation scope receiving the effect record.
/// * `observer` - Snapshot updated with transition facts.
/// * `error` - Structured failure carrying the observed publication state.
pub(super) fn record_temp_failure(
    scope: &qubit_operation::OperationScope,
    observer: &LocalFactObserver,
    error: &LocalTempLifecycleError,
) {
    match error {
        LocalTempLifecycleError::Persist(failure) => match failure.state {
            crate::outcome::LocalPersistFailureState::Published => record_writer_effect(
                scope,
                observer,
                0,
                LocalPublicationState::Published,
                LocalDurabilityState::Failed,
            ),
            crate::outcome::LocalPersistFailureState::Indeterminate => record_writer_effect(
                scope,
                observer,
                0,
                LocalPublicationState::Unknown,
                LocalDurabilityState::Unknown,
            ),
            _ => {}
        },
        LocalTempLifecycleError::Seal { .. } => record_stage_write(scope, observer, 0, EffectFact::Known),
        _ => {}
    }
}

#[cfg(test)]
mod temp_error_mapping_tests {
    use super::LocalTempCommandError;
    use super::LocalTempLifecycleError;
    use super::LocalTempWorkerCommandFailure;
    use super::map_temp_command_error;
    use super::map_temp_worker_error;

    #[test]
    fn test_temp_worker_errors_map_to_stable_failures() {
        let cases = [
            (LocalTempCommandError::Capacity, LocalTempWorkerCommandFailure::Capacity),
            (
                LocalTempCommandError::ResourceUnavailable,
                LocalTempWorkerCommandFailure::ResourceUnavailable,
            ),
            (
                LocalTempCommandError::UnsupportedKind,
                LocalTempWorkerCommandFailure::ResourceUnavailable,
            ),
            (LocalTempCommandError::Panicked, LocalTempWorkerCommandFailure::Panicked),
        ];

        for (error, expected) in cases {
            assert_eq!(map_temp_worker_error(error), expected);
        }
    }

    #[test]
    fn test_temp_command_errors_classify_unsupported_and_worker_failures() {
        let operation = crate::LocalFileOperation::PersistTemp;
        let unsupported = map_temp_command_error(LocalTempCommandError::UnsupportedKind, operation);
        match unsupported {
            LocalTempLifecycleError::Unsupported(error) => {
                assert_eq!(error.kind(), crate::LocalFileErrorKind::Unsupported);
                assert_eq!(error.operation(), operation);
            }
            other => panic!("expected unsupported lifecycle error, got {other:?}"),
        }

        let cases = [
            (LocalTempCommandError::Capacity, LocalTempWorkerCommandFailure::Capacity),
            (
                LocalTempCommandError::ResourceUnavailable,
                LocalTempWorkerCommandFailure::ResourceUnavailable,
            ),
            (LocalTempCommandError::Panicked, LocalTempWorkerCommandFailure::Panicked),
        ];
        for (error, expected) in cases {
            match map_temp_command_error(error, operation) {
                LocalTempLifecycleError::Worker(actual) => assert_eq!(actual, expected),
                other => panic!("expected worker lifecycle error, got {other:?}"),
            }
        }
    }
}
