// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex;
use std::time::Duration;

use crate::LocalFileError;
use crate::options::LocalReadAllOptions;

/// Handle for a typed local operation managed by qubit-operation.
///
/// Owned memory permits stay attached to this handle until its outcome is
/// delivered or the handle is dropped and the upstream result slot discarded.
pub struct LocalOperation<T: Send + 'static, E: Send + 'static = LocalFileError> {
    /// Stores the inner associated with this value.
    inner: Option<qubit_operation::OperationHandle<T, E>>,
    /// Stores the budget associated with this value.
    budget: Option<Arc<dyn Send + Sync>>,
}

impl<T: Send + 'static, E: Send + 'static> LocalOperation<T, E> {
    #[cfg(feature = "async")]
    /// Runs the new behavior while preserving the operation lifecycle invariants.

    /// # Parameters
    /// - `budget` — supplies data or policy for this operation.
    pub(crate) fn new(
        inner: qubit_operation::OperationHandle<T, E>,
        budget: Arc<dyn Send + Sync>,
    ) -> Self {
        Self {
            inner: Some(inner),
            budget: Some(budget),
        }
    }

    /// Runs the id behavior while preserving the operation lifecycle invariants.
    pub fn id(&self) -> qubit_operation::OperationId {
        self.inner().id()
    }
    /// Runs the request stop behavior while preserving the operation lifecycle invariants.

    /// # Parameters
    /// - `reason` — supplies data or policy for this operation.
    pub fn request_stop(
        &self,
        reason: qubit_operation::StopReason,
    ) -> qubit_operation::StopRequestStatus {
        self.inner().request_stop(reason)
    }
    /// Runs the snapshot behavior while preserving the operation lifecycle invariants.
    pub fn snapshot(&self) -> qubit_operation::OperationSnapshot {
        self.inner().snapshot()
    }
    /// Runs the wait outcome behavior while preserving the operation lifecycle invariants.
    pub fn wait_outcome(
        &mut self,
    ) -> Result<qubit_operation::OperationOutcome<T, E>, qubit_operation::OutcomeError> {
        let result = self.inner_mut().wait_outcome();
        if result.is_ok() {
            self.budget.take();
        }
        result
    }
    /// Runs the try take outcome behavior while preserving the operation lifecycle invariants.
    pub fn try_take_outcome(
        &mut self,
    ) -> Result<Option<qubit_operation::OperationOutcome<T, E>>, qubit_operation::OutcomeError>
    {
        let result = self.inner_mut().try_take_outcome();
        if matches!(result, Ok(Some(_))) {
            self.budget.take();
        }
        result
    }
    /// Runs the wait settlement behavior while preserving the operation lifecycle invariants.
    pub fn wait_settlement(
        &self,
    ) -> Result<Arc<qubit_operation::SettlementReport>, qubit_operation::ObservationError> {
        self.inner().wait_settlement()
    }
    /// Runs the wait settlement for behavior while preserving the operation lifecycle invariants.

    /// # Parameters
    /// - `timeout` — supplies data or policy for this operation.
    pub fn wait_settlement_for(
        &self,
        timeout: Duration,
    ) -> Result<qubit_operation::SettlementWait, qubit_operation::ObservationError> {
        self.inner().wait_settlement_for(timeout)
    }
    /// Runs the into observer behavior while preserving the operation lifecycle invariants.
    pub fn into_observer(mut self) -> qubit_operation::OperationObserver {
        let observer = self
            .inner
            .take()
            .expect("local handle is present")
            .into_observer();
        self.budget.take();
        observer
    }
    #[cfg(feature = "async")]
    /// Runs the into async behavior while preserving the operation lifecycle invariants.
    pub fn into_async(mut self) -> AsyncLocalOperation<T, E> {
        let inner = self
            .inner
            .take()
            .expect("local handle is present")
            .into_async();
        AsyncLocalOperation {
            inner: Some(inner),
            budget: self.budget.take(),
        }
    }
    /// Runs the inner behavior while preserving the operation lifecycle invariants.
    fn inner(&self) -> &qubit_operation::OperationHandle<T, E> {
        self.inner.as_ref().expect("local handle is present")
    }
    /// Runs the inner mut behavior while preserving the operation lifecycle invariants.
    fn inner_mut(&mut self) -> &mut qubit_operation::OperationHandle<T, E> {
        self.inner.as_mut().expect("local handle is present")
    }
}

impl<T: Send + 'static, E: Send + 'static> Drop for LocalOperation<T, E> {
    /// Runs the drop behavior while preserving the operation lifecycle invariants.
    fn drop(&mut self) {
        drop(self.inner.take());
        self.budget.take();
    }
}

#[cfg(feature = "async")]
/// Represents async local operation within the local operation subsystem.
pub struct AsyncLocalOperation<T: Send + 'static, E: Send + 'static = LocalFileError> {
    /// Stores the inner associated with this value.
    inner: Option<qubit_operation::AsyncOperationHandle<T, E>>,
    /// Stores the budget associated with this value.
    budget: Option<Arc<dyn Send + Sync>>,
}

#[cfg(feature = "async")]
impl<T: Send + 'static, E: Send + 'static> AsyncLocalOperation<T, E> {
    /// Runs the id behavior while preserving the operation lifecycle invariants.
    pub fn id(&self) -> qubit_operation::OperationId {
        self.inner().id()
    }
    /// Runs the request stop behavior while preserving the operation lifecycle invariants.

    /// # Parameters
    /// - `reason` — supplies data or policy for this operation.
    pub fn request_stop(
        &self,
        reason: qubit_operation::StopReason,
    ) -> qubit_operation::StopRequestStatus {
        self.inner().request_stop(reason)
    }
    /// Runs the snapshot behavior while preserving the operation lifecycle invariants.
    pub fn snapshot(&self) -> qubit_operation::OperationSnapshot {
        self.inner().snapshot()
    }
    /// Runs the wait outcome behavior while preserving the operation lifecycle invariants.
    pub async fn wait_outcome(
        &mut self,
    ) -> Result<qubit_operation::OperationOutcome<T, E>, qubit_operation::OutcomeError> {
        let result = self.inner_mut().wait_outcome().await;
        if result.is_ok() {
            self.budget.take();
        }
        result
    }
    /// Runs the try take outcome behavior while preserving the operation lifecycle invariants.
    pub fn try_take_outcome(
        &mut self,
    ) -> Result<Option<qubit_operation::OperationOutcome<T, E>>, qubit_operation::OutcomeError>
    {
        let result = self.inner_mut().try_take_outcome();
        if matches!(result, Ok(Some(_))) {
            self.budget.take();
        }
        result
    }
    /// Runs the wait settlement behavior while preserving the operation lifecycle invariants.
    pub async fn wait_settlement(&self) -> Arc<qubit_operation::SettlementReport> {
        self.inner().wait_settlement().await
    }
    /// Runs the into sync behavior while preserving the operation lifecycle invariants.
    pub fn into_sync(mut self) -> LocalOperation<T, E> {
        let inner = self
            .inner
            .take()
            .expect("async local handle is present")
            .into_sync();
        LocalOperation {
            inner: Some(inner),
            budget: self.budget.take(),
        }
    }
    /// Runs the into observer behavior while preserving the operation lifecycle invariants.
    pub fn into_observer(mut self) -> qubit_operation::AsyncOperationObserver {
        let observer = self
            .inner
            .take()
            .expect("async local handle is present")
            .into_observer();
        self.budget.take();
        observer
    }
    /// Runs the inner behavior while preserving the operation lifecycle invariants.
    fn inner(&self) -> &qubit_operation::AsyncOperationHandle<T, E> {
        self.inner.as_ref().expect("async local handle is present")
    }
    /// Runs the inner mut behavior while preserving the operation lifecycle invariants.
    fn inner_mut(&mut self) -> &mut qubit_operation::AsyncOperationHandle<T, E> {
        self.inner.as_mut().expect("async local handle is present")
    }
}

#[cfg(feature = "async")]
impl<T: Send + 'static, E: Send + 'static> Drop for AsyncLocalOperation<T, E> {
    /// Runs the drop behavior while preserving the operation lifecycle invariants.
    fn drop(&mut self) {
        drop(self.inner.take());
        self.budget.take();
    }
}

/// Owned input to a complete local file read.
#[derive(Debug)]
pub struct LocalReadRequest {
    /// Path interpreted using the filesystem authority captured at submission.
    pub path: PathBuf,
    /// Complete-read behavior and byte limit.
    pub read: LocalReadAllOptions,
}

/// Per-operation resource limits. None inherits the executor's limit.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct LocalOperationOptions {
    /// Maximum bytes retained for this operation, if overridden.
    pub max_owned_bytes: Option<usize>,
    /// Maximum native handles retained for this operation, if overridden.
    pub max_open_handles: Option<usize>,
    /// Maximum temporary disk bytes retained, if overridden.
    pub max_disk_bytes: Option<u64>,
}

/// Finite limits for the local worker group and its owned resources.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LocalExecutionConfig {
    /// Number of local I/O workers.
    pub worker_count: usize,
    /// Maximum queued ordinary jobs.
    pub queue_capacity: usize,
    /// Maximum jobs waiting for queue capacity.
    pub waiter_capacity: usize,
    /// Total owned in-memory payload capacity.
    pub max_owned_bytes: usize,
    /// Maximum simultaneously retained native handles.
    pub max_open_handles: usize,
    /// Maximum simultaneously reserved temporary disk bytes.
    pub max_disk_bytes: u64,
    /// Maximum live fixed-size local fact snapshots.
    pub max_fact_records: usize,
}

impl Default for LocalExecutionConfig {
    /// Runs the default behavior while preserving the operation lifecycle invariants.
    fn default() -> Self {
        Self {
            worker_count: 4,
            queue_capacity: 64,
            waiter_capacity: 64,
            max_owned_bytes: 16 * 1024 * 1024,
            max_open_handles: 256,
            max_disk_bytes: 1024 * 1024 * 1024,
            max_fact_records: 64,
        }
    }
}

/// Invalid finite-limit configuration supplied for a local executor.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LocalExecutorConfigError {
    /// At least one worker is required.
    ZeroWorkers,
    /// Ordinary job queue capacity must be nonzero.
    ZeroQueueCapacity,
    /// Queue waiter capacity must be nonzero.
    ZeroWaiterCapacity,
    /// Owned memory capacity must be nonzero.
    ZeroOwnedBytes,
    /// Native handle capacity must be nonzero.
    ZeroOpenHandles,
    /// Temporary disk capacity must be nonzero.
    ZeroDiskBytes,
    /// Fact snapshot capacity must be nonzero.
    ZeroFactRecords,
}

/// Failure to create the local worker group.
#[derive(Debug)]
pub enum LocalExecutorCreateError {
    /// The supplied configuration contains a zero capacity.
    InvalidConfig(LocalExecutorConfigError),
    /// A worker thread could not be started.
    WorkerStart(std::io::Error),
}

impl std::fmt::Display for LocalExecutorCreateError {
    /// Runs the fmt behavior while preserving the operation lifecycle invariants.

    /// # Parameters
    /// - `formatter` — supplies data or policy for this operation.
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidConfig(error) => {
                write!(formatter, "invalid local executor config: {error:?}")
            }
            Self::WorkerStart(error) => {
                write!(formatter, "failed to start local I/O worker: {error}")
            }
        }
    }
}

impl std::error::Error for LocalExecutorCreateError {
    /// Runs the source behavior while preserving the operation lifecycle invariants.
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::InvalidConfig(_) => None,
            Self::WorkerStart(error) => Some(error),
        }
    }
}

/// Known byte interval modified by an operation.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LocalModifiedRange {
    /// First modified byte, inclusive.
    pub start: u64,
    /// End of the modified interval, exclusive.
    pub end: u64,
}

/// Whether an operation published its destination.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum LocalPublicationState {
    /// No publication fact has been recorded.
    #[default]
    NotPublished,
    /// Publication completed.
    Published,
    /// The publication result could not be established.
    Unknown,
}

/// Durability fact associated with a published local change.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum LocalDurabilityState {
    /// Durability has not been established.
    #[default]
    Unknown,
    /// The configured durability requirement was met.
    Durable,
    /// Publication succeeded, but the durability requirement failed.
    Failed,
}

/// Bounded summary of local filesystem effects, independent of operation
/// outcome.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct LocalFactSnapshot {
    /// Number of known mutation events.
    pub modified_count: u64,
    /// Total number of known modified bytes.
    pub modified_bytes: u64,
    /// A known modified byte range, when representable as one interval.
    pub modified_range: Option<LocalModifiedRange>,
    /// Whether the full mutation range is unknown or non-contiguous.
    pub range_unknown: bool,
    /// Publication status retained even when a generic outcome omits the value.
    pub publication: LocalPublicationState,
    /// Durability status for the published change.
    pub durability: LocalDurabilityState,
    /// Failure to record the effect in the operation manager.
    pub effect_record_error: Option<qubit_operation::EffectRecordError>,
    /// Recovery responsibility associated with this operation, if any.
    pub recovery_id: Option<qubit_operation::RecoveryId>,
}

/// Read-only observer for the bounded local fact snapshot.
#[derive(Clone)]
pub struct LocalFactObserver {
    /// Stores the snapshot associated with this value.
    snapshot: Arc<Mutex<LocalFactSnapshot>>,
    /// Stores the  record permit associated with this value.
    _record_permit: Option<Arc<dyn Send + Sync>>,
}

impl std::fmt::Debug for LocalFactObserver {
    /// Runs the fmt behavior while preserving the operation lifecycle invariants.

    /// # Parameters
    /// - `formatter` — supplies data or policy for this operation.
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("LocalFactObserver")
            .field("snapshot", &self.snapshot())
            .finish()
    }
}

impl LocalFactObserver {
    /// Returns the latest fixed-size snapshot without retaining a resource
    /// reference.
    pub fn snapshot(&self) -> LocalFactSnapshot {
        *self
            .snapshot
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    #[allow(dead_code)]
    /// Runs the new behavior while preserving the operation lifecycle invariants.
    pub(crate) fn new() -> Self {
        Self {
            snapshot: Arc::new(Mutex::new(LocalFactSnapshot::default())),
            _record_permit: None,
        }
    }

    #[cfg(feature = "async")]
    /// Runs the with permit behavior while preserving the operation lifecycle invariants.
    pub(crate) fn with_permit(permit: Arc<dyn Send + Sync>) -> Self {
        Self {
            snapshot: Arc::new(Mutex::new(LocalFactSnapshot::default())),
            _record_permit: Some(permit),
        }
    }

    #[allow(dead_code)]
    /// Runs the update behavior while preserving the operation lifecycle invariants.

    /// # Parameters
    /// - `snapshot` — supplies data or policy for this operation.
    pub(crate) fn update(&self, snapshot: LocalFactSnapshot) {
        *self
            .snapshot
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner()) = snapshot;
    }
}

/// Why an owned local request was rejected before it started.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum LocalSubmitRejection {
    /// The manager or local executor is closed.
    Closed,
    /// A configured local capacity is exhausted.
    CapacityExceeded,
    /// The supplied operation manager does not own this executor.
    WrongManager,
    /// The operation scope ended before admission completed.
    ScopeEnded,
    /// The operation was stopped before local execution began.
    Stopped,
    /// The prepared local job was already claimed.
    AlreadyBound,
    /// A relative Host path requires the explicit asynchronous Host
    /// constructor.
    AuthorityNotCaptured,
}

/// Submission rejection that returns the exact owned request to its caller.
pub struct LocalSubmitError<R> {
    /// Stores the request associated with this value.
    request: R,
    /// Stores the rejection associated with this value.
    rejection: LocalSubmitRejection,
}

impl<R> LocalSubmitError<R> {
    #[allow(dead_code)]
    /// Runs the new behavior while preserving the operation lifecycle invariants.

    /// # Parameters
    /// - `rejection` — supplies data or policy for this operation.
    pub(crate) fn new(request: R, rejection: LocalSubmitRejection) -> Self {
        Self { request, rejection }
    }

    /// Returns the original request without requiring it to implement Clone or
    /// Debug.
    pub fn into_request(self) -> R {
        self.request
    }

    /// Returns the admission reason.
    pub fn rejection(&self) -> LocalSubmitRejection {
        self.rejection
    }
}

impl<R> std::fmt::Debug for LocalSubmitError<R> {
    /// Runs the fmt behavior while preserving the operation lifecycle invariants.

    /// # Parameters
    /// - `formatter` — supplies data or policy for this operation.
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("LocalSubmitError")
            .field("rejection", &self.rejection)
            .finish_non_exhaustive()
    }
}

impl<R> std::fmt::Display for LocalSubmitError<R> {
    /// Runs the fmt behavior while preserving the operation lifecycle invariants.

    /// # Parameters
    /// - `formatter` — supplies data or policy for this operation.
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "local operation rejected: {:?}", self.rejection)
    }
}

impl<R: 'static> std::error::Error for LocalSubmitError<R> {}

#[cfg(test)]
mod tests {
    use super::LocalSubmitError;
    use super::LocalSubmitRejection;

    /// Represents opaque request within the local operation subsystem.
    struct OpaqueRequest {
        /// Stores the identity associated with this value.
        identity: Box<()>,
    }

    #[test]
    /// Runs the test rejected request round trips the same non clone non debug value behavior while preserving the operation lifecycle invariants.
    fn test_rejected_request_round_trips_the_same_non_clone_non_debug_value() {
        let request = OpaqueRequest {
            identity: Box::new(()),
        };
        let identity = &*request.identity as *const ();
        let rejected = LocalSubmitError::new(request, LocalSubmitRejection::CapacityExceeded);
        assert_eq!(rejected.rejection(), LocalSubmitRejection::CapacityExceeded);
        let returned = rejected.into_request();

        assert_eq!(&*returned.identity as *const (), identity);
    }
}
