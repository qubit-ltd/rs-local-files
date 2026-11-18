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
    /// # Parameters
    /// - `manager` — operation manager that owns the admission scope.
    /// - `qubit_operation` — input value used by this operation.
    /// - `operation` — operation options used for admission and cancellation.
    /// - `path` — path resolved under the captured filesystem authority.
    /// - `options` — operation policy applied to the local filesystem request.
    /// - `crate` — input value used by this operation.
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
        let job = super::internal::prepared_local_job::PreparedLocalJob::new_scoped_with_shared_handle_permit(
            Arc::clone(&self.executor.inner),
            path.clone(),
            slot,
            Arc::clone(&budget),
            Arc::clone(&handle),
            observer.clone(),
            move |_, scope| match filesystem.list_with_options(&execution_path, &options) {
                Err(error) => OperationOutcome::Failed(error),
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
                    Err(error) => OperationOutcome::Failed(error),
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
                Err(error) => OperationOutcome::Failed(error),
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
    /// observer.
    ///
    /// # Parameters
    /// - `manager` — operation manager that owns the admission scope.
    /// - `qubit_operation` — input value used by this operation.
    /// - `operation` — operation options used for admission and cancellation.
    /// - `request` — owned input retained if submission is rejected.
    /// - `super` — input value used by this operation.
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
        if limits.max_open_handles == Some(0)
            || limits
                .max_open_handles
                .is_some_and(|limit| limit > self.executor.inner.config.max_open_handles)
        {
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
                    |output, additional| reserve_vec_capacity(output, additional, &budget_for_task, byte_limit),
                );
                match read {
                    Ok(value) => qubit_operation::OperationOutcome::Succeeded(value),
                    Err(error) => {
                        let mut permit = budget_for_task.lock().unwrap_or_else(|e| e.into_inner());
                        let amount = permit.amount();
                        permit.shrink(amount);
                        qubit_operation::OperationOutcome::Failed(error)
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
) -> std::io::Result<()> {
    let old_capacity = output.capacity();
    let requested_capacity = output
        .len()
        .checked_add(additional)
        .ok_or_else(|| std::io::Error::other("read capacity overflow"))?;
    let target_capacity = requested_capacity.max(old_capacity);
    let delta = target_capacity - old_capacity;
    let mut lease = permit.lock().unwrap_or_else(|e| e.into_inner());
    if target_capacity > maximum || !lease.grow(delta) {
        return Err(std::io::Error::other("executor owned-byte limit exceeded"));
    }
    if let Err(error) = output.try_reserve_exact(additional) {
        lease.shrink(delta);
        return Err(std::io::Error::other(error));
    }
    let actual_delta = output.capacity().saturating_sub(old_capacity);
    if actual_delta < delta {
        lease.shrink(delta - actual_delta);
    } else if actual_delta > delta && !lease.grow(actual_delta - delta) {
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
        let mut output = Vec::new();

        reserve_vec_capacity(&mut output, 4, &permit, 16).unwrap();
        output.extend_from_slice(b"four");
        let first_capacity = output.capacity();
        assert_eq!(executor.inner.byte_slots.used(), first_capacity);

        reserve_vec_capacity(&mut output, 12, &permit, 16).unwrap();
        assert!(output.capacity() > first_capacity);
        assert_eq!(executor.inner.byte_slots.used(), output.capacity());
        output.extend_from_slice(b"twelve bytes");
        let reserved = executor.inner.byte_slots.used();
        let capacity = output.capacity();

        assert!(reserve_vec_capacity(&mut output, 1, &permit, 16).is_err());
        assert_eq!(output.capacity(), capacity);
        assert_eq!(executor.inner.byte_slots.used(), reserved);
        drop(permit);
        assert_eq!(executor.inner.byte_slots.used(), 0);
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
}
