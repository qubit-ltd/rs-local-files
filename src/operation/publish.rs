//! Owned publication commands for local writers.

use std::future::Future;
use std::io::IoSlice;
use std::ops::{Deref, DerefMut};
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::{Arc, Mutex};
use std::task::{Context, Poll, Waker};

use qubit_io::Output;
use qubit_operation::{
    AsyncCleanup, CleanupContext, CleanupFuture, CleanupOutcome, DiagnosticSummary, EffectFact,
    EffectRecord, OperationOptions, OperationOutcome, ResourceId, ResourceRef, StopReport,
};

use super::executor::LocalIoExecutor;
use super::handoff::LocalReadLease;
use super::internal::permits::DiskPermit;
use super::temp::{LocalTempCommandError, LocalTempNative};
use super::{
    AsyncLocalFileSystem, LocalDurabilityState, LocalFactObserver, LocalFactSnapshot,
    LocalOperation, LocalOperationOptions, LocalPublicationState, LocalSubmitError,
};
use crate::options::LocalWriteOptions;
use crate::{LocalFileCommitError, LocalFileError, LocalWriteOutcome};

/// Owned data and policy for one atomic writer commit.
#[derive(Debug)]
pub struct LocalWriterCommitRequest {
    /// Destination interpreted through the facade's captured authority.
    pub path: PathBuf,
    /// Writer policy fixed for this command.
    pub options: LocalWriteOptions,
    /// Bytes owned by the operation before submission.
    pub bytes: Vec<u8>,
}

/// Structured errors retain the native writer's original errors and recovery owner.
#[derive(Debug)]
pub enum LocalWriterCommandError {
    /// Scope capacity for retryable writer cleanup was unavailable.
    ResourceCapacity(qubit_operation::ResourceCapacityError),
    /// The cleanup resource could not be committed to the operation scope.
    ResourceCommit(qubit_operation::ResourceManageErrorKind),
    /// Opening the writer failed before any write was attempted.
    Open(LocalFileError),
    /// Writing or flushing failed; a retryable writer is retained if abort cleanup failed.
    Write {
        cause: LocalFileError,
        cleanup_error: Option<LocalFileError>,
        retryable_writer: Option<crate::LocalFileWriter>,
        recovery_resource: Option<ResourceId>,
    },
    /// Commit failed, retaining its publication state and retryable writer.
    Commit(LocalFileCommitError),
}

struct LocalManagedAbortResource<W: Send + 'static> {
    native: Mutex<Option<W>>,
    id: Mutex<Option<ResourceId>>,
    cleanup_permit: Mutex<Option<super::internal::permits::Permit>>,
    abort: fn(&mut W) -> Result<(), LocalFileError>,
}

impl<W: Send + 'static> LocalManagedAbortResource<W> {
    fn new(
        cleanup_permit: super::internal::permits::Permit,
        abort: fn(&mut W) -> Result<(), LocalFileError>,
    ) -> Self {
        Self {
            native: Mutex::new(None),
            id: Mutex::new(None),
            cleanup_permit: Mutex::new(Some(cleanup_permit)),
            abort,
        }
    }
}

struct LocalManagedAbortCleanup<W: Send + 'static> {
    executor: LocalIoExecutor,
    marker: std::marker::PhantomData<fn(W)>,
}

impl<W: Send + 'static> LocalManagedAbortCleanup<W> {
    fn new(executor: LocalIoExecutor) -> Self {
        Self {
            executor,
            marker: std::marker::PhantomData,
        }
    }
}

struct LocalAbortCleanupState<W: Send + 'static> {
    owner: Mutex<Option<LocalManagedAbortResource<W>>>,
    result: Mutex<Option<CleanupOutcome<LocalManagedAbortResource<W>>>>,
    waker: Mutex<Option<Waker>>,
}

struct LocalAbortCleanupWait<W: Send + 'static>(Arc<LocalAbortCleanupState<W>>);

impl<W: Send + 'static> Future for LocalAbortCleanupWait<W> {
    type Output = CleanupOutcome<LocalManagedAbortResource<W>>;

    fn poll(self: Pin<&mut Self>, context: &mut Context<'_>) -> Poll<Self::Output> {
        if let Some(result) = self
            .0
            .result
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .take()
        {
            return Poll::Ready(result);
        }
        let Some(new_waker) = super::temp::safe_clone_waker(context.waker()) else {
            return Poll::Pending;
        };
        let old_waker = self
            .0
            .waker
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .replace(new_waker);
        super::temp::safe_drop_waker(old_waker);
        match self
            .0
            .result
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .take()
        {
            Some(result) => Poll::Ready(result),
            None => Poll::Pending,
        }
    }
}

impl<W: Send + 'static> AsyncCleanup<LocalManagedAbortResource<W>> for LocalManagedAbortCleanup<W> {
    fn cleanup(
        self: Arc<Self>,
        resource: LocalManagedAbortResource<W>,
        _context: CleanupContext,
    ) -> CleanupFuture<LocalManagedAbortResource<W>> {
        let permit = resource
            .cleanup_permit
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .take()
            .or_else(|| self.executor.reserve_cleanup());
        let Some(permit) = permit else {
            return Box::pin(async move {
                let failure = local_abort_cleanup_failure(
                    &resource,
                    "local cleanup capacity exhausted",
                    None,
                );
                CleanupOutcome::Failed { failure, resource }
            });
        };
        let state = Arc::new(LocalAbortCleanupState {
            owner: Mutex::new(Some(resource)),
            result: Mutex::new(None),
            waker: Mutex::new(None),
        });
        let worker_state = Arc::clone(&state);
        let job = Box::new(move || {
            let Some(resource) = worker_state
                .owner
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .take()
            else {
                return;
            };
            let error = {
                let mut native = resource
                    .native
                    .lock()
                    .unwrap_or_else(|error| error.into_inner());
                match native.as_mut() {
                    None => None,
                    Some(owner) => {
                        match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                            (resource.abort)(owner)
                        })) {
                            Ok(Ok(())) => {
                                native.take();
                                None
                            }
                            Ok(Err(error)) => Some(error),
                            Err(_) => Some(LocalFileError::new(
                                crate::LocalFileErrorKind::Indeterminate,
                                crate::LocalFileOperation::Abort,
                            )),
                        }
                    }
                }
            };
            let outcome = match error {
                None => CleanupOutcome::Released,
                Some(error) => {
                    let failure = local_abort_cleanup_failure(
                        &resource,
                        "local writer abort cleanup failed",
                        Some(Arc::new(error)),
                    );
                    CleanupOutcome::Failed { failure, resource }
                }
            };
            *worker_state
                .result
                .lock()
                .unwrap_or_else(|error| error.into_inner()) = Some(outcome);
            if let Some(waker) = worker_state
                .waker
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .take()
            {
                if let Err(payload) = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    waker.wake();
                })) {
                    std::mem::forget(payload);
                }
            }
        });
        if self.executor.enqueue_cleanup(job, permit).is_err() {
            let resource = state
                .owner
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .take()
                .expect("cleanup owner remains available when queue admission fails");
            return Box::pin(async move {
                let failure = local_abort_cleanup_failure(
                    &resource,
                    "local cleanup queue is unavailable",
                    None,
                );
                CleanupOutcome::Failed { failure, resource }
            });
        }
        Box::pin(LocalAbortCleanupWait(state))
    }
}

fn local_abort_cleanup_failure<W: Send + 'static>(
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

fn abort_managed_file_writer(writer: &mut crate::LocalFileWriter) -> Result<(), LocalFileError> {
    writer.abort().map(|_| ())
}

#[cfg(feature = "digest")]
fn abort_conditional_writer<D: digest::Digest + Send + 'static>(
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

#[cfg(feature = "digest")]
fn abort_managed_conditional_after_error<D: digest::Digest + Send + 'static>(
    resource: &ResourceRef<LocalManagedAbortResource<crate::LocalConditionalWriter<D>>>,
    cause: LocalFileError,
) -> LocalConditionalWriterCommandError<D> {
    let mut native = resource
        .native
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let writer = native
        .as_mut()
        .expect("managed conditional writer is available");
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

#[cfg(feature = "digest")]
fn abort_managed_conditional_after_stop<D: digest::Digest + Send + 'static>(
    resource: &ResourceRef<LocalManagedAbortResource<crate::LocalConditionalWriter<D>>>,
    stop: StopReport,
) -> OperationOutcome<
    crate::outcome::LocalConditionalWriteOutcome<D>,
    LocalConditionalWriterCommandError<D>,
> {
    let mut native = resource
        .native
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let Some(writer) = native.as_mut() else {
        return OperationOutcome::Stopped(stop);
    };
    if (resource.abort)(writer).is_ok() {
        native.take();
    }
    OperationOutcome::Stopped(stop)
}

impl AsyncLocalFileSystem {
    /// Sets the default symlink policy on this facade's owned filesystem value.
    ///
    /// This changes only the filesystem value retained by this facade; it does not
    /// modify the manager or any other facade. Conditional publication requires
    /// [`crate::policy::LocalSymlinkPolicy::Reject`] because it validates the
    /// destination without following symbolic links.
    pub fn with_symlink_policy(
        mut self,
        policy: crate::policy::LocalSymlinkPolicy,
    ) -> crate::LocalResult<Self> {
        self.filesystem.set_symlink_policy(policy)?;
        Ok(self)
    }

    /// Commits owned bytes through the local writer on a bounded I/O worker.
    ///
    /// The operation checks cancellation immediately before opening and writing.
    /// Once the native commit begins, it waits for the writer's actual result.
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
            relative,
            1,
            limits,
            owned_capacity,
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
                        return OperationOutcome::Failed(
                            LocalWriterCommandError::ResourceCapacity(error),
                        );
                    }
                };
                let Some(cleanup_permit) = executor.reserve_cleanup() else {
                    return OperationOutcome::Failed(LocalWriterCommandError::Open(
                        LocalFileError::new(
                            crate::LocalFileErrorKind::ResourceLimit,
                            crate::LocalFileOperation::OpenWriter,
                        ),
                    ));
                };
                let resource =
                    LocalManagedAbortResource::new(cleanup_permit, abort_managed_file_writer);
                let writer_resource = match reservation.commit_with(
                    resource,
                    LocalManagedAbortCleanup::<crate::LocalFileWriter>::new(executor.clone()),
                ) {
                    Ok(resource) => resource,
                    Err(error) => {
                        let (reason, _resource, _cleanup, _reservation) = error.into_parts();
                        return match reason {
                            qubit_operation::ResourceManageErrorKind::Stopped => {
                                OperationOutcome::Stopped(
                                    stop_report(scope).expect("scope stopped before writer open"),
                                )
                            }
                            reason => OperationOutcome::Failed(
                                LocalWriterCommandError::ResourceCommit(reason),
                            ),
                        };
                    }
                };
                let resource_id = writer_resource.id();
                *writer_resource
                    .id
                    .lock()
                    .unwrap_or_else(|error| error.into_inner()) = Some(resource_id);
                let writer =
                    match filesystem.open_writer_with_options(&destination, &request.options) {
                        Ok(writer) => writer,
                        Err(error) => {
                            return OperationOutcome::Failed(LocalWriterCommandError::Open(error));
                        }
                    };
                *writer_resource
                    .native
                    .lock()
                    .unwrap_or_else(|error| error.into_inner()) = Some(writer);
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
                        let mut native = writer_resource
                            .native
                            .lock()
                            .unwrap_or_else(|error| error.into_inner());
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
                    let mut native = writer_resource
                        .native
                        .lock()
                        .unwrap_or_else(|error| error.into_inner());
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
                if append_effect_recorded
                    && crate::local::test_support_enabled("lf7-append-panic-after-write")
                {
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
                let writer = writer_resource
                    .native
                    .lock()
                    .unwrap_or_else(|error| error.into_inner())
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
                            crate::LocalWriteFailureState::NotPublished => {
                                LocalPublicationState::NotPublished
                            }
                            crate::LocalWriteFailureState::Published => {
                                LocalPublicationState::Published
                            }
                            crate::LocalWriteFailureState::Indeterminate => {
                                LocalPublicationState::Unknown
                            }
                        };
                        if append_effect_recorded {
                            let durability = match error.state() {
                                crate::LocalWriteFailureState::Published => {
                                    LocalDurabilityState::Failed
                                }
                                crate::LocalWriteFailureState::NotPublished
                                | crate::LocalWriteFailureState::Indeterminate => {
                                    LocalDurabilityState::Unknown
                                }
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

fn abort_managed_writer_after_error(
    resource: &ResourceRef<LocalManagedAbortResource<crate::LocalFileWriter>>,
    cause: LocalFileError,
    accepted_bytes: u64,
    scope: &qubit_operation::OperationScope,
    observer: &LocalFactObserver,
    effect_already_recorded: bool,
) -> LocalWriterCommandError {
    let mut native = resource
        .native
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let writer = native.as_mut().expect("managed writer is available");
    match writer.abort() {
        Ok(outcome) => {
            native.take();
            if !effect_already_recorded
                && outcome.failure_state() == Some(crate::LocalWriteFailureState::Published)
            {
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
                    cleanup_error: Some(cleanup_error),
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
                cleanup_error: Some(cleanup_error),
                retryable_writer: None,
                recovery_resource: Some(resource.id()),
            }
        }
    }
}

fn abort_managed_writer_after_stop(
    resource: &ResourceRef<LocalManagedAbortResource<crate::LocalFileWriter>>,
    stop: StopReport,
    accepted_bytes: u64,
    scope: &qubit_operation::OperationScope,
    observer: &LocalFactObserver,
    effect_already_recorded: bool,
) -> OperationOutcome<LocalWriteOutcome, LocalWriterCommandError> {
    let mut native = resource
        .native
        .lock()
        .unwrap_or_else(|error| error.into_inner());
    let Some(writer) = native.as_mut() else {
        return OperationOutcome::Stopped(stop);
    };
    match writer.abort() {
        Ok(outcome) => {
            native.take();
            if !effect_already_recorded
                && outcome.failure_state() == Some(crate::LocalWriteFailureState::Published)
            {
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

fn record_writer_effect(
    scope: &qubit_operation::OperationScope,
    observer: &LocalFactObserver,
    bytes: u64,
    publication: LocalPublicationState,
    durability: LocalDurabilityState,
) {
    record_local_effect(scope, observer, 1, bytes, publication, durability);
}

fn record_append_writer_bytes(
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

fn update_writer_effect_observer(
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

fn record_local_effect(
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

fn record_stage_write(
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

fn stop_report(scope: &qubit_operation::OperationScope) -> Option<StopReport> {
    scope.context().stop_reason().map(|reason| StopReport {
        reason,
        accepted_at: None,
    })
}

/// One owned file prepared for a staged-tree publication.
#[derive(Debug)]
pub struct LocalStagedTreeFile {
    /// Normalized relative path beneath the staging directory.
    pub path: PathBuf,
    /// Owned bytes written to this path.
    pub bytes: Vec<u8>,
}

/// Owned request that creates, verifies, and publishes a staged tree.
#[derive(Debug)]
pub struct LocalStagedTreePublishRequest {
    /// Parent directory opened through the facade's captured authority.
    pub parent: PathBuf,
    /// Single native entry name installed under `parent`.
    pub name: std::ffi::OsString,
    /// Private temporary-directory creation policy.
    pub stage_options: crate::options::LocalTempDirectoryOptions,
    /// Files written under the stage; paths must be direct children.
    pub files: Vec<LocalStagedTreeFile>,
    /// Durability and access options for publication; exact verification is added automatically.
    pub publish_options: crate::options::LocalTreePublishOptions,
}

/// A retained disk reservation associated with a staged-tree owner.
#[derive(Clone)]
pub struct LocalStagedTreeDiskReservation(Arc<DiskPermit>);

impl std::fmt::Debug for LocalStagedTreeDiskReservation {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_tuple("LocalStagedTreeDiskReservation")
            .field(&self.0.amount())
            .finish()
    }
}

impl LocalStagedTreeDiskReservation {
    /// Returns the number of disk bytes held by this reservation.
    pub fn bytes(&self) -> u64 {
        self.0.amount()
    }
}

/// Keeps a native staged-tree error and its disk reservation together.
pub struct LocalStagedTreeReservedError<E> {
    error: E,
    reservation: LocalStagedTreeDiskReservation,
}

impl<E> LocalStagedTreeReservedError<E> {
    fn new(error: E, reservation: LocalStagedTreeDiskReservation) -> Self {
        Self { error, reservation }
    }

    /// Borrows the original native error and its recovery facts.
    pub fn error(&self) -> &E {
        &self.error
    }

    /// Transfers both the native error owner and its disk reservation.
    pub fn into_parts(self) -> (E, LocalStagedTreeDiskReservation) {
        (self.error, self.reservation)
    }
}

impl<E> Deref for LocalStagedTreeReservedError<E> {
    type Target = E;

    fn deref(&self) -> &Self::Target {
        &self.error
    }
}

impl<E: std::fmt::Debug> std::fmt::Debug for LocalStagedTreeReservedError<E> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("LocalStagedTreeReservedError")
            .field("error", &self.error)
            .field("reservation", &self.reservation)
            .finish()
    }
}

/// A failed staged write that still owns the unpublished stage and disk quota.
pub struct LocalStagedTreeRecovery {
    stage: Option<crate::directory::LocalStagedDirectory>,
    reservation: LocalStagedTreeDiskReservation,
}

impl LocalStagedTreeRecovery {
    fn new(
        stage: crate::directory::LocalStagedDirectory,
        reservation: LocalStagedTreeDiskReservation,
    ) -> Self {
        Self {
            stage: Some(stage),
            reservation,
        }
    }

    /// Sets cleanup limits on the retained stage.
    pub fn set_cleanup_limits(&mut self, limits: crate::options::LocalTempCleanupLimits) {
        self.stage
            .as_mut()
            .expect("staged recovery owner is present")
            .set_cleanup_limits(limits);
    }

    /// Returns the stage name under its captured parent authority.
    pub fn relative_name(&self) -> &std::ffi::OsStr {
        self.stage
            .as_ref()
            .expect("staged recovery owner is present")
            .relative_name()
    }

    /// Explicitly cleans the stage. A failure keeps its native recovery error
    /// and the disk reservation in one value.
    pub fn cleanup(
        mut self,
    ) -> Result<(), LocalStagedTreeReservedError<crate::directory::LocalTreeCleanupError>> {
        let stage = self.stage.take().expect("staged recovery owner is present");
        stage
            .cleanup()
            .map_err(|error| LocalStagedTreeReservedError::new(error, self.reservation))
    }
}

impl Deref for LocalStagedTreeRecovery {
    type Target = crate::directory::LocalStagedDirectory;

    fn deref(&self) -> &Self::Target {
        self.stage
            .as_ref()
            .expect("staged recovery owner is present")
    }
}

impl DerefMut for LocalStagedTreeRecovery {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.stage
            .as_mut()
            .expect("staged recovery owner is present")
    }
}

impl std::fmt::Debug for LocalStagedTreeRecovery {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("LocalStagedTreeRecovery")
            .field("stage", &self.stage)
            .field("reservation", &self.reservation)
            .finish()
    }
}

/// Errors keep the stage or native recovery owners attached for retry/cleanup.
#[derive(Debug)]
pub enum LocalStagedTreePublishError {
    /// Parent authority could not be opened.
    Parent(LocalFileError),
    /// Stage creation failed with its native cleanup ownership.
    Create(LocalStagedTreeReservedError<crate::directory::LocalTreeCreateError>),
    /// A staged file could not be created; the stage remains available for cleanup.
    Write {
        cause: LocalFileError,
        stage: LocalStagedTreeRecovery,
    },
    /// Verification or publication failed with the existing native recovery report.
    Publish(LocalStagedTreeReservedError<crate::directory::LocalTreePublishError>),
    /// A cancellation cleanup failed and retains the cleanup owner.
    Cleanup(LocalStagedTreeReservedError<crate::directory::LocalTreeCleanupError>),
}

impl AsyncLocalFileSystem {
    /// Creates files beneath a private stage, verifies the exact tree, then publishes it.
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
        if disk_limit == 0
            || disk_limit > self.executor.inner.config.max_disk_bytes
            || disk_bytes > disk_limit
        {
            return Err(LocalSubmitError::new(
                request,
                super::LocalSubmitRejection::CapacityExceeded,
            ));
        }
        let Some(disk) = self.executor.reserve_disk(disk_bytes) else {
            return Err(LocalSubmitError::new(
                request,
                super::LocalSubmitRejection::CapacityExceeded,
            ));
        };
        let reservation = LocalStagedTreeDiskReservation(Arc::new(disk));
        let filesystem = self.filesystem.clone();
        self.submit_owned_with_bytes(
            manager,
            operation,
            request,
            relative,
            2,
            limits,
            owned_bytes,
            move |request, scope, observer, _budget, _maximum| {
                if let Some(stop) = stop_report(scope) {
                    return OperationOutcome::Stopped(stop);
                }
                let parent = match filesystem.open_directory_with_options(
                    &parent_path,
                    &crate::options::LocalDirectoryOpenOptions::new(),
                ) {
                    Ok(parent) => parent,
                    Err(error) => {
                        return OperationOutcome::Failed(LocalStagedTreePublishError::Parent(
                            error,
                        ));
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
                            Err(error) => {
                                OperationOutcome::Failed(LocalStagedTreePublishError::Cleanup(
                                    LocalStagedTreeReservedError::new(error, reservation.clone()),
                                ))
                            }
                        };
                    }
                    let entry = match crate::directory::LocalTreeEntry::new(
                        file.path.clone(),
                        crate::LocalFileKind::File,
                    ) {
                        Ok(entry) => entry,
                        Err(cause) => {
                            return OperationOutcome::Failed(LocalStagedTreePublishError::Write {
                                cause,
                                stage: LocalStagedTreeRecovery::new(stage, reservation.clone()),
                            });
                        }
                    };
                    let mut input = file.bytes.as_slice();
                    if let Err(cause) =
                        stage.write_file_new(&file.path, &mut input, file.bytes.len() as u64)
                    {
                        record_stage_write(scope, &observer, 0, EffectFact::Unknown);
                        return OperationOutcome::Failed(LocalStagedTreePublishError::Write {
                            cause,
                            stage: LocalStagedTreeRecovery::new(stage, reservation.clone()),
                        });
                    }
                    record_stage_write(
                        scope,
                        &observer,
                        file.bytes.len() as u64,
                        EffectFact::Known,
                    );
                    expected.push(entry);
                }
                let validation = crate::directory::LocalTreeValidationOptions::new()
                    .with_exact_entries(expected);
                let publish_options = request.publish_options.clone().with_validation(validation);
                if let Some(stop) = stop_report(scope) {
                    return match stage.cleanup() {
                        Ok(()) => OperationOutcome::Stopped(stop),
                        Err(error) => {
                            OperationOutcome::Failed(LocalStagedTreePublishError::Cleanup(
                                LocalStagedTreeReservedError::new(error, reservation.clone()),
                            ))
                        }
                    };
                }
                match stage.publish_new(&parent, &request.name, &publish_options) {
                    Ok(outcome) => {
                        if outcome.publication() == crate::outcome::LocalTreePublication::Published
                        {
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
                            crate::outcome::LocalTreePublication::Indeterminate => {
                                record_local_effect(
                                    scope,
                                    &observer,
                                    1,
                                    0,
                                    LocalPublicationState::Unknown,
                                    LocalDurabilityState::Unknown,
                                )
                            }
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

#[cfg(feature = "digest")]
/// Structured conditional writer failure preserving open, stream, and commit errors.
#[derive(Debug)]
pub enum LocalConditionalWriterCommandError<D: digest::Digest + Send + 'static> {
    /// Scope capacity for retryable writer cleanup was unavailable.
    ResourceCapacity(qubit_operation::ResourceCapacityError),
    /// The cleanup resource could not be committed to the operation scope.
    ResourceCommit(qubit_operation::ResourceManageErrorKind),
    /// Opening the candidate writer failed.
    Open(LocalFileError),
    /// Writing/flushing the candidate failed.
    Write {
        cause: LocalFileError,
        cleanup_error: Option<LocalFileError>,
        recovery_resource: Option<ResourceId>,
        retryable_writer: Option<crate::LocalConditionalWriter<D>>,
    },
    /// Conditional publication failed with native recovery state.
    Commit(crate::error::LocalConditionalCommitError),
}

#[cfg(feature = "digest")]
/// Owned request for a digest-preconditioned writer commit.
#[derive(Debug)]
pub struct LocalConditionalWriterCommitRequest<D: digest::Digest> {
    /// Destination interpreted through the facade authority.
    pub path: PathBuf,
    /// Digest/absence condition checked by the native conditional state machine.
    pub precondition: crate::options::LocalWritePrecondition<digest::Output<D>>,
    /// Publication policy.
    pub options: LocalWriteOptions,
    /// Owned candidate bytes.
    pub bytes: Vec<u8>,
}

#[cfg(feature = "digest")]
impl AsyncLocalFileSystem {
    /// Commits bytes only when the digest precondition still holds.
    pub fn submit_conditional_writer_commit<D: digest::Digest + Send + 'static>(
        &self,
        manager: &qubit_operation::OperationManager,
        operation: OperationOptions,
        request: LocalConditionalWriterCommitRequest<D>,
        limits: LocalOperationOptions,
    ) -> Result<
        (
            LocalOperation<
                crate::outcome::LocalConditionalWriteOutcome<D>,
                LocalConditionalWriterCommandError<D>,
            >,
            LocalFactObserver,
        ),
        LocalSubmitError<LocalConditionalWriterCommitRequest<D>>,
    > {
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
            relative,
            1,
            limits,
            owned_capacity,
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
                        return OperationOutcome::Failed(
                            LocalConditionalWriterCommandError::ResourceCapacity(error),
                        );
                    }
                };
                let Some(cleanup_permit) = executor.reserve_cleanup() else {
                    return OperationOutcome::Failed(LocalConditionalWriterCommandError::Open(
                        LocalFileError::new(
                            crate::LocalFileErrorKind::ResourceLimit,
                            crate::LocalFileOperation::OpenWriter,
                        ),
                    ));
                };
                let resource =
                    LocalManagedAbortResource::new(cleanup_permit, abort_conditional_writer::<D>);
                let writer_resource = match reservation.commit_with(
                    resource,
                    LocalManagedAbortCleanup::<crate::LocalConditionalWriter<D>>::new(
                        executor.clone(),
                    ),
                ) {
                    Ok(resource) => resource,
                    Err(error) => {
                        let (reason, _resource, _cleanup, _reservation) = error.into_parts();
                        return match reason {
                            qubit_operation::ResourceManageErrorKind::Stopped => {
                                OperationOutcome::Stopped(
                                    stop_report(scope).expect("scope stopped before writer open"),
                                )
                            }
                            reason => OperationOutcome::Failed(
                                LocalConditionalWriterCommandError::ResourceCommit(reason),
                            ),
                        };
                    }
                };
                let resource_id = writer_resource.id();
                *writer_resource
                    .id
                    .lock()
                    .unwrap_or_else(|error| error.into_inner()) = Some(resource_id);
                let writer = match filesystem.open_conditional_writer::<D>(
                    &destination,
                    request.precondition,
                    &request.options,
                ) {
                    Ok(writer) => writer,
                    Err(error) => {
                        return OperationOutcome::Failed(LocalConditionalWriterCommandError::Open(
                            error,
                        ));
                    }
                };
                *writer_resource
                    .native
                    .lock()
                    .unwrap_or_else(|error| error.into_inner()) = Some(writer);
                let mut offset = 0;
                while offset < request.bytes.len() {
                    if let Some(stop) = stop_report(scope) {
                        return abort_managed_conditional_after_stop(&writer_resource, stop);
                    }
                    let write_result = {
                        let mut native = writer_resource
                            .native
                            .lock()
                            .unwrap_or_else(|error| error.into_inner());
                        Output::write(
                            native
                                .as_mut()
                                .expect("managed conditional writer is available"),
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
                            return OperationOutcome::Failed(
                                abort_managed_conditional_after_error(&writer_resource, error),
                            );
                        }
                        Ok(written) => offset += written,
                        Err(source) => {
                            let error = LocalFileError::from_io(
                                crate::LocalFileOperation::Commit,
                                Some(destination.clone()),
                                None,
                                source,
                            );
                            return OperationOutcome::Failed(
                                abort_managed_conditional_after_error(&writer_resource, error),
                            );
                        }
                    }
                }
                let flush_result = {
                    let mut native = writer_resource
                        .native
                        .lock()
                        .unwrap_or_else(|error| error.into_inner());
                    Output::flush(
                        native
                            .as_mut()
                            .expect("managed conditional writer is available"),
                    )
                };
                if let Err(source) = flush_result {
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
                if let Some(stop) = stop_report(scope) {
                    return abort_managed_conditional_after_stop(&writer_resource, stop);
                }
                #[cfg(feature = "test-support")]
                crate::test_support::hit_test_gate("lf7-conditional-before-publish");
                if let Some(stop) = stop_report(scope) {
                    return abort_managed_conditional_after_stop(&writer_resource, stop);
                }
                let writer = writer_resource
                    .native
                    .lock()
                    .unwrap_or_else(|error| error.into_inner())
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
                            crate::outcome::LocalConditionalPublicationState::Published => {
                                record_writer_effect(
                                    scope,
                                    &observer,
                                    request.bytes.len() as u64,
                                    LocalPublicationState::Published,
                                    LocalDurabilityState::Failed,
                                )
                            }
                            crate::outcome::LocalConditionalPublicationState::Indeterminate => {
                                record_local_effect(
                                    scope,
                                    &observer,
                                    1,
                                    0,
                                    LocalPublicationState::Unknown,
                                    LocalDurabilityState::Unknown,
                                )
                            }
                            _ => {}
                        }
                        OperationOutcome::Failed(LocalConditionalWriterCommandError::Commit(error))
                    }
                }
            },
        )
    }
}

/// One owned lifecycle action for a previously managed temporary resource.
#[derive(Clone, Debug)]
pub enum LocalTempLifecycleCommand {
    /// Atomically keep the temporary resource at a generated destination.
    Keep,
    /// Atomically persist at the requested path.
    Persist {
        /// Target interpreted using the temporary resource's captured authority.
        target: PathBuf,
        /// Persistence guarantees for this transition.
        options: crate::options::LocalPersistOptions,
    },
    /// Seal a temporary file as read-only.
    Seal {
        /// File synchronization policy for sealing.
        options: crate::LocalSealOptions,
    },
}

/// Successful result of one temporary lifecycle transition.
pub enum LocalTempLifecycleResult {
    /// The temporary resource was published outside its private sandbox.
    Persisted(crate::outcome::LocalPersistOutcome),
    /// The same managed resource now contains a sealed read-only file.
    Sealed(LocalReadLease),
}

/// Lossless persistence failure facts; the owner remains in the same managed resource.
#[derive(Debug)]
pub struct LocalTempPersistFailure {
    /// Native cause.
    pub error: LocalFileError,
    /// Requested target.
    pub requested_target: PathBuf,
    /// Resolved target if resolution completed.
    pub resolved_target: Option<PathBuf>,
    /// Stage where persistence stopped.
    pub stage: crate::outcome::LocalPersistStage,
    /// Publication state established by the attempt.
    pub state: crate::outcome::LocalPersistFailureState,
    /// Source authority state observed at failure.
    pub source_state: crate::outcome::LocalTempSourceState,
}

/// Managed command failure, preserving native errors and worker admission state.
#[derive(Debug)]
pub enum LocalTempLifecycleError {
    /// Structured persistence failure.
    Persist(LocalTempPersistFailure),
    /// Structured sealing failure.
    Seal {
        /// Native cause.
        error: LocalFileError,
        /// Access state after the attempted seal.
        access_state: crate::LocalTempAccessState,
    },
    /// The resource kind does not support this command.
    Unsupported(LocalFileError),
    /// The bounded local worker could not accept or complete the command.
    Worker(LocalTempWorkerCommandFailure),
    /// The operation was stopped before the native transition began.
    Stopped,
}

/// Worker-level command failure mapped to a stable public value.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LocalTempWorkerCommandFailure {
    /// Worker command capacity was unavailable.
    Capacity,
    /// The managed resource no longer had a native owner.
    ResourceUnavailable,
    /// The worker command panicked before returning its result.
    Panicked,
}

/// Submits a temporary lifecycle transition and returns the same resource access token.
pub type LocalTempLifecycleOperation =
    qubit_operation::AsyncOperationHandle<LocalTempLifecycleResult, LocalTempLifecycleError>;

/// Owned temporary-resource lifecycle request.
pub struct LocalTempLifecycleRequest {
    /// Existing access token for the registered temporary resource.
    pub access: super::LocalTempFileAccess,
    /// Native lifecycle action to serialize on the local worker.
    pub command: LocalTempLifecycleCommand,
}

impl AsyncLocalFileSystem {
    /// Serializes temporary keep/persist/seal work on the local worker and awaits it
    /// from the operation future, keeping the resource's existing identity.
    pub fn submit_temp_lifecycle(
        &self,
        manager: &qubit_operation::OperationManager,
        operation: OperationOptions,
        request: LocalTempLifecycleRequest,
    ) -> Result<
        (
            LocalTempLifecycleOperation,
            super::LocalTempFileAccess,
            LocalFactObserver,
        ),
        LocalSubmitError<LocalTempLifecycleRequest>,
    > {
        let scoped = match manager.with_group(&self.executor.inner.group) {
            Ok(scoped) => scoped,
            Err(error) => {
                let rejection = match error {
                    qubit_operation::GroupBindingError::WrongManager => {
                        super::LocalSubmitRejection::WrongManager
                    }
                    qubit_operation::GroupBindingError::ScopeEnded => {
                        super::LocalSubmitRejection::ScopeEnded
                    }
                    qubit_operation::GroupBindingError::Closed => {
                        super::LocalSubmitRejection::Closed
                    }
                    qubit_operation::GroupBindingError::CapacityExceeded => {
                        super::LocalSubmitRejection::CapacityExceeded
                    }
                };
                return Err(LocalSubmitError::new(request, rejection));
            }
        };
        let Some(fact_permit) = self.executor.reserve_fact() else {
            return Err(LocalSubmitError::new(
                request,
                super::LocalSubmitRejection::CapacityExceeded,
            ));
        };
        let fact_permit: Arc<dyn Send + Sync> = Arc::new(fact_permit);
        let observer = LocalFactObserver::with_permit(Arc::clone(&fact_permit));
        let observer_for_task = observer.clone();
        let returned_access = request.access.clone_access();
        let request_owner = Arc::new(Mutex::new(Some(request)));
        let request_for_body = Arc::clone(&request_owner);
        let executor = self.executor.clone();
        let submit = scoped
            .as_async()
            .try_submit(operation, move |scope| async move {
                let request = request_for_body
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
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
                let receiver = request
                    .access
                    .enqueue_native_transition(&executor, move |native| {
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
                        record_writer_effect(
                            &scope,
                            &observer_for_task,
                            0,
                            publication,
                            durability,
                        );
                        Ok(result)
                    }
                }
            });
        match submit {
            Ok(handle) => Ok((handle, returned_access, observer)),
            Err(error) => {
                let (reason, _body) = error.into_parts();
                let request = request_owner
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .take()
                    .expect("rejected temp command returns its original request");
                let rejection = match reason {
                    qubit_operation::AdmissionError::Closed
                    | qubit_operation::AdmissionError::ExecutorUnavailable => {
                        super::LocalSubmitRejection::Closed
                    }
                    qubit_operation::AdmissionError::CapacityExceeded
                    | qubit_operation::AdmissionError::Busy => {
                        super::LocalSubmitRejection::CapacityExceeded
                    }
                    qubit_operation::AdmissionError::ScopeEnded => {
                        super::LocalSubmitRejection::ScopeEnded
                    }
                    qubit_operation::AdmissionError::Stopped => {
                        super::LocalSubmitRejection::Stopped
                    }
                    _ => super::LocalSubmitRejection::Closed,
                };
                Err(LocalSubmitError::new(request, rejection))
            }
        }
    }
}

fn map_temp_worker_error(error: LocalTempCommandError) -> LocalTempWorkerCommandFailure {
    match error {
        LocalTempCommandError::Capacity => LocalTempWorkerCommandFailure::Capacity,
        LocalTempCommandError::ResourceUnavailable => {
            LocalTempWorkerCommandFailure::ResourceUnavailable
        }
        LocalTempCommandError::UnsupportedKind => {
            LocalTempWorkerCommandFailure::ResourceUnavailable
        }
        LocalTempCommandError::Panicked => LocalTempWorkerCommandFailure::Panicked,
    }
}

fn map_temp_command_error(
    error: LocalTempCommandError,
    operation: crate::LocalFileOperation,
) -> LocalTempLifecycleError {
    if error == LocalTempCommandError::UnsupportedKind {
        LocalTempLifecycleError::Unsupported(LocalFileError::new(
            crate::LocalFileErrorKind::Unsupported,
            operation,
        ))
    } else {
        LocalTempLifecycleError::Worker(map_temp_worker_error(error))
    }
}

fn transition_temp_native(
    native: LocalTempNative,
    command: LocalTempLifecycleCommand,
    lease_access: super::LocalTempFileAccess,
    context: qubit_operation::OperationContext,
) -> Result<
    (
        Option<LocalTempNative>,
        (
            LocalTempLifecycleResult,
            LocalPublicationState,
            LocalDurabilityState,
        ),
    ),
    (LocalTempNative, LocalTempLifecycleError),
> {
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
                        LocalTempLifecycleResult::Persisted(outcome),
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
                            LocalTempLifecycleResult::Persisted(outcome),
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
                        LocalTempLifecycleResult::Sealed(LocalReadLease::from_temp_access(
                            &lease_access,
                            false,
                        )),
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
        (LocalTempNative::Directory(directory), LocalTempLifecycleCommand::Keep) => {
            match directory.keep() {
                Ok(outcome) => {
                    let durability = if outcome.durable() {
                        LocalDurabilityState::Durable
                    } else {
                        LocalDurabilityState::Unknown
                    };
                    Ok((
                        None,
                        (
                            LocalTempLifecycleResult::Persisted(outcome),
                            LocalPublicationState::Published,
                            durability,
                        ),
                    ))
                }
                Err(error) => Err(persist_failure(error, LocalTempNative::Directory)),
            }
        }
        (
            LocalTempNative::Directory(directory),
            LocalTempLifecycleCommand::Persist { target, options },
        ) => match directory.persist_with(&target, options) {
            Ok(outcome) => {
                let durability = if outcome.durable() {
                    LocalDurabilityState::Durable
                } else {
                    LocalDurabilityState::Unknown
                };
                Ok((
                    None,
                    (
                        LocalTempLifecycleResult::Persisted(outcome),
                        LocalPublicationState::Published,
                        durability,
                    ),
                ))
            }
            Err(error) => Err(persist_failure(error, LocalTempNative::Directory)),
        },
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

fn record_temp_failure(
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
        LocalTempLifecycleError::Seal { .. } => {
            record_stage_write(scope, observer, 0, EffectFact::Known)
        }
        _ => {}
    }
}
