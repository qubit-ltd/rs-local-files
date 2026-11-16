use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex;

use crate::LocalFileError;
use crate::options::LocalReadAllOptions;

/// Handle for a typed local operation managed by qubit-operation.
pub type LocalOperation<T, E = LocalFileError> = qubit_operation::OperationHandle<T, E>;

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

/// Bounded summary of local filesystem effects, independent of operation outcome.
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
#[derive(Clone, Debug)]
pub struct LocalFactObserver {
    snapshot: Arc<Mutex<LocalFactSnapshot>>,
}

impl LocalFactObserver {
    /// Returns the latest fixed-size snapshot without retaining a resource reference.
    pub fn snapshot(&self) -> LocalFactSnapshot {
        *self
            .snapshot
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }

    #[allow(dead_code)]
    pub(crate) fn new() -> Self {
        Self {
            snapshot: Arc::new(Mutex::new(LocalFactSnapshot::default())),
        }
    }

    #[allow(dead_code)]
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
}

/// Submission rejection that returns the exact owned request to its caller.
pub struct LocalSubmitError<R> {
    request: R,
    rejection: LocalSubmitRejection,
}

impl<R> LocalSubmitError<R> {
    #[allow(dead_code)]
    pub(crate) fn new(request: R, rejection: LocalSubmitRejection) -> Self {
        Self { request, rejection }
    }

    /// Returns the original request without requiring it to implement Clone or Debug.
    pub fn into_request(self) -> R {
        self.request
    }

    /// Returns the admission reason.
    pub fn rejection(&self) -> LocalSubmitRejection {
        self.rejection
    }
}

impl<R> std::fmt::Debug for LocalSubmitError<R> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("LocalSubmitError")
            .field("rejection", &self.rejection)
            .finish_non_exhaustive()
    }
}

impl<R> std::fmt::Display for LocalSubmitError<R> {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "local operation rejected: {:?}", self.rejection)
    }
}

impl<R: 'static> std::error::Error for LocalSubmitError<R> {}

#[cfg(test)]
mod tests {
    use super::LocalSubmitError;
    use super::LocalSubmitRejection;

    struct OpaqueRequest {
        identity: Box<()>,
    }

    #[test]
    fn rejected_request_round_trips_the_same_non_clone_non_debug_value() {
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
