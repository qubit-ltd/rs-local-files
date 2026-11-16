use std::path::PathBuf;
use std::sync::Arc;

use super::LocalIoExecutor;

/// Asynchronous facade retaining its synchronous filesystem authority and executor.
#[derive(Clone)]
pub struct AsyncLocalFileSystem {
    pub(crate) filesystem: crate::LocalFileSystem,
    pub(crate) executor: LocalIoExecutor,
    host_base: Option<PathBuf>,
}

impl AsyncLocalFileSystem {
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
}

impl crate::LocalFileSystem {
    /// Moves this filesystem and its retained authority into an async facade.
    pub fn into_async(self, executor: LocalIoExecutor) -> crate::LocalResult<AsyncLocalFileSystem> {
        let host_base = if self.scope() == crate::path::LocalFileSystemScope::Host {
            Some(std::env::current_dir().map_err(|source| {
                crate::LocalFileError::from_io(
                    crate::LocalFileOperation::CurrentDirectory,
                    None,
                    None,
                    source,
                )
                .with_reason("failed to capture process directory for async local filesystem")
            })?)
        } else {
            None
        };
        Ok(AsyncLocalFileSystem::new(self, executor, host_base))
    }
}

impl AsyncLocalFileSystem {
    /// Submits a complete file read and returns its independent local fact observer.
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
        let Some(byte_permit) = self.executor.reserve_bytes(byte_limit) else {
            return Err(super::LocalSubmitError::new(
                request,
                super::LocalSubmitRejection::CapacityExceeded,
            ));
        };
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
        let observer = super::LocalFactObserver::new();
        let filesystem = self.filesystem.clone();
        let job = super::internal::prepared_local_job::PreparedLocalJob::new(
            Arc::clone(&self.executor.inner),
            request,
            slot,
            byte_permit,
            handle_permit,
            move |_request| match filesystem
                .read_all_with_options(&execution_path, &execution_options)
            {
                Ok(value) => qubit_operation::OperationOutcome::Succeeded(value),
                Err(error) => qubit_operation::OperationOutcome::Failed(error),
            },
        );
        match scoped.try_submit_external(operation, Arc::clone(&job)) {
            Ok(handle) => Ok((handle, observer)),
            Err(error) => {
                let (reason, _prepared) = error.into_parts();
                let request = job
                    .take_request()
                    .expect("rejected local job preserves its request");
                Err(super::LocalSubmitError::new(
                    request,
                    match reason {
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
                    },
                ))
            }
        }
    }
}
