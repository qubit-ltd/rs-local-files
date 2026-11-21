// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::future::poll_fn;
use std::path::PathBuf;
use std::sync::Arc;
mod completion;

// Implements complete-operation submission methods for this filesystem facade.
mod complete_operations;
mod owned_submission;
use std::sync::Mutex;

use completion::Completion;
use owned_submission::OwnedSubmission;

use super::LocalIoExecutor;

#[path = "async_local_file_system/publish.rs"]
mod publish;
/// Asynchronous facade retaining its synchronous filesystem authority and
/// executor.
///
/// # Examples
///
/// Bind an application manager to a bounded local executor, then use the
/// facade to submit an operation.
///
/// ```no_run
/// # async fn example() -> Result<(), Box<dyn std::error::Error>> {
/// use std::path::PathBuf;
/// use qubit_local_files::{AsyncLocalFileSystem, LocalExecutionConfig, LocalIoExecutor, LocalReadRequest};
/// use qubit_local_files::operation::LocalOperationOptions;
/// use qubit_operation::{ManagerOptions, OperationManager, OperationOptions, OperationOutcome};
///
/// let manager = OperationManager::new(ManagerOptions::default())?;
/// let executor = LocalIoExecutor::new(manager.clone(), LocalExecutionConfig::default())?;
/// let filesystem = AsyncLocalFileSystem::host(executor).await?;
/// let (mut operation, observer) = filesystem.submit_read_all_with_observer(
///     &manager,
///     OperationOptions::default(),
///     LocalReadRequest { path: PathBuf::from("input.txt"), read: Default::default() },
///     LocalOperationOptions::default(),
/// )?;
/// let mut operation = operation.into_async();
/// match operation.wait_outcome().await? {
///     OperationOutcome::Succeeded(bytes) => println!("read {} bytes", bytes.len()),
///     OperationOutcome::Failed(error) => return Err(error.into()),
///     other => return Err(format!("read did not succeed: {other:?}").into()),
/// }
/// let _facts = observer.snapshot();
/// # Ok(())
/// # }
/// ```
#[derive(Clone)]
pub struct AsyncLocalFileSystem {
    /// Stores the filesystem associated with this value.
    pub(crate) filesystem: crate::LocalFileSystem,
    /// Stores the executor associated with this value.
    pub(crate) executor: LocalIoExecutor,
    /// Stores the host base associated with this value.
    pub(crate) host_base: Option<PathBuf>,
}

impl AsyncLocalFileSystem {
    /// Runs the new behavior while preserving the operation lifecycle
    /// invariants.
    /// # Parameters
    /// - `executor` — supplies data or policy for this operation.
    /// - `host_base` — supplies data or policy for this operation.
    pub(crate) fn new(
        filesystem: crate::LocalFileSystem,
        executor: LocalIoExecutor,
        host_base: Option<PathBuf>,
    ) -> Self {
        Self {
            filesystem,
            executor,
            host_base,
        }
    }
    /// Creates a Host facade after reading the process cwd on a local I/O
    /// worker.
    ///
    /// # Parameters
    /// - `executor` — bounded worker executor used to run blocking work.
    pub async fn host(executor: LocalIoExecutor) -> crate::LocalResult<Self> {
        let filesystem = crate::LocalFileSystem::host()?;
        let host_base = run_on_worker(&executor, || {
            std::env::current_dir().map_err(|source| {
                crate::LocalFileError::from_io(crate::LocalFileOperation::CurrentDirectory, None, None, source)
                    .with_reason("failed to capture process directory for async local filesystem")
            })
        })
        .await?;
        Ok(Self::new(filesystem, executor, Some(host_base)))
    }
    /// Opens a Rooted authority on a local I/O worker and returns its async
    /// facade.
    ///
    /// # Parameters
    /// - `executor` — bounded worker executor used to run blocking work.
    /// - `root` — root directory captured by the filesystem authority.
    pub async fn rooted(executor: LocalIoExecutor, root: PathBuf) -> crate::LocalResult<Self> {
        let filesystem = run_on_worker(&executor, move || crate::LocalFileSystem::rooted(&root)).await?;
        Ok(Self::new(filesystem, executor, None))
    }
    /// Submits a bounded, worker-backed directory traversal.
    ///
    /// The ordinary executor handle pool reserves one enumeration reader by
    /// default, or the complete explicit `max_open_directories` value. A zero
    /// or unavailable reservation is rejected with `CapacityExceeded`; the
    /// reservation is returned when the walker closes. Temporary resolution
    /// handles and cleanup handles follow their separate existing budgets.
    ///
    /// # Parameters
    /// - `manager` — operation manager that owns the admission scope.
    /// - `operation` — operation options used for admission and cancellation.
    /// - `path` — path resolved under the captured filesystem authority.
    /// - `options` — operation policy applied to the local filesystem request.
    pub fn submit_open_walker(
        &self,
        manager: &qubit_operation::OperationManager,
        operation: qubit_operation::OperationOptions,
        path: PathBuf,
        options: crate::LocalListOptions,
    ) -> Result<super::LocalOperation<super::stream::AsyncLocalDirectoryWalker>, super::LocalSubmitError<PathBuf>> {
        use qubit_operation::OperationOutcome;
        let scoped = match manager.with_group(&self.executor.inner.group) {
            Ok(value) => value,
            Err(qubit_operation::GroupBindingError::WrongManager) => {
                return Err(super::LocalSubmitError::new(
                    path,
                    super::LocalSubmitRejection::WrongManager,
                ));
            }
            Err(qubit_operation::GroupBindingError::ScopeEnded) => {
                return Err(super::LocalSubmitError::new(
                    path,
                    super::LocalSubmitRejection::ScopeEnded,
                ));
            }
            Err(qubit_operation::GroupBindingError::Closed) => {
                return Err(super::LocalSubmitError::new(path, super::LocalSubmitRejection::Closed));
            }
            Err(qubit_operation::GroupBindingError::CapacityExceeded) => {
                return Err(super::LocalSubmitError::new(
                    path,
                    super::LocalSubmitRejection::CapacityExceeded,
                ));
            }
        };
        if self.filesystem.scope() == crate::path::LocalFileSystemScope::Host
            && self.host_base.is_none()
            && path.is_relative()
        {
            return Err(super::LocalSubmitError::new(
                path,
                super::LocalSubmitRejection::AuthorityNotCaptured,
            ));
        }
        let execution_path = bind_host_path(&self.host_base, &path);
        let directory_readers = options.max_open_directories().unwrap_or(1);
        let Some(slot) = self.executor.reserve_slot() else {
            return Err(super::LocalSubmitError::new(
                path,
                super::LocalSubmitRejection::CapacityExceeded,
            ));
        };
        let Some(budget) = self.executor.reserve_bytes(super::stream::LOCAL_WALK_BATCH_BYTES) else {
            return Err(super::LocalSubmitError::new(
                path,
                super::LocalSubmitRejection::CapacityExceeded,
            ));
        };
        let Some(handle) = self.executor.reserve_handles(directory_readers) else {
            return Err(super::LocalSubmitError::new(
                path,
                super::LocalSubmitRejection::CapacityExceeded,
            ));
        };
        let handle = Arc::new(handle);
        let Some(fact) = self.executor.reserve_fact() else {
            return Err(super::LocalSubmitError::new(
                path,
                super::LocalSubmitRejection::CapacityExceeded,
            ));
        };
        let budget = Arc::new(Mutex::new(budget));
        let fact: Arc<dyn Send + Sync> = Arc::new(fact);
        let observer = super::LocalFactObserver::with_permit(Arc::clone(&fact));
        let filesystem = self.filesystem.clone();
        let executor = self.executor.clone();
        let worker_budget = Arc::clone(&budget);
        let worker_handle = Arc::clone(&handle);
        let job = super::internal::prepared_local_job::PreparedLocalJob::new_scoped_with_shared_handle_permit(
            Arc::clone(&self.executor.inner),
            path.clone(),
            slot,
            Arc::clone(&budget),
            Arc::clone(&handle),
            observer.clone(),
            move |_, scope| match filesystem
                .list_with_options(&execution_path, &options.with_max_open_directories(directory_readers))
            {
                Err(error) => {
                    shrink_byte_budget(&worker_budget);
                    OperationOutcome::Failed(error)
                }
                Ok(walker) => {
                    let resource =
                        super::stream::WorkerWalkDriver::managed(scope, executor, walker, worker_budget, worker_handle);
                    match resource.and_then(super::stream::AsyncLocalDirectoryWalker::new) {
                        Ok(walker) => OperationOutcome::Succeeded(walker),
                        Err(error) => OperationOutcome::Failed(crate::LocalFileError::from_io(
                            crate::LocalFileOperation::List,
                            Some(execution_path),
                            None,
                            error,
                        )),
                    }
                }
            },
        );
        match scoped.try_submit_external(operation, Arc::clone(&job)) {
            Ok(handle) => Ok(super::LocalOperation::new(handle, budget)),
            Err(error) => {
                let (reason, _) = error.into_parts();
                let request = job.take_request().unwrap_or(path);
                Err(super::LocalSubmitError::new(
                    request,
                    match reason {
                        qubit_operation::AdmissionError::ScopeEnded => super::LocalSubmitRejection::ScopeEnded,
                        qubit_operation::AdmissionError::Stopped => super::LocalSubmitRejection::Stopped,
                        qubit_operation::AdmissionError::CapacityExceeded | qubit_operation::AdmissionError::Busy => {
                            super::LocalSubmitRejection::CapacityExceeded
                        }
                        _ => super::LocalSubmitRejection::Closed,
                    },
                ))
            }
        }
    }
    /// Submits a worker-backed reader open. The returned stream owns a managed
    /// worker resource and performs every native read and seek on the executor.
    ///
    /// # Parameters
    /// - `manager` — operation manager that owns the admission scope.
    /// - `qubit_operation` — input value used by this operation.
    /// - `operation` — operation options used for admission and cancellation.
    /// - `path` — path resolved under the captured filesystem authority.
    /// - `options` — operation policy applied to the local filesystem request.
    /// - `crate` — input value used by this operation.
    pub fn submit_open_reader(
        &self,
        manager: &qubit_operation::OperationManager,
        operation: qubit_operation::OperationOptions,
        path: PathBuf,
        options: crate::LocalReadOptions,
    ) -> Result<super::LocalOperation<super::stream::AsyncLocalFileReader>, super::LocalSubmitError<PathBuf>> {
        use qubit_operation::OperationOutcome;
        let scoped = match manager.with_group(&self.executor.inner.group) {
            Ok(value) => value,
            Err(qubit_operation::GroupBindingError::WrongManager) => {
                return Err(super::LocalSubmitError::new(
                    path,
                    super::LocalSubmitRejection::WrongManager,
                ));
            }
            Err(qubit_operation::GroupBindingError::ScopeEnded) => {
                return Err(super::LocalSubmitError::new(
                    path,
                    super::LocalSubmitRejection::ScopeEnded,
                ));
            }
            Err(qubit_operation::GroupBindingError::Closed) => {
                return Err(super::LocalSubmitError::new(path, super::LocalSubmitRejection::Closed));
            }
            Err(qubit_operation::GroupBindingError::CapacityExceeded) => {
                return Err(super::LocalSubmitError::new(
                    path,
                    super::LocalSubmitRejection::CapacityExceeded,
                ));
            }
        };
        if self.filesystem.scope() == crate::path::LocalFileSystemScope::Host
            && self.host_base.is_none()
            && path.is_relative()
        {
            return Err(super::LocalSubmitError::new(
                path,
                super::LocalSubmitRejection::AuthorityNotCaptured,
            ));
        }
        let execution_path = bind_host_path(&self.host_base, &path);
        let Some(slot) = self.executor.reserve_slot() else {
            return Err(super::LocalSubmitError::new(
                path,
                super::LocalSubmitRejection::CapacityExceeded,
            ));
        };
        let Some(bytes) = self.executor.reserve_bytes(super::stream::LOCAL_STREAM_BUFFER_SIZE) else {
            return Err(super::LocalSubmitError::new(
                path,
                super::LocalSubmitRejection::CapacityExceeded,
            ));
        };
        let Some(handle) = self.executor.reserve_handle() else {
            return Err(super::LocalSubmitError::new(
                path,
                super::LocalSubmitRejection::CapacityExceeded,
            ));
        };
        let handle = Arc::new(handle);
        let Some(fact) = self.executor.reserve_fact() else {
            return Err(super::LocalSubmitError::new(
                path,
                super::LocalSubmitRejection::CapacityExceeded,
            ));
        };
        let bytes = Arc::new(Mutex::new(bytes));
        let fact: Arc<dyn Send + Sync> = Arc::new(fact);
        let observer = super::LocalFactObserver::with_permit(Arc::clone(&fact));
        let filesystem = self.filesystem.clone();
        let executor = self.executor.clone();
        let worker_budget = Arc::clone(&bytes);
        let worker_handle = Arc::clone(&handle);
        let job = super::internal::prepared_local_job::PreparedLocalJob::new_scoped_with_shared_handle_permit(
            Arc::clone(&self.executor.inner),
            path.clone(),
            slot,
            Arc::clone(&bytes),
            Arc::clone(&handle),
            observer.clone(),
            move |_, scope| {
                let opened = filesystem.open_reader_with_options(&execution_path, &options);
                match opened {
                    Err(error) => {
                        shrink_byte_budget(&worker_budget);
                        OperationOutcome::Failed(error)
                    }
                    Ok(reader) => {
                        let resource = super::stream::WorkerStreamDriver::managed(
                            scope,
                            executor,
                            super::stream::LocalNativeStream::Reader(reader),
                            worker_budget,
                            worker_handle,
                            None,
                            false,
                        );
                        match resource.and_then(super::stream::AsyncLocalFileReader::new) {
                            Ok(reader) => OperationOutcome::Succeeded(reader),
                            Err(error) => OperationOutcome::Failed(crate::LocalFileError::from_io(
                                crate::LocalFileOperation::OpenReader,
                                Some(execution_path),
                                None,
                                error,
                            )),
                        }
                    }
                }
            },
        );
        match scoped.try_submit_external(operation, Arc::clone(&job)) {
            Ok(handle) => Ok(super::LocalOperation::new(handle, bytes)),
            Err(error) => {
                let (reason, _) = error.into_parts();
                let request = job.take_request().unwrap_or(path);
                Err(super::LocalSubmitError::new(
                    request,
                    match reason {
                        qubit_operation::AdmissionError::ScopeEnded => super::LocalSubmitRejection::ScopeEnded,
                        qubit_operation::AdmissionError::Stopped => super::LocalSubmitRejection::Stopped,
                        qubit_operation::AdmissionError::CapacityExceeded | qubit_operation::AdmissionError::Busy => {
                            super::LocalSubmitRejection::CapacityExceeded
                        }
                        _ => super::LocalSubmitRejection::Closed,
                    },
                ))
            }
        }
    }
    /// Submits a worker-backed writer open. Accepted bytes are sent to the
    /// native writer on the executor; the returned observer reports the final
    /// commit fact.
    ///
    /// # Parameters
    /// - `manager` — operation manager that owns the admission scope.
    /// - `qubit_operation` — input value used by this operation.
    /// - `operation` — operation options used for admission and cancellation.
    /// - `path` — path resolved under the captured filesystem authority.
    /// - `options` — operation policy applied to the local filesystem request.
    /// - `crate` — input value used by this operation.
    pub fn submit_open_writer(
        &self,
        manager: &qubit_operation::OperationManager,
        operation: qubit_operation::OperationOptions,
        path: PathBuf,
        options: crate::LocalWriteOptions,
    ) -> Result<
        (
            super::LocalOperation<super::stream::AsyncLocalFileWriter>,
            super::LocalFactObserver,
        ),
        super::LocalSubmitError<PathBuf>,
    > {
        use qubit_operation::OperationOutcome;
        let scoped = match manager.with_group(&self.executor.inner.group) {
            Ok(value) => value,
            Err(qubit_operation::GroupBindingError::WrongManager) => {
                return Err(super::LocalSubmitError::new(
                    path,
                    super::LocalSubmitRejection::WrongManager,
                ));
            }
            Err(qubit_operation::GroupBindingError::ScopeEnded) => {
                return Err(super::LocalSubmitError::new(
                    path,
                    super::LocalSubmitRejection::ScopeEnded,
                ));
            }
            Err(qubit_operation::GroupBindingError::Closed) => {
                return Err(super::LocalSubmitError::new(path, super::LocalSubmitRejection::Closed));
            }
            Err(qubit_operation::GroupBindingError::CapacityExceeded) => {
                return Err(super::LocalSubmitError::new(
                    path,
                    super::LocalSubmitRejection::CapacityExceeded,
                ));
            }
        };
        if self.filesystem.scope() == crate::path::LocalFileSystemScope::Host
            && self.host_base.is_none()
            && path.is_relative()
        {
            return Err(super::LocalSubmitError::new(
                path,
                super::LocalSubmitRejection::AuthorityNotCaptured,
            ));
        }
        let execution_path = bind_host_path(&self.host_base, &path);
        let Some(slot) = self.executor.reserve_slot() else {
            return Err(super::LocalSubmitError::new(
                path,
                super::LocalSubmitRejection::CapacityExceeded,
            ));
        };
        let Some(budget) = self.executor.reserve_bytes(super::stream::LOCAL_STREAM_BUFFER_SIZE) else {
            return Err(super::LocalSubmitError::new(
                path,
                super::LocalSubmitRejection::CapacityExceeded,
            ));
        };
        let Some(handle) = self.executor.reserve_handle() else {
            return Err(super::LocalSubmitError::new(
                path,
                super::LocalSubmitRejection::CapacityExceeded,
            ));
        };
        let handle = Arc::new(handle);
        let Some(fact) = self.executor.reserve_fact() else {
            return Err(super::LocalSubmitError::new(
                path,
                super::LocalSubmitRejection::CapacityExceeded,
            ));
        };
        let budget = Arc::new(Mutex::new(budget));
        let fact: Arc<dyn Send + Sync> = Arc::new(fact);
        let observer = super::LocalFactObserver::with_permit(Arc::clone(&fact));
        let filesystem = self.filesystem.clone();
        let executor = self.executor.clone();
        let worker_budget = Arc::clone(&budget);
        let worker_handle = Arc::clone(&handle);
        let worker_observer = observer.clone();
        let job = super::internal::prepared_local_job::PreparedLocalJob::new_scoped_with_shared_handle_permit(
            Arc::clone(&self.executor.inner),
            path.clone(),
            slot,
            Arc::clone(&budget),
            Arc::clone(&handle),
            observer.clone(),
            move |_, scope| match filesystem.open_writer_with_options(&execution_path, &options) {
                Err(error) => {
                    shrink_byte_budget(&worker_budget);
                    OperationOutcome::Failed(error)
                }
                Ok(writer) => {
                    let resource = super::stream::WorkerStreamDriver::managed(
                        scope,
                        executor,
                        super::stream::LocalNativeStream::Writer(Box::new(writer)),
                        worker_budget,
                        worker_handle,
                        Some(worker_observer),
                        options.mode() == crate::LocalWriteMode::Append,
                    );
                    match resource.and_then(super::stream::AsyncLocalFileWriter::new) {
                        Ok(writer) => OperationOutcome::Succeeded(writer),
                        Err(error) => OperationOutcome::Failed(crate::LocalFileError::from_io(
                            crate::LocalFileOperation::OpenWriter,
                            Some(execution_path),
                            None,
                            error,
                        )),
                    }
                }
            },
        );
        match scoped.try_submit_external(operation, Arc::clone(&job)) {
            Ok(handle) => Ok((super::LocalOperation::new(handle, budget), observer)),
            Err(error) => {
                let (reason, _) = error.into_parts();
                let request = job.take_request().unwrap_or(path);
                Err(super::LocalSubmitError::new(
                    request,
                    match reason {
                        qubit_operation::AdmissionError::ScopeEnded => super::LocalSubmitRejection::ScopeEnded,
                        qubit_operation::AdmissionError::Stopped => super::LocalSubmitRejection::Stopped,
                        qubit_operation::AdmissionError::CapacityExceeded | qubit_operation::AdmissionError::Busy => {
                            super::LocalSubmitRejection::CapacityExceeded
                        }
                        _ => super::LocalSubmitRejection::Closed,
                    },
                ))
            }
        }
    }
    /// Submits a complete file read and returns its independent local fact
    /// observer alongside the operation handle. The bytes are delivered by
    /// the operation outcome; the observer reports operation facts and does
    /// not contain or replace that result. Memory ceilings are reported as
    /// `OwnedBytes` resource limits; explicit read ceilings retain read-limit
    /// diagnostics. Use [`Self::submit_read_all`] when
    /// the read should check cooperative cancellation between read blocks.
    ///
    /// # Parameters
    /// - `manager` — operation manager that owns the admission scope.
    /// - `operation` — operation options used for admission and cancellation.
    /// - `request` — owned input retained if submission is rejected.
    /// - `limits` — per-operation resource ceilings.
    pub fn submit_read_all_with_observer(
        &self,
        manager: &qubit_operation::OperationManager,
        operation: qubit_operation::OperationOptions,
        request: super::LocalReadRequest,
        limits: super::LocalOperationOptions,
    ) -> Result<
        (super::LocalOperation<Vec<u8>>, super::LocalFactObserver),
        super::LocalSubmitError<super::LocalReadRequest>,
    > {
        let scoped = match manager.with_group(&self.executor.inner.group) {
            Ok(value) => value,
            Err(qubit_operation::GroupBindingError::WrongManager) => {
                return Err(super::LocalSubmitError::new(
                    request,
                    super::LocalSubmitRejection::WrongManager,
                ));
            }
            Err(qubit_operation::GroupBindingError::ScopeEnded) => {
                return Err(super::LocalSubmitError::new(
                    request,
                    super::LocalSubmitRejection::ScopeEnded,
                ));
            }
            Err(qubit_operation::GroupBindingError::Closed) => {
                return Err(super::LocalSubmitError::new(
                    request,
                    super::LocalSubmitRejection::Closed,
                ));
            }
            Err(qubit_operation::GroupBindingError::CapacityExceeded) => {
                return Err(super::LocalSubmitError::new(
                    request,
                    super::LocalSubmitRejection::CapacityExceeded,
                ));
            }
        };
        let byte_limit = limits
            .max_owned_bytes
            .unwrap_or(self.executor.inner.config.max_owned_bytes);
        if byte_limit == 0 || byte_limit > self.executor.inner.config.max_owned_bytes {
            return Err(super::LocalSubmitError::new(
                request,
                super::LocalSubmitRejection::CapacityExceeded,
            ));
        }
        if self.filesystem.scope() == crate::path::LocalFileSystemScope::Host
            && self.host_base.is_none()
            && request.path.is_relative()
        {
            return Err(super::LocalSubmitError::new(
                request,
                super::LocalSubmitRejection::AuthorityNotCaptured,
            ));
        }
        let read_limit = request
            .read
            .max_bytes()
            .map_or(byte_limit as u64, |limit| limit.min(byte_limit as u64));
        let execution_path = match (&self.host_base, request.path.is_relative()) {
            (Some(base), true) => base.join(&request.path),
            _ => request.path.clone(),
        };
        let execution_options = request.read.with_max_bytes(read_limit);
        let memory_limit_dominates = !request.read.max_bytes().is_some_and(|limit| limit <= byte_limit as u64);
        let Some(slot) = self.executor.reserve_slot() else {
            return Err(super::LocalSubmitError::new(
                request,
                super::LocalSubmitRejection::CapacityExceeded,
            ));
        };
        let Some(byte_permit) = self.executor.reserve_bytes(0) else {
            return Err(super::LocalSubmitError::new(
                request,
                super::LocalSubmitRejection::CapacityExceeded,
            ));
        };
        let byte_permit = Arc::new(Mutex::new(byte_permit));
        let capacity_error = Arc::new(Mutex::new(None));
        let invalid_handle_limit = match limits.max_open_handles {
            Some(limit) => limit == 0 || limit > self.executor.inner.config.max_open_handles,
            None => false,
        };
        if invalid_handle_limit {
            return Err(super::LocalSubmitError::new(
                request,
                super::LocalSubmitRejection::CapacityExceeded,
            ));
        }
        let Some(handle_permit) = self.executor.reserve_handle() else {
            return Err(super::LocalSubmitError::new(
                request,
                super::LocalSubmitRejection::CapacityExceeded,
            ));
        };
        let Some(fact_permit) = self.executor.reserve_fact() else {
            return Err(super::LocalSubmitError::new(
                request,
                super::LocalSubmitRejection::CapacityExceeded,
            ));
        };
        let fact_permit: Arc<dyn Send + Sync> = Arc::new(fact_permit);
        let observer = super::LocalFactObserver::with_permit(Arc::clone(&fact_permit));
        let filesystem = self.filesystem.clone();
        let budget_for_task = Arc::clone(&byte_permit);
        let error_for_task = Arc::clone(&capacity_error);
        let job = super::internal::prepared_local_job::PreparedLocalJob::new(
            Arc::clone(&self.executor.inner),
            request,
            slot,
            Arc::clone(&byte_permit),
            handle_permit,
            observer.clone(),
            move |_request| {
                let read = filesystem.read_all_with_options_and_reserver(
                    &execution_path,
                    &execution_options,
                    |output, additional| {
                        reserve_vec_capacity(output, additional, &budget_for_task, byte_limit, &error_for_task)
                    },
                );
                match read {
                    Ok(value) => qubit_operation::OperationOutcome::Succeeded(value),
                    Err(error) => {
                        let mut permit = match budget_for_task.lock() {
                            Ok(permit) => permit,
                            Err(poisoned) => poisoned.into_inner(),
                        };
                        let amount = permit.amount();
                        permit.shrink(amount);
                        let recorded_limit = match capacity_error.lock() {
                            Ok(mut capacity_error) => capacity_error.take(),
                            Err(poisoned) => poisoned.into_inner().take(),
                        };
                        let structured = match recorded_limit {
                            Some(limit) => crate::error::LocalFileError::from_resource_limit(
                                crate::error::LocalFileOperation::Read,
                                Some(execution_path.clone()),
                                limit,
                            ),
                            None => match error.read_limit_error() {
                                Some(read_limit) if memory_limit_dominates => {
                                    let observed = read_limit.observed();
                                    let requested = usize::try_from(observed).unwrap_or(usize::MAX);
                                    let facts = crate::error::LocalResourceLimitError::new(
                                        crate::error::LocalResourceKind::OwnedBytes,
                                        byte_limit,
                                        byte_limit,
                                        requested,
                                    );
                                    let facts = if observed > usize::MAX as u64 {
                                        facts.with_overflow()
                                    } else {
                                        facts.with_observed(requested)
                                    };
                                    crate::error::LocalFileError::from_resource_limit(
                                        crate::error::LocalFileOperation::Read,
                                        error.path().map(std::path::Path::to_path_buf),
                                        facts,
                                    )
                                }
                                _ => error,
                            },
                        };
                        qubit_operation::OperationOutcome::Failed(structured)
                    }
                }
            },
        );
        match scoped.try_submit_external(operation, Arc::clone(&job)) {
            Ok(handle) => Ok((super::LocalOperation::new(handle, byte_permit), observer)),
            Err(error) => {
                let (reason, _prepared) = error.into_parts();
                let request = job.take_request().expect("rejected local job preserves its request");
                Err(super::LocalSubmitError::new(
                    request,
                    match reason {
                        qubit_operation::AdmissionError::Closed
                        | qubit_operation::AdmissionError::ExecutorUnavailable => super::LocalSubmitRejection::Closed,
                        qubit_operation::AdmissionError::CapacityExceeded | qubit_operation::AdmissionError::Busy => {
                            super::LocalSubmitRejection::CapacityExceeded
                        }
                        qubit_operation::AdmissionError::ScopeEnded => super::LocalSubmitRejection::ScopeEnded,
                        qubit_operation::AdmissionError::Stopped => super::LocalSubmitRejection::Stopped,
                        _ => super::LocalSubmitRejection::Closed,
                    },
                ))
            }
        }
    }
}
/// Runs the bind host path behavior while preserving the operation lifecycle
/// invariants.
/// # Parameters
/// - `base` — supplies data or policy for this operation.
/// - `path` — supplies data or policy for this operation.
fn bind_host_path(base: &Option<PathBuf>, path: &PathBuf) -> PathBuf {
    match (base, path.is_relative()) {
        (Some(base), true) => base.join(path),
        _ => path.clone(),
    }
}
/// Releases all executor-owned bytes after a native open fails.
///
/// # Parameters
/// - `permit` — shared reservation retained by the pending operation.
fn shrink_byte_budget(permit: &Arc<Mutex<super::internal::permits::Permit>>) {
    let mut permit = match permit.lock() {
        Ok(permit) => permit,
        Err(poisoned) => poisoned.into_inner(),
    };
    let amount = permit.amount();
    permit.shrink(amount);
}
/// Runs the reserve vec capacity behavior while preserving the operation
/// lifecycle invariants.
/// # Parameters
/// - `output` — supplies data or policy for this operation.
/// - `additional` — supplies data or policy for this operation.
/// - `permit` — supplies data or policy for this operation.
/// - `maximum` — supplies data or policy for this operation.
fn reserve_vec_capacity(
    output: &mut Vec<u8>,
    additional: usize,
    permit: &Arc<Mutex<super::internal::permits::Permit>>,
    maximum: usize,
    capacity_error: &Arc<Mutex<Option<crate::error::LocalResourceLimitError>>>,
) -> std::io::Result<()> {
    let old_capacity = output.capacity();
    let requested_capacity = match output.len().checked_add(additional) {
        Some(requested_capacity) => requested_capacity,
        None => return Err(std::io::Error::other("read capacity overflow")),
    };
    let target_capacity = requested_capacity.max(old_capacity);
    let delta = target_capacity - old_capacity;
    let mut lease = match permit.lock() {
        Ok(lease) => lease,
        Err(poisoned) => poisoned.into_inner(),
    };
    if target_capacity > maximum {
        *capacity_error.lock().unwrap_or_else(|error| error.into_inner()) =
            Some(crate::error::LocalResourceLimitError::new(
                crate::error::LocalResourceKind::OwnedBytes,
                maximum,
                maximum.saturating_sub(old_capacity),
                delta,
            ));
        return Err(std::io::Error::other("executor owned-byte limit exceeded"));
    }
    if let Err((limit, remaining, requested)) = lease.try_grow(delta) {
        *capacity_error.lock().unwrap_or_else(|error| error.into_inner()) =
            Some(crate::error::LocalResourceLimitError::new(
                crate::error::LocalResourceKind::OwnedBytes,
                limit,
                remaining,
                requested,
            ));
        return Err(std::io::Error::other("executor owned-byte limit exceeded"));
    }
    if let Err(error) = output.try_reserve_exact(additional) {
        lease.shrink(delta);
        return Err(std::io::Error::other(error));
    }
    let actual_delta = output.capacity().saturating_sub(old_capacity);
    reconcile_vec_capacity(&mut lease, delta, actual_delta)
}

/// Reconciles a byte permit with the capacity returned by an allocator.
///
/// # Parameters
/// - `lease` — byte permit to reconcile.
/// - `requested_delta` — capacity initially reserved before allocation.
/// - `actual_delta` — capacity reported after allocation.
fn reconcile_vec_capacity(
    lease: &mut super::internal::permits::Permit,
    requested_delta: usize,
    actual_delta: usize,
) -> std::io::Result<()> {
    if actual_delta < requested_delta {
        lease.shrink(requested_delta - actual_delta);
    } else if actual_delta > requested_delta && !lease.grow(actual_delta - requested_delta) {
        let reserved = lease.amount();
        lease.shrink(reserved);
        return Err(std::io::Error::other(
            "allocator capacity exceeds executor owned-byte limit",
        ));
    }
    Ok(())
}
/// Runs the run on worker behavior while preserving the operation lifecycle
/// invariants.
/// # Type Parameters
/// - `T` — type used by this operation.
/// - `F` — type used by this operation.
/// # Parameters
/// - `executor` — supplies data or policy for this operation.
/// - `task` — supplies data or policy for this operation.
async fn run_on_worker<T, F>(executor: &LocalIoExecutor, task: F) -> crate::LocalResult<T>
where
    T: Send + 'static,
    F: FnOnce() -> crate::LocalResult<T> + Send + 'static,
{
    let completion = Arc::new(Mutex::new(Completion {
        value: None,
        waker: None,
    }));
    let completion_for_worker = Arc::clone(&completion);
    let scheduled = executor.enqueue_task(move || {
        let value = std::panic::catch_unwind(std::panic::AssertUnwindSafe(task)).unwrap_or_else(|_| {
            Err(crate::LocalFileError::from_io(
                crate::LocalFileOperation::Configure,
                None,
                None,
                std::io::Error::other("async filesystem constructor panicked"),
            ))
        });
        let waker = {
            let mut completion = completion_for_worker.lock().unwrap_or_else(|e| e.into_inner());
            completion.value = Some(value);
            completion.waker.take()
        };
        if let Some(waker) = waker {
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| waker.wake()));
        }
    });
    if scheduled.is_err() {
        return Err(crate::LocalFileError::new(
            crate::LocalFileErrorKind::ResourceLimit,
            crate::LocalFileOperation::Configure,
        )
        .with_reason("local worker queue is full while constructing async filesystem"));
    }
    poll_fn(|cx| {
        let mut new_waker = Some(cx.waker().clone());
        let mut completion = completion.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(value) = completion.value.take() {
            drop(completion);
            if let Some(waker) = new_waker.take() {
                let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| drop(waker)));
            }
            return std::task::Poll::Ready(value);
        }
        let old_waker = if completion
            .waker
            .as_ref()
            .is_some_and(|waker| waker.will_wake(new_waker.as_ref().expect("new waker")))
        {
            None
        } else {
            std::mem::replace(&mut completion.waker, new_waker.take())
        };
        drop(completion);
        if let Some(waker) = old_waker {
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| drop(waker)));
        }
        if let Some(waker) = new_waker {
            let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| drop(waker)));
        }
        std::task::Poll::Pending
    })
    .await
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;
    use std::sync::Arc;
    use std::sync::Mutex;
    use std::task::Context;
    use std::task::Poll;
    use std::task::Wake;
    use std::task::Waker;

    use qubit_operation::ManagerOptions;
    use qubit_operation::OperationManager;

    use super::AsyncLocalFileSystem;
    use super::reserve_vec_capacity;
    use crate::operation::LocalExecutionConfig;
    use crate::operation::LocalIoExecutor;

    struct WorkerQueueGate(Option<std::sync::mpsc::SyncSender<()>>);

    impl Drop for WorkerQueueGate {
        fn drop(&mut self) {
            if let Some(release) = self.0.take() {
                let _ = release.send(());
            }
        }
    }

    fn filesystem_with_blocked_worker(byte_budget: usize) -> (OperationManager, AsyncLocalFileSystem, WorkerQueueGate) {
        use std::sync::mpsc::sync_channel;
        use std::time::Duration;

        let manager = OperationManager::new(ManagerOptions::default()).unwrap();
        let executor = LocalIoExecutor::new(
            manager.clone(),
            LocalExecutionConfig {
                worker_count: 1,
                queue_capacity: 1,
                waiter_capacity: 1,
                max_owned_bytes: byte_budget,
                max_open_handles: 1,
                max_fact_records: 1,
                ..Default::default()
            },
        )
        .unwrap();
        let filesystem = block_on(AsyncLocalFileSystem::host(executor.clone())).unwrap();
        let (entered_tx, entered_rx) = sync_channel(1);
        let (release_tx, release_rx) = sync_channel(1);
        assert!(
            executor
                .inner
                .queue
                .push(
                    Box::new(move || {
                        entered_tx.send(()).unwrap();
                        let _ = release_rx.recv();
                    }),
                    false,
                    2,
                )
                .is_ok()
        );
        entered_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        (manager, filesystem, WorkerQueueGate(Some(release_tx)))
    }

    fn assert_cancelled_open_outcome<T: Send + 'static>(
        operation: &mut crate::operation::LocalOperation<T>,
        reason: qubit_operation::StopReason,
    ) {
        use qubit_operation::OperationOutcome;

        assert!(matches!(
            operation.try_take_outcome().unwrap(),
            Some(OperationOutcome::Stopped(report)) if report.reason == reason
        ));
    }

    fn assert_open_budgets_returned(filesystem: &AsyncLocalFileSystem) {
        let inner = &filesystem.executor.inner;
        assert_eq!(inner.byte_slots.used(), 0, "cancelled open must return byte capacity");
        assert_eq!(
            inner.handle_slots.used(),
            0,
            "cancelled open must return handle capacity"
        );
        assert_eq!(inner.fact_slots.used(), 0, "cancelled open must return fact capacity");
        assert_eq!(inner.queue_slots.used(), 0, "cancelled open must return queue capacity");
        assert_eq!(
            inner.waiter_slots.used(),
            0,
            "cancelled open must return waiter capacity"
        );
    }

    #[test]
    fn test_queued_cancelled_reader_reuses_open_budget_before_taking_outcome() {
        use qubit_operation::OperationOutcome;
        use qubit_operation::StopReason;

        let directory = tempfile::tempdir().unwrap();
        let queued_path = directory.path().join("queued-reader");
        let replacement_path = directory.path().join("replacement-reader");
        std::fs::write(&queued_path, b"queued").unwrap();
        std::fs::write(&replacement_path, b"replacement").unwrap();
        let (manager, filesystem, gate) =
            filesystem_with_blocked_worker(super::super::stream::LOCAL_STREAM_BUFFER_SIZE);
        let mut cancelled = filesystem
            .submit_open_reader(
                &manager,
                Default::default(),
                queued_path,
                crate::LocalReadOptions::default(),
            )
            .unwrap();
        assert_eq!(
            cancelled.request_stop(StopReason::Timeout),
            qubit_operation::StopRequestStatus::Accepted
        );
        drop(gate);
        cancelled.wait_settlement().unwrap();
        assert_open_budgets_returned(&filesystem);

        let mut replacement = filesystem
            .submit_open_reader(
                &manager,
                Default::default(),
                replacement_path,
                crate::LocalReadOptions::default(),
            )
            .expect("the first post-cancellation submission should reuse the byte and handle budgets");
        assert_cancelled_open_outcome(&mut cancelled, StopReason::Timeout);
        let mut reader = match replacement.wait_outcome().unwrap() {
            OperationOutcome::Succeeded(reader) => reader,
            _ => panic!("replacement reader should open"),
        };
        block_on(reader.close()).unwrap();
        drop(reader);
        replacement.wait_settlement().unwrap();
    }

    #[test]
    fn test_queued_cancelled_writer_reuses_open_budget_without_writing_target() {
        use qubit_operation::OperationOutcome;
        use qubit_operation::StopReason;

        let directory = tempfile::tempdir().unwrap();
        let cancelled_path = directory.path().join("cancelled-writer");
        let replacement_path = directory.path().join("replacement-writer");
        let (manager, filesystem, gate) =
            filesystem_with_blocked_worker(super::super::stream::LOCAL_STREAM_BUFFER_SIZE);
        let (mut cancelled, _) = filesystem
            .submit_open_writer(
                &manager,
                Default::default(),
                cancelled_path.clone(),
                crate::LocalWriteOptions::default(),
            )
            .unwrap();
        assert_eq!(
            cancelled.request_stop(StopReason::Timeout),
            qubit_operation::StopRequestStatus::Accepted
        );
        drop(gate);
        cancelled.wait_settlement().unwrap();
        assert_open_budgets_returned(&filesystem);

        let (mut replacement, _) = filesystem
            .submit_open_writer(
                &manager,
                Default::default(),
                replacement_path,
                crate::LocalWriteOptions::default(),
            )
            .expect("the first post-cancellation submission should reuse the byte and handle budgets");
        assert_cancelled_open_outcome(&mut cancelled, StopReason::Timeout);
        assert!(
            !cancelled_path.exists(),
            "a cancelled queued writer must not create its target"
        );
        let mut writer = match replacement.wait_outcome().unwrap() {
            OperationOutcome::Succeeded(writer) => writer,
            _ => panic!("replacement writer should open"),
        };
        block_on(writer.close()).unwrap();
        drop(writer);
        replacement.wait_settlement().unwrap();
    }

    #[test]
    fn test_queued_cancelled_walker_reuses_open_budget_before_taking_outcome() {
        use qubit_operation::OperationOutcome;
        use qubit_operation::StopReason;

        let directory = tempfile::tempdir().unwrap();
        std::fs::create_dir(directory.path().join("queued-walker")).unwrap();
        std::fs::create_dir(directory.path().join("replacement-walker")).unwrap();
        let (manager, filesystem, gate) = filesystem_with_blocked_worker(super::super::stream::LOCAL_WALK_BATCH_BYTES);
        let mut cancelled = filesystem
            .submit_open_walker(
                &manager,
                Default::default(),
                directory.path().join("queued-walker"),
                crate::LocalListOptions::default(),
            )
            .unwrap();
        assert_eq!(
            cancelled.request_stop(StopReason::Timeout),
            qubit_operation::StopRequestStatus::Accepted
        );
        drop(gate);
        cancelled.wait_settlement().unwrap();
        assert_open_budgets_returned(&filesystem);

        let mut replacement = filesystem
            .submit_open_walker(
                &manager,
                Default::default(),
                directory.path().join("replacement-walker"),
                crate::LocalListOptions::default(),
            )
            .expect("the first post-cancellation submission should reuse the byte and handle budgets");
        assert_cancelled_open_outcome(&mut cancelled, StopReason::Timeout);
        let mut walker = match replacement.wait_outcome().unwrap() {
            OperationOutcome::Succeeded(walker) => walker,
            _ => panic!("replacement walker should open"),
        };
        block_on(walker.close()).unwrap();
        drop(walker);
        replacement.wait_settlement().unwrap();
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
    /// Runs the test read buffer growth reserves actual capacity before
    /// allocation behavior while preserving the operation lifecycle invariants.
    #[test]
    fn test_read_buffer_growth_reserves_actual_capacity_before_allocation() {
        let manager = OperationManager::new(ManagerOptions::default()).unwrap();
        let executor = LocalIoExecutor::new(
            manager,
            LocalExecutionConfig {
                max_owned_bytes: 16,
                ..Default::default()
            },
        )
        .unwrap();
        let permit = Arc::new(Mutex::new(executor.reserve_bytes(0).unwrap()));
        let capacity_error = Arc::new(Mutex::new(None));
        let mut output = Vec::new();

        reserve_vec_capacity(&mut output, 4, &permit, 16, &capacity_error).unwrap();
        output.extend_from_slice(b"four");
        let first_capacity = output.capacity();
        assert_eq!(executor.inner.byte_slots.used(), first_capacity);

        reserve_vec_capacity(&mut output, 12, &permit, 16, &capacity_error).unwrap();
        assert!(output.capacity() > first_capacity);
        assert_eq!(executor.inner.byte_slots.used(), output.capacity());
        output.extend_from_slice(b"twelve bytes");
        let reserved = executor.inner.byte_slots.used();
        let capacity = output.capacity();

        assert!(reserve_vec_capacity(&mut output, 1, &permit, 16, &capacity_error).is_err());
        assert_eq!(output.capacity(), capacity);
        assert_eq!(executor.inner.byte_slots.used(), reserved);
        drop(permit);
        assert_eq!(executor.inner.byte_slots.used(), 0);
    }

    #[test]
    fn test_read_buffer_capacity_reconciliation_handles_synthetic_over_allocation() {
        let pool = crate::operation::internal::permits::PermitPool::new(7);
        let mut permit = pool.try_acquire(4).expect("requested capacity");

        super::reconcile_vec_capacity(&mut permit, 4, 7).expect("allocator over-allocation");
        assert_eq!(permit.amount(), 7);
        assert_eq!(pool.used(), 7);

        drop(permit);
        assert_eq!(pool.used(), 0);
    }

    #[test]
    fn test_read_buffer_capacity_reconciliation_refunds_when_over_allocation_exceeds_limit() {
        let pool = crate::operation::internal::permits::PermitPool::new(6);
        let mut permit = pool.try_acquire(4).expect("requested capacity");

        let error =
            super::reconcile_vec_capacity(&mut permit, 4, 7).expect_err("unaffordable allocator over-allocation");
        assert_eq!(
            error.to_string(),
            "allocator capacity exceeds executor owned-byte limit"
        );
        assert_eq!(permit.amount(), 0);
        assert_eq!(pool.used(), 0);
    }

    #[test]
    fn test_async_openers_create_and_release_worker_managed_resources() {
        use qubit_operation::OperationOutcome;

        use crate::operation::LocalReadRequest;
        use crate::options::LocalListOptions;
        use crate::options::LocalReadOptions;
        use crate::options::LocalWriteMode;
        use crate::options::LocalWriteOptions;

        let directory = tempfile::tempdir().unwrap();
        let input = directory.path().join("input");
        std::fs::write(&input, b"reader input").unwrap();
        let manager = OperationManager::new(ManagerOptions::default()).unwrap();
        let executor = LocalIoExecutor::new(manager.clone(), LocalExecutionConfig::default()).unwrap();
        let filesystem = block_on(AsyncLocalFileSystem::rooted(executor, directory.path().to_path_buf())).unwrap();

        let mut reader_open = filesystem
            .submit_open_reader(
                &manager,
                Default::default(),
                PathBuf::from("input"),
                LocalReadOptions::default(),
            )
            .unwrap();
        let reader = match reader_open.wait_outcome().unwrap() {
            OperationOutcome::Succeeded(reader) => reader,
            _ => panic!("expected reader to open"),
        };
        drop(reader);
        reader_open.wait_settlement().unwrap();

        let mut missing_open = filesystem
            .submit_open_reader(
                &manager,
                Default::default(),
                PathBuf::from("missing"),
                LocalReadOptions::default(),
            )
            .unwrap();
        assert!(matches!(
            missing_open.wait_outcome().unwrap(),
            OperationOutcome::Failed(_)
        ));

        let mut writer_open = filesystem
            .submit_open_writer(
                &manager,
                Default::default(),
                PathBuf::from("output"),
                LocalWriteOptions::new(LocalWriteMode::CreateOrReplace),
            )
            .unwrap()
            .0;
        let writer = match writer_open.wait_outcome().unwrap() {
            OperationOutcome::Succeeded(writer) => writer,
            _ => panic!("expected writer to open"),
        };
        drop(writer);
        writer_open.wait_settlement().unwrap();

        let mut walker_open = filesystem
            .submit_open_walker(
                &manager,
                Default::default(),
                PathBuf::from("."),
                LocalListOptions::default(),
            )
            .unwrap();
        let walker = match walker_open.wait_outcome().unwrap() {
            OperationOutcome::Succeeded(walker) => walker,
            _ => panic!("expected walker to open"),
        };
        drop(walker);
        walker_open.wait_settlement().unwrap();

        let mut read = filesystem
            .submit_read_all(
                &manager,
                Default::default(),
                LocalReadRequest {
                    path: PathBuf::from("input"),
                    read: Default::default(),
                },
                Default::default(),
            )
            .unwrap()
            .0;
        assert!(matches!(
            read.wait_outcome().unwrap(),
            OperationOutcome::Succeeded(bytes) if bytes == b"reader input"
        ));
    }

    #[test]
    fn test_async_facade_constructors_and_worker_task_outcomes() {
        use crate::LocalFileErrorKind;

        let manager = OperationManager::new(ManagerOptions::default()).unwrap();
        let executor = LocalIoExecutor::new(manager, LocalExecutionConfig::default()).unwrap();
        let host = block_on(AsyncLocalFileSystem::host(executor.clone())).unwrap();
        assert_eq!(
            host.host_base.as_deref(),
            Some(std::env::current_dir().unwrap().as_path())
        );

        let missing_root = tempfile::tempdir().unwrap().path().join("missing-root");
        let error = match block_on(AsyncLocalFileSystem::rooted(executor.clone(), missing_root)) {
            Err(error) => error,
            Ok(_) => panic!("missing rooted filesystem must fail to construct"),
        };
        assert_ne!(error.kind(), LocalFileErrorKind::ResourceLimit);

        let value = block_on(super::run_on_worker(&executor, || Ok::<_, crate::LocalFileError>(17))).unwrap();
        assert_eq!(value, 17);

        let error = block_on(super::run_on_worker(&executor, || -> crate::LocalResult<()> {
            panic!("constructor task panic")
        }))
        .unwrap_err();
        assert_eq!(error.kind(), LocalFileErrorKind::Io);
        assert_eq!(error.operation(), crate::LocalFileOperation::Configure);

        assert_eq!(
            super::bind_host_path(&Some(PathBuf::from("captured")), &PathBuf::from("relative")),
            PathBuf::from("captured/relative")
        );
        assert_eq!(
            super::bind_host_path(&Some(PathBuf::from("captured")), &PathBuf::from("/absolute")),
            PathBuf::from("/absolute")
        );
        assert_eq!(
            super::bind_host_path(&None, &PathBuf::from("relative")),
            PathBuf::from("relative")
        );
    }

    #[test]
    fn test_async_constructor_rejects_when_worker_permits_are_exhausted() {
        use std::sync::mpsc;
        use std::time::Duration;

        let manager = OperationManager::new(ManagerOptions::default()).unwrap();
        let executor = LocalIoExecutor::new(
            manager,
            LocalExecutionConfig {
                worker_count: 1,
                queue_capacity: 1,
                waiter_capacity: 1,
                ..Default::default()
            },
        )
        .unwrap();
        let (started_tx, started_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        executor
            .enqueue_task(move || {
                started_tx.send(()).unwrap();
                release_rx.recv().unwrap();
            })
            .unwrap();
        started_rx.recv_timeout(Duration::from_secs(5)).unwrap();
        let queued = executor.reserve_slot().unwrap();
        let waiting = executor.reserve_slot().unwrap();

        let error = block_on(super::run_on_worker(&executor, || Ok::<_, crate::LocalFileError>(()))).unwrap_err();
        assert_eq!(error.kind(), crate::LocalFileErrorKind::ResourceLimit);
        drop((queued, waiting));
        release_tx.send(()).unwrap();
    }

    #[test]
    fn test_read_buffer_reservation_recovers_poisoned_permit_and_checks_overflow() {
        let pool = crate::operation::internal::permits::PermitPool::new(8);
        let mut bytes = vec![1];
        let permit = Arc::new(std::sync::Mutex::new(
            pool.try_acquire(bytes.capacity()).expect("initial permit"),
        ));
        #[cfg(panic = "unwind")]
        crate::operation::poison_test_mutex(&permit);

        let capacity_error = Arc::new(std::sync::Mutex::new(None));
        let error = super::reserve_vec_capacity(&mut bytes, usize::MAX, &permit, 8, &capacity_error)
            .expect_err("length overflow");
        assert_eq!(error.to_string(), "read capacity overflow");
        assert_eq!(pool.used(), 1);
        super::reserve_vec_capacity(&mut bytes, 1, &permit, 8, &capacity_error).expect("grow within byte budget");
        assert_eq!(pool.used(), bytes.capacity());
        assert_eq!(bytes, [1]);
        drop(permit);
        assert_eq!(pool.used(), 0);
    }

    #[test]
    fn test_worker_constructor_replaces_pending_waker_before_completion() {
        use std::sync::mpsc;
        use std::time::Duration;

        struct ChannelWake(mpsc::Sender<()>);
        impl Wake for ChannelWake {
            fn wake(self: Arc<Self>) {
                let _ = self.0.send(());
            }
            fn wake_by_ref(self: &Arc<Self>) {
                let _ = self.0.send(());
            }
        }

        let manager = OperationManager::new(ManagerOptions::default()).unwrap();
        let executor = LocalIoExecutor::new(
            manager,
            LocalExecutionConfig {
                worker_count: 1,
                queue_capacity: 1,
                ..Default::default()
            },
        )
        .unwrap();
        let (started_tx, started_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        executor
            .enqueue_task(move || {
                started_tx.send(()).unwrap();
                release_rx.recv().unwrap();
            })
            .unwrap();
        started_rx.recv_timeout(Duration::from_secs(5)).unwrap();

        let (first_wake_tx, first_wake_rx) = mpsc::channel();
        let (second_wake_tx, second_wake_rx) = mpsc::channel();
        let first_waker = Waker::from(Arc::new(ChannelWake(first_wake_tx)));
        let second_waker = Waker::from(Arc::new(ChannelWake(second_wake_tx)));
        let mut first_context = Context::from_waker(&first_waker);
        let mut second_context = Context::from_waker(&second_waker);
        let mut future = Box::pin(super::run_on_worker(&executor, || Ok::<_, crate::LocalFileError>(23)));

        assert!(matches!(future.as_mut().poll(&mut first_context), Poll::Pending));
        assert!(matches!(future.as_mut().poll(&mut second_context), Poll::Pending));
        assert!(matches!(future.as_mut().poll(&mut second_context), Poll::Pending));
        release_tx.send(()).unwrap();

        second_wake_rx
            .recv_timeout(Duration::from_secs(5))
            .expect("completion should wake the latest registered waker");
        assert!(first_wake_rx.try_recv().is_err());
        assert!(matches!(future.as_mut().poll(&mut second_context), Poll::Ready(Ok(23))));
    }

    #[test]
    fn test_read_all_with_observer_reports_success_failure_and_authority_rejection() {
        use qubit_operation::OperationOutcome;

        use crate::operation::LocalReadRequest;
        use crate::operation::LocalSubmitRejection;
        use crate::options::LocalReadAllOptions;

        let directory = tempfile::tempdir().unwrap();
        std::fs::write(directory.path().join("input"), b"bounded payload").unwrap();
        let manager = OperationManager::new(ManagerOptions::default()).unwrap();
        let executor = LocalIoExecutor::new(manager.clone(), LocalExecutionConfig::default()).unwrap();
        let filesystem = block_on(AsyncLocalFileSystem::rooted(executor, directory.path().to_path_buf())).unwrap();

        let (mut operation, observer) = filesystem
            .submit_read_all_with_observer(
                &manager,
                Default::default(),
                LocalReadRequest {
                    path: PathBuf::from("input"),
                    read: LocalReadAllOptions::new().with_max_bytes(64),
                },
                Default::default(),
            )
            .unwrap();
        assert!(matches!(
            operation.wait_outcome().unwrap(),
            OperationOutcome::Succeeded(bytes) if bytes == b"bounded payload"
        ));
        operation.wait_settlement().unwrap();
        drop(observer);

        let mut missing = filesystem
            .submit_read_all_with_observer(
                &manager,
                Default::default(),
                LocalReadRequest {
                    path: PathBuf::from("missing"),
                    read: Default::default(),
                },
                Default::default(),
            )
            .unwrap()
            .0;
        assert!(matches!(missing.wait_outcome().unwrap(), OperationOutcome::Failed(_)));
        missing.wait_settlement().unwrap();

        let host_without_capture = AsyncLocalFileSystem::new(
            crate::LocalFileSystem::host().unwrap(),
            LocalIoExecutor::new(manager.clone(), LocalExecutionConfig::default()).unwrap(),
            None,
        );
        let request = LocalReadRequest {
            path: PathBuf::from("relative"),
            read: Default::default(),
        };
        let error = match host_without_capture.submit_read_all_with_observer(
            &manager,
            Default::default(),
            request,
            Default::default(),
        ) {
            Err(error) => error,
            Ok(_) => panic!("uncaptured relative Host path must be rejected"),
        };
        assert_eq!(error.rejection(), LocalSubmitRejection::AuthorityNotCaptured);
        assert_eq!(error.into_request().path, PathBuf::from("relative"));
    }

    #[test]
    fn test_async_facade_submissions_reject_wrong_manager_and_closed_group() {
        use crate::operation::LocalReadRequest;
        use crate::operation::LocalSubmitRejection;
        use crate::options::LocalListOptions;
        use crate::options::LocalReadOptions;
        use crate::options::LocalWriteMode;
        use crate::options::LocalWriteOptions;

        let directory = tempfile::tempdir().unwrap();
        let manager = OperationManager::new(ManagerOptions::default()).unwrap();
        let other_manager = OperationManager::new(ManagerOptions::default()).unwrap();
        let executor = LocalIoExecutor::new(manager.clone(), LocalExecutionConfig::default()).unwrap();
        let filesystem = block_on(AsyncLocalFileSystem::rooted(
            executor.clone(),
            directory.path().to_path_buf(),
        ))
        .unwrap();

        macro_rules! rejection {
            ($result:expr) => {
                match $result {
                    Err(error) => error.rejection(),
                    Ok(_) => panic!("submission with another manager must be rejected"),
                }
            };
        }

        for rejection in [
            rejection!(filesystem.submit_open_walker(
                &other_manager,
                Default::default(),
                PathBuf::from("walker"),
                LocalListOptions::default()
            )),
            rejection!(filesystem.submit_open_reader(
                &other_manager,
                Default::default(),
                PathBuf::from("reader"),
                LocalReadOptions::default()
            )),
            rejection!(filesystem.submit_open_writer(
                &other_manager,
                Default::default(),
                PathBuf::from("writer"),
                LocalWriteOptions::new(LocalWriteMode::CreateOrReplace),
            )),
            rejection!(filesystem.submit_read_all_with_observer(
                &other_manager,
                Default::default(),
                LocalReadRequest {
                    path: PathBuf::from("read-all"),
                    read: Default::default()
                },
                Default::default(),
            )),
        ] {
            assert_eq!(rejection, LocalSubmitRejection::WrongManager);
        }

        let shutdown = executor
            .request_shutdown(qubit_operation::ShutdownMode::Drain)
            .wait()
            .unwrap();
        assert_eq!(shutdown.snapshot.active_operations, 0);
        let submissions = [
            rejection!(filesystem.submit_open_walker(
                &manager,
                Default::default(),
                PathBuf::from("walker"),
                LocalListOptions::default()
            )),
            rejection!(filesystem.submit_open_reader(
                &manager,
                Default::default(),
                PathBuf::from("reader"),
                LocalReadOptions::default()
            )),
            rejection!(filesystem.submit_open_writer(
                &manager,
                Default::default(),
                PathBuf::from("writer"),
                LocalWriteOptions::new(LocalWriteMode::CreateOrReplace),
            )),
            rejection!(filesystem.submit_read_all_with_observer(
                &manager,
                Default::default(),
                LocalReadRequest {
                    path: PathBuf::from("read-all"),
                    read: Default::default()
                },
                Default::default(),
            )),
        ];
        assert!(
            submissions
                .into_iter()
                .all(|rejection| rejection == LocalSubmitRejection::Closed)
        );
    }

    #[test]
    fn test_async_facade_submissions_reject_ended_scope_and_group_capacity() {
        use qubit_operation::GroupOptions;
        use qubit_operation::OperationOptions;
        use qubit_operation::OperationOutcome;

        use crate::operation::LocalReadRequest;
        use crate::operation::LocalSubmitRejection;
        use crate::options::LocalListOptions;
        use crate::options::LocalReadOptions;
        use crate::options::LocalWriteMode;
        use crate::options::LocalWriteOptions;

        let directory = tempfile::tempdir().unwrap();
        let manager = OperationManager::new(ManagerOptions::default()).unwrap();
        let executor = LocalIoExecutor::new(manager.clone(), LocalExecutionConfig::default()).unwrap();
        let filesystem = block_on(AsyncLocalFileSystem::rooted(
            executor.clone(),
            directory.path().to_path_buf(),
        ))
        .unwrap();

        let ended_scope_manager = match manager
            .run(OperationOptions::default(), |scope| Ok::<_, ()>(scope.manager()))
            .unwrap()
        {
            OperationOutcome::Succeeded(scoped_manager) => scoped_manager,
            _ => panic!("scope manager capture operation must succeed"),
        };

        macro_rules! assert_path_rejection {
            ($result:expr, $path:expr, $expected:expr, $request:expr $(,)?) => {{
                let error = match $result {
                    Err(error) => error,
                    Ok(_) => panic!("submission must be rejected"),
                };
                assert_eq!(error.rejection(), $expected);
                assert_eq!($request(error), $path);
            }};
        }
        let owned_path = |error: crate::operation::LocalSubmitError<PathBuf>| error.into_request();
        let read_path = |error: crate::operation::LocalSubmitError<LocalReadRequest>| error.into_request().path;

        assert_path_rejection!(
            filesystem.submit_open_walker(
                &ended_scope_manager,
                Default::default(),
                PathBuf::from("ended-walker"),
                LocalListOptions::default(),
            ),
            PathBuf::from("ended-walker"),
            LocalSubmitRejection::ScopeEnded,
            owned_path,
        );
        assert_path_rejection!(
            filesystem.submit_open_reader(
                &ended_scope_manager,
                Default::default(),
                PathBuf::from("ended-reader"),
                LocalReadOptions::default(),
            ),
            PathBuf::from("ended-reader"),
            LocalSubmitRejection::ScopeEnded,
            owned_path,
        );
        assert_path_rejection!(
            filesystem.submit_open_writer(
                &ended_scope_manager,
                Default::default(),
                PathBuf::from("ended-writer"),
                LocalWriteOptions::new(LocalWriteMode::CreateOrReplace),
            ),
            PathBuf::from("ended-writer"),
            LocalSubmitRejection::ScopeEnded,
            owned_path,
        );
        assert_path_rejection!(
            filesystem.submit_read_all_with_observer(
                &ended_scope_manager,
                Default::default(),
                LocalReadRequest {
                    path: PathBuf::from("ended-read-all"),
                    read: Default::default(),
                },
                Default::default(),
            ),
            PathBuf::from("ended-read-all"),
            LocalSubmitRejection::ScopeEnded,
            read_path,
        );

        let constrained_manager = ManagerOptions {
            max_groups_per_operation: 1,
            ..Default::default()
        };
        let constrained = OperationManager::new(constrained_manager).unwrap();
        let executor = LocalIoExecutor::new(constrained.clone(), LocalExecutionConfig::default()).unwrap();
        let filesystem = block_on(AsyncLocalFileSystem::rooted(executor, directory.path().to_path_buf())).unwrap();
        let other_group = constrained.group(GroupOptions { capacity: 1 }).unwrap();
        let manager_at_capacity = constrained.with_group(&other_group).unwrap();

        assert_path_rejection!(
            filesystem.submit_open_walker(
                &manager_at_capacity,
                Default::default(),
                PathBuf::from("capacity-walker"),
                LocalListOptions::default(),
            ),
            PathBuf::from("capacity-walker"),
            LocalSubmitRejection::CapacityExceeded,
            owned_path,
        );
        assert_path_rejection!(
            filesystem.submit_open_reader(
                &manager_at_capacity,
                Default::default(),
                PathBuf::from("capacity-reader"),
                LocalReadOptions::default(),
            ),
            PathBuf::from("capacity-reader"),
            LocalSubmitRejection::CapacityExceeded,
            owned_path,
        );
        assert_path_rejection!(
            filesystem.submit_open_writer(
                &manager_at_capacity,
                Default::default(),
                PathBuf::from("capacity-writer"),
                LocalWriteOptions::new(LocalWriteMode::CreateOrReplace),
            ),
            PathBuf::from("capacity-writer"),
            LocalSubmitRejection::CapacityExceeded,
            owned_path,
        );
        assert_path_rejection!(
            filesystem.submit_read_all_with_observer(
                &manager_at_capacity,
                Default::default(),
                LocalReadRequest {
                    path: PathBuf::from("capacity-read-all"),
                    read: Default::default(),
                },
                Default::default(),
            ),
            PathBuf::from("capacity-read-all"),
            LocalSubmitRejection::CapacityExceeded,
            read_path,
        );
    }

    #[test]
    fn test_open_submissions_reject_invalid_scope_authority_and_exhausted_handles() {
        use crate::operation::LocalSubmitRejection;
        use crate::options::LocalListOptions;
        use crate::options::LocalReadOptions;

        let directory = tempfile::tempdir().unwrap();
        let manager = OperationManager::new(ManagerOptions::default()).unwrap();
        let other_manager = OperationManager::new(ManagerOptions::default()).unwrap();
        let executor = LocalIoExecutor::new(
            manager.clone(),
            LocalExecutionConfig {
                max_open_handles: 1,
                ..Default::default()
            },
        )
        .unwrap();
        let filesystem = block_on(AsyncLocalFileSystem::rooted(
            executor.clone(),
            directory.path().to_path_buf(),
        ))
        .unwrap();

        let wrong_manager_path = PathBuf::from("wrong-manager");
        let rejected = match filesystem.submit_open_reader(
            &other_manager,
            Default::default(),
            wrong_manager_path.clone(),
            LocalReadOptions::default(),
        ) {
            Err(error) => error,
            Ok(_) => panic!("wrong manager must reject reader submission"),
        };
        assert_eq!(rejected.rejection(), LocalSubmitRejection::WrongManager);
        assert_eq!(rejected.into_request(), wrong_manager_path);

        let uncaptured_host =
            AsyncLocalFileSystem::new(crate::LocalFileSystem::host().unwrap(), executor.clone(), None);
        let relative_path = PathBuf::from("relative-input");
        let rejected = match uncaptured_host.submit_open_walker(
            &manager,
            Default::default(),
            relative_path.clone(),
            LocalListOptions::default(),
        ) {
            Err(error) => error,
            Ok(_) => panic!("uncaptured relative Host path must be rejected"),
        };
        assert_eq!(rejected.rejection(), LocalSubmitRejection::AuthorityNotCaptured);
        assert_eq!(rejected.into_request(), relative_path);

        let held_handle = executor.reserve_handle().unwrap();
        let exhausted_path = PathBuf::from("handle-limit");
        let rejected = match filesystem.submit_open_reader(
            &manager,
            Default::default(),
            exhausted_path.clone(),
            LocalReadOptions::default(),
        ) {
            Err(error) => error,
            Ok(_) => panic!("exhausted handle capacity must reject reader submission"),
        };
        assert_eq!(rejected.rejection(), LocalSubmitRejection::CapacityExceeded);
        assert_eq!(rejected.into_request(), exhausted_path);
        drop(held_handle);

        let accepted_path = PathBuf::from(".");
        let mut opened = filesystem
            .submit_open_walker(&manager, Default::default(), accepted_path, LocalListOptions::default())
            .expect("released handle permit should allow another open");
        match opened.wait_outcome().unwrap() {
            qubit_operation::OperationOutcome::Succeeded(walker) => drop(walker),
            _ => panic!("expected walker after permit release"),
        }
        opened.wait_settlement().unwrap();
    }

    #[test]
    fn test_async_writer_commit_publishes_bytes_and_reports_fact() {
        use qubit_io::AsyncOutput;
        use qubit_operation::OperationOutcome;

        use crate::operation::LocalPublicationState;
        use crate::options::LocalWriteMode;
        use crate::options::LocalWriteOptions;

        let directory = tempfile::tempdir().unwrap();
        let manager = OperationManager::new(ManagerOptions::default()).unwrap();
        let executor = LocalIoExecutor::new(manager.clone(), LocalExecutionConfig::default()).unwrap();
        let filesystem = block_on(AsyncLocalFileSystem::rooted(executor, directory.path().to_path_buf())).unwrap();
        let (mut operation, observer) = filesystem
            .submit_open_writer(
                &manager,
                Default::default(),
                PathBuf::from("published"),
                LocalWriteOptions::new(LocalWriteMode::CreateOrReplace),
            )
            .unwrap();
        let mut writer = match operation.wait_outcome().unwrap() {
            OperationOutcome::Succeeded(writer) => writer,
            _ => panic!("expected writer to open"),
        };

        block_on(writer.write_fully_async(b"committed payload")).unwrap();
        let outcome = block_on(writer.commit()).unwrap();
        assert_eq!(outcome.bytes_written(), b"committed payload".len() as u64);
        assert_eq!(
            std::fs::read(directory.path().join("published")).unwrap(),
            b"committed payload"
        );
        let fact = observer.snapshot();
        assert_eq!(fact.publication, LocalPublicationState::Published);
        assert_eq!(fact.modified_count, 1);
        assert_eq!(fact.modified_bytes, b"committed payload".len() as u64);
        drop(writer);
        operation.wait_settlement().unwrap();
    }

    #[test]
    fn test_async_reader_worker_preserves_seek_order_and_closes_explicitly() {
        use qubit_io::AsyncInput;
        use qubit_operation::OperationOutcome;

        use crate::options::LocalReadOptions;

        let directory = tempfile::tempdir().unwrap();
        std::fs::write(directory.path().join("input"), b"worker stream").unwrap();
        let manager = OperationManager::new(ManagerOptions::default()).unwrap();
        let executor = LocalIoExecutor::new(manager.clone(), LocalExecutionConfig::default()).unwrap();
        let filesystem = block_on(AsyncLocalFileSystem::rooted(executor, directory.path().to_path_buf())).unwrap();
        let mut operation = filesystem
            .submit_open_reader(
                &manager,
                Default::default(),
                PathBuf::from("input"),
                LocalReadOptions::default(),
            )
            .unwrap();
        let mut reader = match operation.wait_outcome().unwrap() {
            OperationOutcome::Succeeded(reader) => reader,
            _ => panic!("expected reader to open"),
        };

        let mut first = [0; 6];
        assert_eq!(block_on(reader.read_fully_async(&mut first)).unwrap(), 6);
        assert_eq!(&first, b"worker");
        assert_eq!(block_on(reader.seek(std::io::SeekFrom::Start(7))).unwrap(), 7);
        let mut remainder = [0; 16];
        let count = block_on(reader.read_fully_async(&mut remainder)).unwrap();
        assert_eq!(&remainder[..count], b"stream");
        assert_eq!(block_on(reader.read_fully_async(&mut remainder)).unwrap(), 0);
        block_on(reader.close()).unwrap();
        operation.wait_settlement().unwrap();
    }

    #[test]
    fn test_async_read_capacity_rejection_returns_the_owned_request() {
        use crate::operation::LocalReadRequest;
        use crate::operation::LocalSubmitRejection;

        let directory = tempfile::tempdir().unwrap();
        let manager = OperationManager::new(ManagerOptions::default()).unwrap();
        let executor = LocalIoExecutor::new(manager.clone(), LocalExecutionConfig::default()).unwrap();
        let filesystem = block_on(AsyncLocalFileSystem::rooted(executor, directory.path().to_path_buf())).unwrap();
        let request = LocalReadRequest {
            path: PathBuf::from("requested-input"),
            read: Default::default(),
        };
        let error = match filesystem.submit_read_all_with_observer(
            &manager,
            Default::default(),
            request,
            crate::operation::LocalOperationOptions {
                max_owned_bytes: Some(0),
                ..Default::default()
            },
        ) {
            Err(error) => error,
            Ok(_) => panic!("zero owned-byte limit must reject the read"),
        };
        assert_eq!(error.rejection(), LocalSubmitRejection::CapacityExceeded);
        assert_eq!(error.into_request().path, PathBuf::from("requested-input"));
    }

    #[test]
    fn test_read_all_enforces_per_operation_handle_limits_and_read_cap() {
        use qubit_operation::OperationOutcome;

        use crate::operation::LocalReadRequest;
        use crate::operation::LocalSubmitRejection;
        use crate::options::LocalReadAllOptions;

        let directory = tempfile::tempdir().unwrap();
        std::fs::write(directory.path().join("input"), b"payload").unwrap();
        let manager = OperationManager::new(ManagerOptions::default()).unwrap();
        let executor = LocalIoExecutor::new(manager.clone(), LocalExecutionConfig::default()).unwrap();
        let filesystem = block_on(AsyncLocalFileSystem::rooted(
            executor.clone(),
            directory.path().to_path_buf(),
        ))
        .unwrap();
        let request = || LocalReadRequest {
            path: PathBuf::from("input"),
            read: Default::default(),
        };
        for max_open_handles in [Some(0), Some(executor.inner.config.max_open_handles + 1)] {
            let result = filesystem.submit_read_all_with_observer(
                &manager,
                Default::default(),
                request(),
                crate::operation::LocalOperationOptions {
                    max_open_handles,
                    ..Default::default()
                },
            );
            let error = match result {
                Err(error) => error,
                Ok(_) => panic!("invalid handle limit must be rejected"),
            };
            assert_eq!(error.rejection(), LocalSubmitRejection::CapacityExceeded);
            assert_eq!(error.into_request().path, PathBuf::from("input"));
        }
        let (mut operation, _) = filesystem
            .submit_read_all_with_observer(
                &manager,
                Default::default(),
                LocalReadRequest {
                    path: PathBuf::from("input"),
                    read: LocalReadAllOptions::new().with_max_bytes(3),
                },
                Default::default(),
            )
            .unwrap();
        assert!(matches!(operation.wait_outcome().unwrap(), OperationOutcome::Failed(_)));
        operation.wait_settlement().unwrap();
        assert_eq!(executor.inner.handle_slots.used(), 0);
    }

    #[test]
    fn test_open_submissions_reject_when_worker_buffer_exceeds_byte_budget() {
        use crate::operation::LocalSubmitRejection;
        use crate::options::LocalListOptions;
        use crate::options::LocalReadOptions;
        use crate::options::LocalWriteMode;
        use crate::options::LocalWriteOptions;

        let directory = tempfile::tempdir().unwrap();
        let manager = OperationManager::new(ManagerOptions::default()).unwrap();
        let executor = LocalIoExecutor::new(
            manager.clone(),
            LocalExecutionConfig {
                max_owned_bytes: 1,
                ..Default::default()
            },
        )
        .unwrap();
        let filesystem = block_on(AsyncLocalFileSystem::rooted(executor, directory.path().to_path_buf())).unwrap();

        let reader_path = PathBuf::from("reader-request");
        let reader_error = match filesystem.submit_open_reader(
            &manager,
            Default::default(),
            reader_path.clone(),
            LocalReadOptions::default(),
        ) {
            Err(error) => error,
            Ok(_) => panic!("reader must reject when its worker buffer exceeds the byte budget"),
        };
        assert_eq!(reader_error.rejection(), LocalSubmitRejection::CapacityExceeded);
        assert_eq!(reader_error.into_request(), reader_path);

        let writer_path = PathBuf::from("writer-request");
        let writer_error = match filesystem.submit_open_writer(
            &manager,
            Default::default(),
            writer_path.clone(),
            LocalWriteOptions::new(LocalWriteMode::CreateOrReplace),
        ) {
            Err(error) => error,
            Ok(_) => panic!("writer must reject when its worker buffer exceeds the byte budget"),
        };
        assert_eq!(writer_error.rejection(), LocalSubmitRejection::CapacityExceeded);
        assert_eq!(writer_error.into_request(), writer_path);

        let walker_path = PathBuf::from("walker-request");
        let walker_error = match filesystem.submit_open_walker(
            &manager,
            Default::default(),
            walker_path.clone(),
            LocalListOptions::default(),
        ) {
            Err(error) => error,
            Ok(_) => panic!("walker must reject when its worker buffer exceeds the byte budget"),
        };
        assert_eq!(walker_error.rejection(), LocalSubmitRejection::CapacityExceeded);
        assert_eq!(walker_error.into_request(), walker_path);
    }

    #[test]
    fn test_open_submissions_reject_when_fact_capacity_is_exhausted() {
        use crate::operation::LocalSubmitRejection;
        use crate::options::LocalListOptions;
        use crate::options::LocalReadOptions;
        use crate::options::LocalWriteMode;
        use crate::options::LocalWriteOptions;

        let directory = tempfile::tempdir().unwrap();
        let manager = OperationManager::new(ManagerOptions::default()).unwrap();
        let executor = LocalIoExecutor::new(
            manager.clone(),
            LocalExecutionConfig {
                max_fact_records: 1,
                ..Default::default()
            },
        )
        .unwrap();
        let filesystem = block_on(AsyncLocalFileSystem::rooted(
            executor.clone(),
            directory.path().to_path_buf(),
        ))
        .unwrap();
        let held_fact = executor.reserve_fact().unwrap();

        let reader_path = PathBuf::from("reader-request");
        let reader_error = match filesystem.submit_open_reader(
            &manager,
            Default::default(),
            reader_path.clone(),
            LocalReadOptions::default(),
        ) {
            Err(error) => error,
            Ok(_) => panic!("reader must reject when no fact record is available"),
        };
        assert_eq!(reader_error.rejection(), LocalSubmitRejection::CapacityExceeded);
        assert_eq!(reader_error.into_request(), reader_path);

        let writer_path = PathBuf::from("writer-request");
        let writer_error = match filesystem.submit_open_writer(
            &manager,
            Default::default(),
            writer_path.clone(),
            LocalWriteOptions::new(LocalWriteMode::CreateOrReplace),
        ) {
            Err(error) => error,
            Ok(_) => panic!("writer must reject when no fact record is available"),
        };
        assert_eq!(writer_error.rejection(), LocalSubmitRejection::CapacityExceeded);
        assert_eq!(writer_error.into_request(), writer_path);

        let walker_path = PathBuf::from("walker-request");
        let walker_error = match filesystem.submit_open_walker(
            &manager,
            Default::default(),
            walker_path.clone(),
            LocalListOptions::default(),
        ) {
            Err(error) => error,
            Ok(_) => panic!("walker must reject when no fact record is available"),
        };
        assert_eq!(walker_error.rejection(), LocalSubmitRejection::CapacityExceeded);
        assert_eq!(walker_error.into_request(), walker_path);

        drop(held_fact);
        assert!(executor.reserve_fact().is_some());
    }
}
