// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::io::Seek;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::Mutex;

use qubit_io::Input;
use qubit_operation::DiagnosticSummary;
use qubit_operation::EffectFact;
use qubit_operation::EffectRecord;
use qubit_operation::OperationManager;
use qubit_operation::OperationOptions;
use qubit_operation::OperationOutcome;
use qubit_operation::OperationScope;
use qubit_operation::StopReport;

use super::super::LocalFactObserver;
use super::super::LocalOperation;
use super::super::LocalOperationOptions;
use super::super::LocalReadRequest;
use super::super::LocalSubmitError;
use super::super::LocalSubmitRejection;
use super::super::complete::LocalCopyOperationResult;
use super::super::complete::LocalCopyRequest;
use super::super::complete::LocalCreateDirectoryOperationResult;
use super::super::complete::LocalCreateDirectoryRequest;
use super::super::complete::LocalDeleteOperationResult;
use super::super::complete::LocalDeleteRequest;
use super::super::complete::LocalReadAtRequest;
use super::super::complete::LocalReadPrefix;
use super::super::complete::LocalReadPrefixRequest;
use super::super::complete::LocalRenameOperationResult;
use super::super::complete::LocalRenameRequest;
use super::super::internal::permits::Permit;
use super::super::internal::prepared_local_job::PreparedLocalJob;
#[path = "complete_read.rs"]
mod complete_read;
use complete_read::CompleteRead;

use super::AsyncLocalFileSystem;
use super::OwnedSubmission;
use crate::LocalDurabilityState;
use crate::LocalFactSnapshot;
use crate::LocalFileError;
use crate::LocalFileErrorKind;
use crate::LocalFileMetadata;
use crate::LocalPublicationState;
use crate::error::LocalFileEffectState;
use crate::error::LocalFileOperation;
use crate::error::LocalReadLimitError;
use crate::error::LocalResourceKind;
use crate::error::LocalResourceLimitError;
use crate::options::LocalReadAllOptions;
use crate::options::LocalReadOptions;
use crate::outcome::LocalCopyFailure;
use crate::outcome::LocalCopyFailureState;
use crate::outcome::LocalCopyStats;
use crate::outcome::LocalRenameFailure;
use crate::outcome::LocalRenameFailureState;

#[cfg(test)]
type ReadAllTestGate = (usize, PathBuf, std::sync::mpsc::Sender<()>, std::sync::mpsc::Receiver<()>);
#[cfg(test)]
static READ_ALL_TEST_GATE: Mutex<Option<ReadAllTestGate>> = Mutex::new(None);
#[cfg(test)]
static READ_ALL_TEST_GATE_LOCK: Mutex<()> = Mutex::new(());

#[cfg(test)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DiscardBufferTestStage {
    BeforeDrop,
    AfterDrop,
}

#[cfg(test)]
type DiscardBufferTestGate = (
    usize,
    PathBuf,
    std::sync::mpsc::Sender<DiscardBufferTestStage>,
    Arc<Mutex<std::sync::mpsc::Receiver<()>>>,
);
#[cfg(test)]
static DISCARD_BUFFER_TEST_GATE: Mutex<Option<DiscardBufferTestGate>> = Mutex::new(None);

#[cfg(test)]
struct DiscardBufferTestGateGuard(usize, PathBuf);

#[cfg(test)]
impl DiscardBufferTestGateGuard {
    fn install(
        authority: usize,
        path: PathBuf,
        entered: std::sync::mpsc::Sender<DiscardBufferTestStage>,
        release: std::sync::mpsc::Receiver<()>,
    ) -> Self {
        *DISCARD_BUFFER_TEST_GATE
            .lock()
            .unwrap_or_else(|error| error.into_inner()) = Some((authority, path.clone(), entered, Arc::new(Mutex::new(release))));
        Self(authority, path)
    }
}

#[cfg(test)]
impl Drop for DiscardBufferTestGateGuard {
    fn drop(&mut self) {
        let mut gate = DISCARD_BUFFER_TEST_GATE
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        if gate.as_ref().is_some_and(|(authority, path, _, _)| *authority == self.0 && path == &self.1) {
            gate.take();
        }
    }
}

#[cfg(test)]
fn pause_discard_buffer_test_gate(authority: usize, path: &Path, stage: DiscardBufferTestStage) {
    let gate = DISCARD_BUFFER_TEST_GATE
        .lock()
        .unwrap_or_else(|error| error.into_inner())
        .as_ref()
        .filter(|(target_authority, target, _, _)| *target_authority == authority && target == path)
        .map(|(_, _, entered, release)| (entered.clone(), Arc::clone(release)));
    if let Some((entered, release)) = gate {
        entered.send(stage).expect("buffer discard observer remains connected");
        release
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .recv_timeout(std::time::Duration::from_secs(5))
            .expect("buffer discard test resumes worker within five seconds");
    }
}

#[cfg(test)]
struct ReadAllTestGateGuard(usize, PathBuf);

#[cfg(test)]
impl ReadAllTestGateGuard {
    fn install(authority: usize, path: PathBuf, entered: std::sync::mpsc::Sender<()>, release: std::sync::mpsc::Receiver<()>) -> Self {
        *READ_ALL_TEST_GATE.lock().unwrap_or_else(|error| error.into_inner()) = Some((authority, path.clone(), entered, release));
        Self(authority, path)
    }
}

#[cfg(test)]
impl Drop for ReadAllTestGateGuard {
    fn drop(&mut self) {
        let mut gate = READ_ALL_TEST_GATE.lock().unwrap_or_else(|error| error.into_inner());
        if gate.as_ref().is_some_and(|(authority, path, _, _)| *authority == self.0 && path == &self.1) {
            gate.take();
        }
    }
}

#[cfg(test)]
mod read_at_success_tests {
    use std::path::Path;
    use std::path::PathBuf;
    use std::sync::Mutex;

    use super::ReadAllTestGate;

    static READ_AT_SUCCESS_TEST_GATE: Mutex<Option<ReadAllTestGate>> = Mutex::new(None);

    pub(super) struct ReadAtSuccessTestGateGuard(usize, PathBuf);

    impl ReadAtSuccessTestGateGuard {
        pub(super) fn install(
            authority: usize,
            path: PathBuf,
            entered: std::sync::mpsc::Sender<()>,
            release: std::sync::mpsc::Receiver<()>,
        ) -> Self {
            *READ_AT_SUCCESS_TEST_GATE
                .lock()
                .unwrap_or_else(|error| error.into_inner()) = Some((authority, path.clone(), entered, release));
            Self(authority, path)
        }
    }

    impl Drop for ReadAtSuccessTestGateGuard {
        fn drop(&mut self) {
            let mut gate = READ_AT_SUCCESS_TEST_GATE
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            if gate.as_ref().is_some_and(|(authority, path, _, _)| *authority == self.0 && path == &self.1) {
                gate.take();
            }
        }
    }

    pub(super) fn pause_read_at_success_test_gate(authority: usize, path: &Path) {
        let gate = {
            let mut registered = READ_AT_SUCCESS_TEST_GATE
                .lock()
                .unwrap_or_else(|error| error.into_inner());
            if registered.as_ref().is_some_and(|(target_authority, target, _, _)| *target_authority == authority && target == path) {
                registered.take()
            } else {
                None
            }
        };
        if let Some((_, _, entered, release)) = gate {
            let _ = entered.send(());
            let _ = release.recv_timeout(std::time::Duration::from_secs(5));
        }
    }
}

impl AsyncLocalFileSystem {
    /// Submits a finite complete read with per-block cooperative cancellation.
    /// The returned `LocalFactObserver` reports facts separately from the
    /// bytes delivered in the operation outcome. The read uses one opened
    /// reader and compares observations before and after reading to detect
    /// changes; this is a consistency check, not an atomic snapshot guarantee.
    /// The observer retains a bounded fact-record slot until it and its clones
    /// are dropped. Taking the outcome transfers the bytes to the caller and
    /// releases the operation handle's memory-budget reference; it does not
    /// wait for settlement. A stop request remains cooperative, so inspect the
    /// outcome to learn whether the read succeeded, failed, or stopped.
    ///
    /// An explicit content ceiling produces `read_limit_error()` when it is
    /// the effective boundary. Executor and per-operation memory ceilings,
    /// and contention for the shared byte pool, produce
    /// `resource_limit_error()` with resource `OwnedBytes`. If an explicit
    /// content limit and a memory limit have the same value, the explicit
    /// content diagnostic takes precedence. Use this method for cooperative
    /// cancellation between read blocks; use
    /// [`super::AsyncLocalFileSystem::submit_read_all_with_observer`] for the
    /// ordinary full-read behavior without those cancellation checkpoints.
    ///
    /// # Parameters
    /// - `manager` — operation manager that owns the admission scope.
    /// - `operation` — operation options used for admission and cancellation.
    /// - `request` — owned input retained if submission is rejected.
    /// - `limits` — per-operation resource ceilings.
    pub fn submit_read_all(
        &self,
        manager: &OperationManager,
        operation: OperationOptions,
        request: LocalReadRequest,
        limits: LocalOperationOptions,
    ) -> Result<(LocalOperation<Vec<u8>>, LocalFactObserver), LocalSubmitError<LocalReadRequest>> {
        let execution_path = match (&self.host_base, request.path.is_relative()) {
            (Some(base), true) => base.join(&request.path),
            _ => request.path.clone(),
        };
        let filesystem = self.filesystem.clone();
        self.submit_owned(
            manager,
            operation,
            request,
            OwnedSubmission {
                path_is_relative: execution_path.is_relative(),

                required_handles: 1,

                limits,

                initial_owned_bytes: 0,
            },
            move |request, scope, _observer, budget, maximum| {
                let read_limit = effective_read_limit(&request.read, maximum);
                match read_complete_cancellable(
                    &filesystem,
                    &execution_path,
                    &request.read,
                    read_limit,
                    scope,
                    &budget,
                    maximum,
                ) {
                    OperationOutcome::Failed(error) => {
                        shrink_to_zero(&budget);
                        OperationOutcome::Failed(error)
                    }
                    OperationOutcome::Stopped(stop) => {
                        shrink_to_zero(&budget);
                        OperationOutcome::Stopped(stop)
                    }
                    OperationOutcome::Succeeded(read) => OperationOutcome::Succeeded(read.bytes),
                    OperationOutcome::ChildFailed(summary) => OperationOutcome::ChildFailed(summary),
                    OperationOutcome::Panicked(summary) => OperationOutcome::Panicked(summary),
                    OperationOutcome::ExecutionFailed(failure) => OperationOutcome::ExecutionFailed(failure),
                }
            },
        )
    }
    /// Submits a complete UTF-8 read using the same allocation accounting as
    /// [`Self::submit_read_all`].
    ///
    /// The operation returns a `LocalFactObserver` alongside its handle. The
    /// text is delivered by the operation outcome; the observer reports
    /// operation facts and does not contain or replace the result. Use this
    /// method when the result must be validated as UTF-8; use
    /// [`Self::submit_read_all`] when the caller needs bytes. Both methods
    /// check for a stop between read blocks and observe the same opened reader
    /// before and after reading to detect changes. These observations do not
    /// provide an atomic snapshot. This method validates UTF-8 after the read
    /// completes, while `submit_read_all` returns the original bytes. Both
    /// methods check the explicit
    /// content limit before an `OwnedBytes` memory limit when both boundaries
    /// are reached at the same value. Executor and per-operation memory limits
    /// further constrain the shared byte pool, whose capacity may be in use by
    /// concurrent operations.
    /// An explicit read ceiling produces `read_limit_error()` when it is the
    /// effective boundary. Executor, per-operation, and shared byte-pool
    /// limits produce `resource_limit_error()` with resource `OwnedBytes`;
    /// callers checking the latter should use `resource_limit_error()` rather
    /// than `read_limit_error()`.
    ///
    /// # Parameters
    /// - `manager` — operation manager that owns the admission scope.
    /// - `operation` — operation options used for admission and cancellation.
    /// - `request` — owned input retained if submission is rejected.
    /// - `limits` — per-operation resource ceilings.
    pub fn submit_read_text(
        &self,
        manager: &OperationManager,
        operation: OperationOptions,
        request: LocalReadRequest,
        limits: LocalOperationOptions,
    ) -> Result<(LocalOperation<String>, LocalFactObserver), LocalSubmitError<LocalReadRequest>> {
        let execution_path = match (&self.host_base, request.path.is_relative()) {
            (Some(base), true) => base.join(&request.path),
            _ => request.path.clone(),
        };
        let filesystem = self.filesystem.clone();
        self.submit_owned(
            manager,
            operation,
            request,
            OwnedSubmission {
                path_is_relative: execution_path.is_relative(),

                required_handles: 1,

                limits,

                initial_owned_bytes: 0,
            },
            move |request, scope, _observer, budget, maximum| {
                let read_limit = effective_read_limit(&request.read, maximum);
                match read_complete_cancellable(
                    &filesystem,
                    &execution_path,
                    &request.read,
                    read_limit,
                    scope,
                    &budget,
                    maximum,
                ) {
                    OperationOutcome::Succeeded(read) => {
                        match decode_complete_text(read.bytes, &read.path, read.current_directory.as_deref()) {
                            Ok(text) => OperationOutcome::Succeeded(text),
                            Err(error) => OperationOutcome::Failed(error),
                        }
                    }
                    OperationOutcome::Failed(error) => {
                        shrink_to_zero(&budget);
                        OperationOutcome::Failed(error)
                    }
                    OperationOutcome::Stopped(stop) => {
                        shrink_to_zero(&budget);
                        OperationOutcome::Stopped(stop)
                    }
                    OperationOutcome::ChildFailed(summary) => OperationOutcome::ChildFailed(summary),
                    OperationOutcome::Panicked(summary) => OperationOutcome::Panicked(summary),
                    OperationOutcome::ExecutionFailed(failure) => OperationOutcome::ExecutionFailed(failure),
                }
            },
        )
    }
    /// Submits a bounded prefix read and reports whether EOF was observed.
    ///
    /// The result contains at most `request.max_bytes` bytes. The worker may
    /// probe one additional byte on the stack to distinguish a complete prefix
    /// from a truncated one; that probe is not retained or charged to the byte
    /// pool. `complete` is true only when EOF was observed within the requested
    /// prefix (including an empty file with a zero-byte limit).
    /// The operation's `max_owned_bytes` limit must be nonzero and no greater
    /// than the executor limit; the read is also subject to the shared executor
    /// byte pool. Other per-operation limits further restrict, rather than
    /// reserve, shared capacity.
    ///
    /// # Parameters
    /// - `manager` — operation manager that owns the admission scope.
    /// - `operation` — operation options used for admission and cancellation.
    /// - `request` — owned path, reader options, and prefix byte limit;
    ///   returned intact if submission is rejected.
    /// - `limits` — per-operation resource ceilings; `None` inherits executor
    ///   limits and `Some` can only tighten them.
    pub fn submit_read_prefix(
        &self,
        manager: &OperationManager,
        operation: OperationOptions,
        request: LocalReadPrefixRequest,
        limits: LocalOperationOptions,
    ) -> Result<(LocalOperation<LocalReadPrefix>, LocalFactObserver), LocalSubmitError<LocalReadPrefixRequest>> {
        let execution_path = match (&self.host_base, request.path.is_relative()) {
            (Some(base), true) => base.join(&request.path),
            _ => request.path.clone(),
        };
        let filesystem = self.filesystem.clone();
        let authority_identity = Arc::as_ptr(&filesystem.core) as usize;
        self.submit_owned(
            manager,
            operation,
            request,
            OwnedSubmission {
                path_is_relative: execution_path.is_relative(),

                required_handles: 1,

                limits,

                initial_owned_bytes: 0,
            },
            move |request, _scope, _observer, budget, maximum| {
                let reader_result = match before_native_call(_scope, || {
                    filesystem.open_reader_with_context(&execution_path, &request.read, |reader, path, pwd| {
                        (reader, path.to_path_buf(), pwd.map(Path::to_path_buf))
                    })
                }) {
                    Ok(result) => result,
                    Err(stop) => return OperationOutcome::Stopped(stop),
                };
                let (mut reader, diagnostic_path, current_directory) = match reader_result {
                    Ok(binding) => binding,
                    Err(error) => return OperationOutcome::Failed(error),
                };
                if let Some(stopped) = stop_outcome(_scope) {
                    return stopped;
                }
                let mut bytes = Vec::new();
                let mut buffer = [0_u8; 8192];
                let mut complete = true;
                loop {
                    if let Some(stopped) = stop_outcome(_scope) {
                        discard_buffer_and_refund(bytes, &budget, &diagnostic_path, authority_identity);
                        return stopped;
                    }
                    if bytes.len() == request.max_bytes {
                        let mut sentinel = [0_u8; 1];
                        match reader.read(&mut sentinel) {
                            Ok(0) => break,
                            Ok(_) => {
                                complete = false;
                                break;
                            }
                            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                            Err(error) => {
                                discard_buffer_and_refund(bytes, &budget, &diagnostic_path, authority_identity);
                                return OperationOutcome::Failed(LocalFileError::from_io(
                                    LocalFileOperation::Read,
                                    Some(diagnostic_path.clone()),
                                    current_directory.clone(),
                                    error,
                                ));
                            }
                        }
                    }
                    let count = (request.max_bytes - bytes.len()).min(buffer.len());
                    match reader.read(&mut buffer[..count]) {
                        Ok(0) => break,
                        Ok(read) => {
                            if let Err(error) = reserve_capacity(&mut bytes, read, &budget, maximum, &diagnostic_path) {
                                discard_buffer_and_refund(bytes, &budget, &diagnostic_path, authority_identity);
                                return OperationOutcome::Failed(attach_current_directory(
                                    error,
                                    current_directory.as_deref(),
                                ));
                            }
                            bytes.extend_from_slice(&buffer[..read]);
                        }
                        Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                        Err(error) => {
                            discard_buffer_and_refund(bytes, &budget, &diagnostic_path, authority_identity);
                            return OperationOutcome::Failed(LocalFileError::from_io(
                                LocalFileOperation::Read,
                                Some(diagnostic_path.clone()),
                                current_directory.clone(),
                                error,
                            ));
                        }
                    }
                }
                finish_prefix_read(_scope, bytes, complete, &budget, &diagnostic_path, authority_identity)
            },
        )
    }
    /// Reads at most the requested number of bytes from a position in one
    /// opened reader.
    ///
    /// The worker opens the path once under the captured filesystem authority,
    /// applies `request.offset` to that reader, and reads no more than the
    /// smaller of `request.max_bytes` and the operation's owned-byte limit.
    /// EOF may produce fewer bytes. A zero-byte request succeeds with an empty
    /// result if opening and seeking succeed. The operation limit must be
    /// nonzero and no greater than the executor limit; accepted reads also
    /// compete for the shared executor byte pool.
    ///
    /// # Parameters
    /// - `manager` — operation manager that owns the admission scope.
    /// - `operation` — operation options used for admission and cancellation.
    /// - `request` — owned path, reader options, position, and byte limit;
    ///   returned intact if submission is rejected.
    /// - `limits` — per-operation resource ceilings; `None` inherits executor
    ///   limits and `Some` can only tighten them.
    pub fn submit_read_at(
        &self,
        manager: &OperationManager,
        operation: OperationOptions,
        request: LocalReadAtRequest,
        limits: LocalOperationOptions,
    ) -> Result<(LocalOperation<Vec<u8>>, LocalFactObserver), LocalSubmitError<LocalReadAtRequest>> {
        let execution_path = match (&self.host_base, request.path.is_relative()) {
            (Some(base), true) => base.join(&request.path),
            _ => request.path.clone(),
        };
        let filesystem = self.filesystem.clone();
        let authority_identity = Arc::as_ptr(&filesystem.core) as usize;
        self.submit_owned(
            manager,
            operation,
            request,
            OwnedSubmission {
                path_is_relative: execution_path.is_relative(),

                required_handles: 1,

                limits,

                initial_owned_bytes: 0,
            },
            move |request, _scope, _observer, budget, maximum| {
                let reader_result = match before_native_call(_scope, || {
                    filesystem.open_reader_with_context(&execution_path, &request.read, |reader, path, pwd| {
                        (reader, path.to_path_buf(), pwd.map(Path::to_path_buf))
                    })
                }) {
                    Ok(result) => result,
                    Err(stop) => return OperationOutcome::Stopped(stop),
                };
                let (mut reader, diagnostic_path, current_directory) = match reader_result {
                    Ok(binding) => binding,
                    Err(error) => return OperationOutcome::Failed(error),
                };
                if let Some(stopped) = stop_outcome(_scope) {
                    return stopped;
                }
                if let Err(error) = reader.seek(request.offset) {
                    return OperationOutcome::Failed(LocalFileError::from_io(
                        LocalFileOperation::Read,
                        Some(diagnostic_path.clone()),
                        current_directory.clone(),
                        error,
                    ));
                }
                let target = request.max_bytes.min(maximum);
                let mut bytes = Vec::new();
                let mut buffer = [0_u8; 8192];
                while bytes.len() < target {
                    if let Some(stopped) = stop_outcome(_scope) {
                        discard_buffer_and_refund(bytes, &budget, &diagnostic_path, authority_identity);
                        return stopped;
                    }
                    let count = (target - bytes.len()).min(buffer.len());
                    match reader.read(&mut buffer[..count]) {
                        Ok(0) => break,
                        Ok(read) => {
                            if let Err(error) = reserve_capacity(&mut bytes, read, &budget, maximum, &diagnostic_path) {
                                discard_buffer_and_refund(bytes, &budget, &diagnostic_path, authority_identity);
                                return OperationOutcome::Failed(attach_current_directory(
                                    error,
                                    current_directory.as_deref(),
                                ));
                            }
                            bytes.extend_from_slice(&buffer[..read]);
                            if let Some(stopped) = stop_outcome(_scope) {
                                discard_buffer_and_refund(bytes, &budget, &diagnostic_path, authority_identity);
                                return stopped;
                            }
                        }
                        Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                        Err(error) => {
                            discard_buffer_and_refund(bytes, &budget, &diagnostic_path, authority_identity);
                            return OperationOutcome::Failed(LocalFileError::from_io(
                                LocalFileOperation::Read,
                                Some(diagnostic_path.clone()),
                                current_directory.clone(),
                                error,
                            ));
                        }
                    }
                }
                #[cfg(test)]
                read_at_success_tests::pause_read_at_success_test_gate(
                    Arc::as_ptr(&filesystem.core) as usize,
                    &diagnostic_path,
                );
                if let Some(stopped) = stop_outcome(_scope) {
                    discard_buffer_and_refund(bytes, &budget, &diagnostic_path, authority_identity);
                    return stopped;
                }
                OperationOutcome::Succeeded(bytes)
            },
        )
    }
    /// Submits final-entry metadata observation.
    ///
    /// # Parameters
    /// - `manager` — operation manager that owns the admission scope.
    /// - `operation` — operation options used for admission and cancellation.
    /// - `path` — path resolved under the captured filesystem authority.
    /// - `limits` — per-operation resource ceilings.
    pub fn submit_metadata(
        &self,
        manager: &OperationManager,
        operation: OperationOptions,
        path: PathBuf,
        limits: LocalOperationOptions,
    ) -> Result<(LocalOperation<LocalFileMetadata>, LocalFactObserver), LocalSubmitError<PathBuf>> {
        let execution_path = match (&self.host_base, path.is_relative()) {
            (Some(base), true) => base.join(&path),
            _ => path.clone(),
        };
        let filesystem = self.filesystem.clone();
        self.submit_owned(
            manager,
            operation,
            path,
            OwnedSubmission {
                path_is_relative: execution_path.is_relative(),
                required_handles: 1,
                limits,
                initial_owned_bytes: 0,
            },
            move |_request, _scope, _observer, _budget, _maximum| match before_native_call(_scope, || {
                filesystem.metadata(&execution_path)
            }) {
                Ok(Ok(metadata)) => OperationOutcome::Succeeded(metadata),
                Ok(Err(error)) => OperationOutcome::Failed(error),
                Err(stop) => OperationOutcome::Stopped(stop),
            },
        )
    }
    /// Submits a complete copy while retaining its structured partial failure.
    ///
    /// Admission reserves `2 + max_open_directories.unwrap_or(1)` native
    /// handles in one request, using saturating addition. The default is three
    /// handles, including for an ordinary file copy. A per-operation handle
    /// limit below that value or an executor whose shared handle pool currently
    /// has fewer available slots rejects submission with
    /// [`LocalSubmitRejection::CapacityExceeded`]. The rejected request remains
    /// available from the returned error. Increase the configured limit when
    /// it is too small; when other operations hold the shared slots, wait for
    /// them to release those handles and retry.
    ///
    /// # Parameters
    /// - `manager` — operation manager that owns the admission scope.
    /// - `operation` — operation options used for admission and cancellation.
    /// - `request` — owned input retained if submission is rejected.
    /// - `limits` — per-operation resource ceilings.
    pub fn submit_copy(
        &self,
        manager: &OperationManager,
        operation: OperationOptions,
        request: LocalCopyRequest,
        limits: LocalOperationOptions,
    ) -> Result<
        (
            LocalOperation<LocalCopyOperationResult, LocalCopyFailure>,
            LocalFactObserver,
        ),
        LocalSubmitError<LocalCopyRequest>,
    > {
        let source = bind_host_path(&self.host_base, &request.source);
        let destination = bind_host_path(&self.host_base, &request.destination);
        let required_handles = 2_usize.saturating_add(request.options.max_open_directories().unwrap_or(1));
        let filesystem = self.filesystem.clone();
        self.submit_owned(
            manager,
            operation,
            request,
            OwnedSubmission {
                path_is_relative: source.is_relative() || destination.is_relative(),

                required_handles,

                limits,

                initial_owned_bytes: 0,
            },
            move |request, scope, observer, _budget, _maximum| {
                if let Some(stopped) = stop_outcome(scope) {
                    return stopped;
                }
                let options = if request.options.max_open_directories().is_none() {
                    request.options.with_max_open_directories(1)
                } else {
                    request.options
                };
                match filesystem.copy_with_options(&source, &destination, &options) {
                    Ok(outcome) => {
                        let stats = outcome.stats();
                        let published = stats.files() > 0 || stats.directories() > 0 || stats.bytes() > 0;
                        let metadata_only = !published && stats.directory_permissions_applied() > 0;
                        record_local_effect(
                            scope,
                            &observer,
                            copied_modified_count(stats),
                            stats.bytes(),
                            if published {
                                LocalPublicationState::Published
                            } else {
                                LocalPublicationState::NotPublished
                            },
                            if outcome.durable() {
                                LocalDurabilityState::Durable
                            } else {
                                LocalDurabilityState::Unknown
                            },
                            EffectFact::Known,
                            if metadata_only {
                                "local copy applied destination directory metadata"
                            } else if published {
                                "local copy published destination"
                            } else {
                                "local copy skipped without publication"
                            },
                        );
                        OperationOutcome::Succeeded(LocalCopyOperationResult {
                            source: request.source,
                            destination: request.destination,
                            outcome,
                        })
                    }
                    Err(failure) => {
                        let stats = failure.partial_stats();
                        let (publication, fact) = match failure.state() {
                            LocalCopyFailureState::Unchanged => {
                                (LocalPublicationState::NotPublished, EffectFact::Known)
                            }
                            LocalCopyFailureState::PartiallyPublished | LocalCopyFailureState::Published => {
                                (LocalPublicationState::Published, EffectFact::Known)
                            }
                            LocalCopyFailureState::Indeterminate => {
                                (LocalPublicationState::Unknown, EffectFact::Unknown)
                            }
                        };
                        record_local_effect(
                            scope,
                            &observer,
                            copied_modified_count(*stats),
                            stats.bytes(),
                            publication,
                            LocalDurabilityState::Unknown,
                            fact,
                            "local copy failed with retained partial statistics",
                        );
                        OperationOutcome::Failed(failure)
                    }
                }
            },
        )
    }
    /// Submits a native rename and retains the achieved publication state.
    ///
    /// # Parameters
    /// - `manager` — operation manager that owns the admission scope.
    /// - `operation` — operation options used for admission and cancellation.
    /// - `request` — owned input retained if submission is rejected.
    /// - `limits` — per-operation resource ceilings.
    pub fn submit_rename(
        &self,
        manager: &OperationManager,
        operation: OperationOptions,
        request: LocalRenameRequest,
        limits: LocalOperationOptions,
    ) -> Result<
        (
            LocalOperation<LocalRenameOperationResult, LocalRenameFailure>,
            LocalFactObserver,
        ),
        LocalSubmitError<LocalRenameRequest>,
    > {
        let source = bind_host_path(&self.host_base, &request.source);
        let destination = bind_host_path(&self.host_base, &request.destination);
        let filesystem = self.filesystem.clone();
        self.submit_owned(
            manager,
            operation,
            request,
            OwnedSubmission {
                path_is_relative: source.is_relative() || destination.is_relative(),

                required_handles: 2,

                limits,

                initial_owned_bytes: 0,
            },
            move |request, scope, observer, _budget, _maximum| {
                if let Some(stopped) = stop_outcome(scope) {
                    return stopped;
                }
                match filesystem.rename_with_options(&source, &destination, &request.options) {
                    Ok(outcome) => {
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
                            EffectFact::Known,
                            "local rename published destination",
                        );
                        OperationOutcome::Succeeded(LocalRenameOperationResult {
                            source: request.source,
                            destination: request.destination,
                            outcome,
                        })
                    }
                    Err(failure) => {
                        let (publication, fact) = match failure.state() {
                            LocalRenameFailureState::Unchanged => {
                                (LocalPublicationState::NotPublished, EffectFact::Known)
                            }
                            LocalRenameFailureState::Renamed => (LocalPublicationState::Published, EffectFact::Known),
                            LocalRenameFailureState::Indeterminate => {
                                (LocalPublicationState::Unknown, EffectFact::Unknown)
                            }
                        };
                        record_local_effect(
                            scope,
                            &observer,
                            u64::from(failure.state() == LocalRenameFailureState::Renamed),
                            0,
                            publication,
                            LocalDurabilityState::Unknown,
                            fact,
                            "local rename failed with retained namespace state",
                        );
                        OperationOutcome::Failed(failure)
                    }
                }
            },
        )
    }
    /// Submits directory creation and records known or unknown parent effects.
    ///
    /// # Parameters
    /// - `manager` — operation manager that owns the admission scope.
    /// - `operation` — operation options used for admission and cancellation.
    /// - `request` — owned input retained if submission is rejected.
    /// - `limits` — per-operation resource ceilings.
    pub fn submit_create_directory(
        &self,
        manager: &OperationManager,
        operation: OperationOptions,
        request: LocalCreateDirectoryRequest,
        limits: LocalOperationOptions,
    ) -> Result<
        (LocalOperation<LocalCreateDirectoryOperationResult>, LocalFactObserver),
        LocalSubmitError<LocalCreateDirectoryRequest>,
    > {
        let path = bind_host_path(&self.host_base, &request.path);
        let filesystem = self.filesystem.clone();
        self.submit_owned(
            manager,
            operation,
            request,
            OwnedSubmission {
                path_is_relative: path.is_relative(),

                required_handles: 1,

                limits,

                initial_owned_bytes: 0,
            },
            move |request, scope, observer, _budget, _maximum| {
                if let Some(stopped) = stop_outcome(scope) {
                    return stopped;
                }
                match filesystem.create_directory_with_options(&path, &request.options) {
                    Ok(outcome) => {
                        let created = outcome.created();
                        record_local_effect(
                            scope,
                            &observer,
                            u64::from(created),
                            0,
                            if created {
                                LocalPublicationState::Published
                            } else {
                                LocalPublicationState::NotPublished
                            },
                            LocalDurabilityState::Unknown,
                            EffectFact::Known,
                            "local directory creation completed",
                        );
                        OperationOutcome::Succeeded(LocalCreateDirectoryOperationResult {
                            path: request.path,
                            outcome,
                        })
                    }
                    Err(error) => {
                        record_local_effect(
                            scope,
                            &observer,
                            0,
                            0,
                            LocalPublicationState::Unknown,
                            LocalDurabilityState::Unknown,
                            EffectFact::Unknown,
                            "local directory creation failed; partial parent state is unknown",
                        );
                        OperationOutcome::Failed(error)
                    }
                }
            },
        )
    }
    /// Submits file or directory deletion while preserving its typed I/O error.
    ///
    /// # Parameters
    /// - `manager` — operation manager that owns the admission scope.
    /// - `operation` — operation options used for admission and cancellation.
    /// - `request` — owned input retained if submission is rejected.
    /// - `limits` — per-operation resource ceilings.
    pub fn submit_delete(
        &self,
        manager: &OperationManager,
        operation: OperationOptions,
        request: LocalDeleteRequest,
        limits: LocalOperationOptions,
    ) -> Result<(LocalOperation<LocalDeleteOperationResult>, LocalFactObserver), LocalSubmitError<LocalDeleteRequest>>
    {
        let path = bind_host_path(&self.host_base, &request.path);
        let filesystem = self.filesystem.clone();
        self.submit_owned(
            manager,
            operation,
            request,
            OwnedSubmission {
                path_is_relative: path.is_relative(),

                required_handles: 1,

                limits,

                initial_owned_bytes: 0,
            },
            move |request, scope, observer, _budget, _maximum| {
                if let Some(stopped) = stop_outcome(scope) {
                    return stopped;
                }
                let result = if request.directory {
                    filesystem.delete_directory_with_options(&path, &request.options)
                } else {
                    filesystem.delete_file_with_options(&path, &request.options)
                };
                match result {
                    Ok(outcome) => {
                        let deleted = outcome.deleted();
                        let recursive_directory = request.directory && request.options.recursive();
                        record_local_effect(
                            scope,
                            &observer,
                            u64::from(deleted),
                            0,
                            if deleted {
                                LocalPublicationState::Published
                            } else {
                                LocalPublicationState::NotPublished
                            },
                            LocalDurabilityState::Unknown,
                            if deleted && recursive_directory {
                                EffectFact::Unknown
                            } else {
                                EffectFact::Known
                            },
                            "local entry deletion completed",
                        );
                        OperationOutcome::Succeeded(LocalDeleteOperationResult {
                            path: request.path,
                            directory: request.directory,
                            outcome,
                            deleted_entries: if !deleted {
                                Some(0)
                            } else if recursive_directory {
                                None
                            } else {
                                Some(1)
                            },
                        })
                    }
                    Err(error) => {
                        let (modified_count, publication, fact, message) = deletion_effect_facts(error.effect_state());
                        record_local_effect(
                            scope,
                            &observer,
                            modified_count,
                            0,
                            publication,
                            LocalDurabilityState::Unknown,
                            fact,
                            message,
                        );
                        OperationOutcome::Failed(error)
                    }
                }
            },
        )
    }
    /// Submits an owned request with its resource requirements.
    /// # Type Parameters
    /// - `R` — request retained until the worker task runs.
    /// - `T` — successful result returned by the operation.
    /// - `E` — operation error returned on failure.
    /// - `F` — worker task that processes the request.
    /// # Parameters
    /// - `manager` — manager that owns the operation admission scope.
    /// - `operation` — cancellation and admission options.
    /// - `request` — owned input passed to the worker task.
    /// - `requirements` — path, handle, byte, and per-operation limit
    ///   requirements.
    /// - `task` — closure that processes the request after admission.
    pub(super) fn submit_owned<R, T, E, F>(
        &self,
        manager: &OperationManager,
        operation: OperationOptions,
        request: R,
        requirements: OwnedSubmission,
        task: F,
    ) -> Result<(LocalOperation<T, E>, LocalFactObserver), LocalSubmitError<R>>
    where
        R: Send + 'static,
        T: Send + 'static,
        E: Send + 'static,
        F: FnOnce(R, &OperationScope, LocalFactObserver, Arc<Mutex<Permit>>, usize) -> OperationOutcome<T, E>
            + Send
            + 'static,
    {
        self.submit_owned_with_bytes(
            manager,
            operation,
            request,
            OwnedSubmission {
                initial_owned_bytes: 0,
                ..requirements
            },
            task,
        )
    }
    /// Submits an operation while reserving its already-owned input allocation
    /// before manager admission. Rejection returns the original request.
    ///
    ///
    /// # Parameters
    /// - `manager` — operation manager that owns the admission scope.
    /// - `operation` — operation options used for admission and cancellation.
    /// - `request` — owned input retained if submission is rejected.
    /// - `requirements` — path, handle, byte, and per-operation limit
    ///   requirements.
    /// - `task` — owned worker closure run after admission.
    /// # Type Parameters
    /// - `R` — type carried through the operation contract.
    /// - `T` — type carried through the operation contract.
    /// - `E` — type carried through the operation contract.
    /// - `F` — type carried through the operation contract.
    pub(super) fn submit_owned_with_bytes<R, T, E, F>(
        &self,
        manager: &OperationManager,
        operation: OperationOptions,
        request: R,
        requirements: OwnedSubmission,
        task: F,
    ) -> Result<(LocalOperation<T, E>, LocalFactObserver), LocalSubmitError<R>>
    where
        R: Send + 'static,
        T: Send + 'static,
        E: Send + 'static,
        F: FnOnce(R, &OperationScope, LocalFactObserver, Arc<Mutex<Permit>>, usize) -> OperationOutcome<T, E>
            + Send
            + 'static,
    {
        let OwnedSubmission {
            path_is_relative,
            required_handles,
            limits,
            initial_owned_bytes,
        } = requirements;
        let scoped = match manager.with_group(&self.executor.inner.group) {
            Ok(scoped) => scoped,
            Err(error) => {
                let rejection = match error {
                    qubit_operation::GroupBindingError::WrongManager => LocalSubmitRejection::WrongManager,
                    qubit_operation::GroupBindingError::ScopeEnded => LocalSubmitRejection::ScopeEnded,
                    qubit_operation::GroupBindingError::Closed => LocalSubmitRejection::Closed,
                    qubit_operation::GroupBindingError::CapacityExceeded => LocalSubmitRejection::CapacityExceeded,
                };
                return Err(LocalSubmitError::new(request, rejection));
            }
        };
        let maximum = limits
            .max_owned_bytes
            .unwrap_or(self.executor.inner.config.max_owned_bytes);
        if maximum == 0 || maximum > self.executor.inner.config.max_owned_bytes || initial_owned_bytes > maximum {
            return Err(LocalSubmitError::new(request, LocalSubmitRejection::CapacityExceeded));
        }
        if self.filesystem.scope() == crate::LocalFileSystemScope::Host && self.host_base.is_none() && path_is_relative
        {
            return Err(LocalSubmitError::new(
                request,
                LocalSubmitRejection::AuthorityNotCaptured,
            ));
        }
        let handle_limit = limits
            .max_open_handles
            .unwrap_or(self.executor.inner.config.max_open_handles);
        if handle_limit > self.executor.inner.config.max_open_handles || required_handles > handle_limit {
            return Err(LocalSubmitError::new(request, LocalSubmitRejection::CapacityExceeded));
        }
        let Some(slot) = self.executor.reserve_slot() else {
            return Err(LocalSubmitError::new(request, LocalSubmitRejection::CapacityExceeded));
        };
        let Some(byte_permit) = self.executor.reserve_bytes(initial_owned_bytes) else {
            return Err(LocalSubmitError::new(request, LocalSubmitRejection::CapacityExceeded));
        };
        let byte_permit = Arc::new(Mutex::new(byte_permit));
        let Some(handle_permit) = self.executor.inner.handle_slots.try_acquire(required_handles) else {
            return Err(LocalSubmitError::new(request, LocalSubmitRejection::CapacityExceeded));
        };
        let Some(fact_permit) = self.executor.reserve_fact() else {
            return Err(LocalSubmitError::new(request, LocalSubmitRejection::CapacityExceeded));
        };
        let fact_permit: Arc<dyn Send + Sync> = Arc::new(fact_permit);
        let observer = LocalFactObserver::with_permit(Arc::clone(&fact_permit));
        let byte_permit_for_job = Arc::clone(&byte_permit);
        let observer_for_job = observer.clone();
        let observer_for_task = observer.clone();
        let job = PreparedLocalJob::new_scoped(
            Arc::clone(&self.executor.inner),
            request,
            slot,
            Arc::clone(&byte_permit),
            handle_permit,
            observer_for_job,
            move |request, scope| task(request, scope, observer_for_task, byte_permit_for_job, maximum),
        );
        match scoped.try_submit_external(operation, Arc::clone(&job)) {
            Ok(handle) => Ok((LocalOperation::new(handle, byte_permit), observer)),
            Err(error) => {
                let (reason, _prepared) = error.into_parts();
                let request = job.take_request().expect("rejected job preserves its request");
                let rejection = match reason {
                    qubit_operation::AdmissionError::Closed | qubit_operation::AdmissionError::ExecutorUnavailable => {
                        LocalSubmitRejection::Closed
                    }
                    qubit_operation::AdmissionError::CapacityExceeded | qubit_operation::AdmissionError::Busy => {
                        LocalSubmitRejection::CapacityExceeded
                    }
                    qubit_operation::AdmissionError::ScopeEnded => LocalSubmitRejection::ScopeEnded,
                    qubit_operation::AdmissionError::Stopped => LocalSubmitRejection::Stopped,
                    _ => LocalSubmitRejection::Closed,
                };
                Err(LocalSubmitError::new(request, rejection))
            }
        }
    }
}
/// Joins a relative path to the captured Host base when one is available.
/// # Parameters
/// - `base` — optional captured Host base path.
/// - `path` — path to bind to that base when it is relative.
fn bind_host_path(base: &Option<PathBuf>, path: &PathBuf) -> PathBuf {
    match (base, path.is_relative()) {
        (Some(base), true) => base.join(path),
        _ => path.clone(),
    }
}
/// Reads a complete file through one observed reader with cooperative stop
/// checks between blocks.
///
/// It validates the pre-read length against the effective content ceiling,
/// reserves every retained buffer growth against both the per-operation limit
/// and shared byte permit, then compares before/after observations to detect
/// changes during the read. These observations detect changes but do not make
/// the read an atomic snapshot. A failure or stop returns partial-buffer
/// capacity to the operation settlement path; a successful buffer retains its
/// permit until the result is delivered. `path` is also the diagnostic path
/// attached to read and limit errors.
///
/// # Returns
/// The complete bytes, or a typed failed/stopped outcome. Explicit content
/// limits remain read-limit errors; memory-budget failures are `OwnedBytes`
/// resource-limit errors.
///
/// # Parameters
/// - `filesystem` — authority used to open the file.
/// - `path` — original path bound once for opening and diagnostics.
/// - `options` — reader options, including symlink and content-limit policy.
/// - `read_limit` — effective byte boundary and whether content options set it.
/// - `scope` — operation scope checked at stop checkpoints.
/// - `budget` — shared permit owning retained output capacity.
/// - `allocation_limit` — this operation's maximum owned-byte capacity.
fn read_complete_cancellable(
    filesystem: &crate::LocalFileSystem,
    path: &Path,
    options: &LocalReadAllOptions,
    read_limit: (u64, bool),
    scope: &OperationScope,
    budget: &Arc<Mutex<Permit>>,
    allocation_limit: usize,
) -> OperationOutcome<CompleteRead, LocalFileError> {
    use crate::observation::LocalObservationRequirement;
    use crate::observation::reader::LocalObservedReader;

    let (read_limit, explicit_read_limit) = read_limit;

    if let Some(stopped) = stop_outcome(scope) {
        return stopped;
    }
    let read_options = effective_read_options(filesystem, options);
    let reader_result = match before_native_call(scope, || {
        filesystem.open_reader_with_context(path, &read_options, |reader, path, current_directory| {
            (reader, path.to_path_buf(), current_directory.map(Path::to_path_buf))
        })
    }) {
        Ok(result) => result,
        Err(stop) => return OperationOutcome::Stopped(stop),
    };
    let (reader, path, current_directory) = match reader_result {
        Ok(binding) => binding,
        Err(error) => return OperationOutcome::Failed(error),
    };
    let mut observed = match LocalObservedReader::from_reader(reader, LocalObservationRequirement::Required) {
        Ok(reader) => reader.bind_error_context(path.clone(), current_directory.clone()),
        Err(error) => {
            return OperationOutcome::Failed(attach_current_directory(
                error.with_path(path.clone()),
                current_directory.as_deref(),
            ));
        }
    };
    let before = observed.observation().clone();
    let observed_len = before.metadata().len();
    if observed_len > read_limit {
        return OperationOutcome::Failed(read_limit_error(
            explicit_read_limit,
            read_limit,
            observed_len,
            0,
            allocation_limit,
            &path,
            current_directory.as_deref(),
        ));
    }

    let mut bytes = Vec::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        if let Some(stopped) = stop_outcome(scope) {
            return stopped;
        }
        if bytes.len() as u64 == read_limit {
            let mut sentinel = [0_u8; 1];
            match observed.read(&mut sentinel) {
                Ok(0) => break,
                Ok(_) => {
                    return OperationOutcome::Failed(read_limit_error(
                        explicit_read_limit,
                        read_limit,
                        read_limit.saturating_add(1),
                        bytes.capacity(),
                        allocation_limit,
                        &path,
                        current_directory.as_deref(),
                    ));
                }
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(error) => {
                    return OperationOutcome::Failed(LocalFileError::from_io(
                        LocalFileOperation::Read,
                        Some(path.clone()),
                        current_directory.clone(),
                        error,
                    ));
                }
            }
        }
        let remaining = (read_limit - bytes.len() as u64).min(buffer.len() as u64) as usize;
        match observed.read(&mut buffer[..remaining]) {
            Ok(0) => break,
            Ok(read) => {
                if let Err(error) = reserve_capacity(&mut bytes, read, budget, allocation_limit, &path) {
                    return OperationOutcome::Failed(attach_current_directory(error, current_directory.as_deref()));
                }
                bytes.extend_from_slice(&buffer[..read]);
                #[cfg(test)]
                pause_read_all_test_gate(Arc::as_ptr(&filesystem.core) as usize, &path);
                if let Some(stopped) = stop_outcome(scope) {
                    return stopped;
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(error) => {
                return OperationOutcome::Failed(LocalFileError::from_io(
                    LocalFileOperation::Read,
                    Some(path.to_path_buf()),
                    current_directory.clone(),
                    error,
                ));
            }
        }
    }
    let after = match observed.observe_now() {
        Ok(observation) => observation,
        Err(error) => return OperationOutcome::Failed(error),
    };
    if !crate::observation::snapshot::observations_match(&before, &after, bytes.len() as u64) {
        return OperationOutcome::Failed(attach_current_directory(
            LocalFileError::new(LocalFileErrorKind::ChangedDuringRead, LocalFileOperation::Read)
                .with_path(path.to_path_buf()),
            current_directory.as_deref(),
        ));
    }
    if let Some(stopped) = stop_outcome(scope) {
        return stopped;
    }
    OperationOutcome::Succeeded(CompleteRead {
        bytes,
        path,
        current_directory,
    })
}

/// Selects the effective read ceiling and whether an explicit content ceiling
/// supplied it. Equal limits preserve the explicit content ceiling as the
/// diagnostic source.
///
/// # Parameters
/// - `options` — read options carrying an optional complete-read ceiling.
/// - `maximum` — executor or operation owned-byte ceiling.
fn effective_read_limit(options: &LocalReadAllOptions, maximum: usize) -> (u64, bool) {
    match options.max_bytes() {
        Some(limit) if limit <= maximum as u64 => (limit, true),
        _ => (maximum as u64, false),
    }
}

/// Creates an error that identifies which complete-read ceiling was exceeded.
///
/// # Parameters
/// - `explicit` — whether the caller's content limit supplied the boundary.
/// - `limit` — effective byte boundary applied to the read.
/// - `observed` — metadata length or proven lower bound beyond the boundary.
/// - `capacity` — already reserved buffer capacity when the boundary was found.
/// - `allocation_limit` — per-operation owned-byte ceiling.
/// - `path` — path associated with the read.
/// - `current_directory` — PWD snapshot used to bind a relative path.
///
/// # Returns
/// A typed read-limit error for explicit ceilings or an `OwnedBytes` resource
/// limit for a memory ceiling.
fn read_limit_error(
    explicit: bool,
    limit: u64,
    observed: u64,
    capacity: usize,
    allocation_limit: usize,
    path: &Path,
    current_directory: Option<&Path>,
) -> LocalFileError {
    if explicit {
        return attach_current_directory(
            LocalFileError::from_read_limit(
                LocalFileOperation::Read,
                Some(path.to_path_buf()),
                LocalReadLimitError::new(limit, observed),
            ),
            current_directory,
        );
    }
    let requested = usize::try_from(observed).unwrap_or(usize::MAX);
    let resource = LocalResourceLimitError::new(
        LocalResourceKind::OwnedBytes,
        allocation_limit,
        allocation_limit.saturating_sub(capacity),
        requested.saturating_sub(capacity),
    );
    let resource = if observed > usize::MAX as u64 {
        resource.with_overflow()
    } else {
        resource.with_observed(requested)
    };
    attach_current_directory(
        LocalFileError::from_resource_limit(LocalFileOperation::Read, Some(path.to_path_buf()), resource),
        current_directory,
    )
}

/// Adds the operation's captured PWD to a diagnostic error.
///
/// # Parameters
/// - `error` — typed error constructed from the bound operation.
/// - `current_directory` — PWD snapshot captured with the path binding.
///
/// # Returns
/// The same error with its current-directory context attached when available.
fn attach_current_directory(error: LocalFileError, current_directory: Option<&Path>) -> LocalFileError {
    match current_directory {
        Some(path) => error.with_current_directory(path.to_path_buf()),
        None => error,
    }
}

/// Decodes complete-read bytes while retaining invalid data and path context.
///
/// # Parameters
/// - `bytes` — owned bytes returned by the complete read.
/// - `path` — path used to attribute an invalid UTF-8 error.
fn decode_complete_text(
    bytes: Vec<u8>,
    path: &Path,
    current_directory: Option<&Path>,
) -> Result<String, LocalFileError> {
    String::from_utf8(bytes).map_err(|source| {
        attach_current_directory(
            LocalFileError::from_utf8(LocalFileOperation::Read, Some(path.to_path_buf()), source),
            current_directory,
        )
    })
}

/// Rechecks cancellation immediately before delivering a prefix result.
///
/// # Parameters
/// - `scope` — operation scope carrying cooperative stop state.
/// - `bytes` — retained output discarded if cancellation was requested.
/// - `complete` — whether the prefix read observed EOF.
/// - `budget` — owned-byte permit for the retained output.
/// - `path` — request path used by the deterministic test gate.
fn finish_prefix_read(
    scope: &OperationScope,
    bytes: Vec<u8>,
    complete: bool,
    budget: &Arc<Mutex<Permit>>,
    path: &Path,
    authority: usize,
) -> OperationOutcome<LocalReadPrefix, LocalFileError> {
    #[cfg(test)]
    pause_read_all_test_gate(authority, path);
    if let Some(stopped) = stop_outcome(scope) {
        discard_buffer_and_refund(bytes, budget, path, authority);
        return stopped;
    }
    OperationOutcome::Succeeded(LocalReadPrefix { bytes, complete })
}

/// Selects and merges the reader options used by a complete read.
///
/// # Parameters
/// - `filesystem` — supplies the instance-level reader defaults.
/// - `options` — supplies complete-read replacements and outer policy.
fn effective_read_options(filesystem: &crate::LocalFileSystem, options: &LocalReadAllOptions) -> LocalReadOptions {
    let mut read_options = options
        .read_options()
        .copied()
        .unwrap_or(*filesystem.default_read_options());
    if let Some(policy) = options.symlink_policy() {
        read_options = read_options.with_symlink_policy(policy);
    }
    read_options
}

#[cfg(test)]
fn pause_read_all_test_gate(authority: usize, path: &Path) {
    let gate = {
        let mut registered = READ_ALL_TEST_GATE.lock().unwrap_or_else(|error| error.into_inner());
        if registered.as_ref().is_some_and(|(target_authority, target, _, _)| *target_authority == authority && target == path) {
            registered.take()
        } else {
            None
        }
    };
    if let Some((_, _, entered, release)) = gate {
        let _ = entered.send(());
        let _ = release.recv_timeout(std::time::Duration::from_secs(5));
    }
}

/// Returns a stopped outcome when the operation scope has a stop request.
/// # Type Parameters
/// - `T` — successful result type.
/// - `E` — operation error type.
/// # Parameters
/// - `scope` — operation scope whose stop state is checked.
// An explicit branch keeps stop paths visible to LLVM function coverage, which
// otherwise reports the executed `Option::map` closure as an uncovered function.
#[allow(clippy::manual_map)]
fn stop_outcome<T, E>(scope: &OperationScope) -> Option<OperationOutcome<T, E>> {
    match scope.context().stop_reason() {
        Some(reason) => Some(OperationOutcome::Stopped(StopReport {
            reason,
            accepted_at: None,
        })),
        None => None,
    }
}
/// Checks for a stop request before invoking a native operation.
/// # Type Parameters
/// - `T` — value returned by the native call.
/// # Parameters
/// - `scope` — operation scope whose stop state is checked.
/// - `native_call` — native operation invoked only when no stop was requested.
fn before_native_call<T>(scope: &OperationScope, native_call: impl FnOnce() -> T) -> Result<T, StopReport> {
    match scope.context().stop_reason() {
        Some(reason) => Err(StopReport {
            reason,
            accepted_at: None,
        }),
        None => Ok(native_call()),
    }
}
/// Records a local effect and updates the observer's bounded fact snapshot.
/// # Parameters
/// - `scope` — scope receiving the effect record.
/// - `observer` — observer whose snapshot is updated.
/// - `modified_count` — number of modified entries to add to the snapshot.
/// - `modified_bytes` — number of modified bytes to add to the snapshot.
/// - `publication` — publication state recorded in the snapshot.
/// - `durability` — durability state recorded in the snapshot.
/// - `fact` — effect classification recorded for the operation.
/// - `description` — bounded diagnostic description of the effect.
#[allow(clippy::too_many_arguments)]
fn record_local_effect(
    scope: &OperationScope,
    observer: &LocalFactObserver,
    modified_count: u64,
    modified_bytes: u64,
    publication: LocalPublicationState,
    durability: LocalDurabilityState,
    fact: EffectFact,
    description: &str,
) {
    let effect_record_error = scope
        .record_effect(EffectRecord {
            operation_id: scope.id(),
            resource_id: None,
            fact,
            description: DiagnosticSummary::new(description),
        })
        .err();
    let mut snapshot: LocalFactSnapshot = observer.snapshot();
    snapshot.modified_count = snapshot.modified_count.saturating_add(modified_count);
    snapshot.modified_bytes = snapshot.modified_bytes.saturating_add(modified_bytes);
    snapshot.modified_range = None;
    snapshot.range_unknown |= modified_count > 0 || fact == EffectFact::Unknown;
    snapshot.publication = publication;
    snapshot.durability = durability;
    snapshot.effect_record_error = effect_record_error;
    observer.update(snapshot);
}

/// Counts copied entries and confirmed permission changes on pre-existing
/// directories.
///
/// Permissions on newly created directories are represented by the copied
/// directory count; this adds only existing-directory changes.
///
/// # Parameters
/// - `stats`: Copy statistics containing completed file, directory, and
///   existing-directory permission changes.
///
/// # Returns
/// The number of modified entries confirmed by the operation.
fn copied_modified_count(stats: LocalCopyStats) -> u64 {
    stats
        .files()
        .saturating_add(stats.directories())
        .saturating_add(stats.directory_permissions_applied())
}
/// Releases all remaining bytes in an executor permit.
/// # Parameters
/// - `permit` — shared byte permit to shrink to zero.
fn shrink_to_zero(permit: &Arc<Mutex<Permit>>) {
    let mut permit = match permit.lock() {
        Ok(permit) => permit,
        Err(poisoned) => poisoned.into_inner(),
    };
    let amount = permit.amount();
    permit.shrink(amount);
}

/// Destroys an unreturned output before releasing its shared byte permit.
///
/// # Type Parameters
/// - `T` — owned value that is discarded before the permit is refunded.
///
/// # Parameters
/// - `buffer` — output whose allocation is no longer retained by the operation.
/// - `permit` — shared byte reservation covering that allocation.
fn discard_buffer_and_refund<T>(buffer: T, permit: &Arc<Mutex<Permit>>, _path: &Path, authority: usize) {
    #[cfg(not(test))]
    let _ = authority;
    #[cfg(test)]
    pause_discard_buffer_test_gate(authority, _path, DiscardBufferTestStage::BeforeDrop);
    drop(buffer);
    #[cfg(test)]
    pause_discard_buffer_test_gate(authority, _path, DiscardBufferTestStage::AfterDrop);
    shrink_to_zero(permit);
}

/// Maps native deletion-effect evidence to the conservative async snapshot.
///
/// # Parameters
/// - `state` — effect evidence reported by the synchronous deletion backend.
fn deletion_effect_facts(
    state: Option<LocalFileEffectState>,
) -> (u64, LocalPublicationState, EffectFact, &'static str) {
    match state {
        Some(LocalFileEffectState::Unchanged) => (
            0,
            LocalPublicationState::NotPublished,
            EffectFact::Known,
            "local deletion failed before changing the namespace",
        ),
        Some(LocalFileEffectState::PartiallyApplied) => (
            1,
            LocalPublicationState::Unknown,
            EffectFact::Known,
            "local deletion partially changed the namespace; exact deletion count is unavailable",
        ),
        Some(LocalFileEffectState::Applied) => (
            1,
            LocalPublicationState::Published,
            EffectFact::Known,
            "local deletion changed the namespace before a later failure",
        ),
        Some(LocalFileEffectState::Indeterminate) | None => (
            0,
            LocalPublicationState::Unknown,
            EffectFact::Unknown,
            "local deletion failed; namespace effects are unknown",
        ),
    }
}

/// Reserves and reconciles capacity before appending bytes to a complete-read
/// buffer.
///
/// The permit is grown before allocation so concurrent operations cannot
/// overcommit shared capacity. Allocation failure returns the reservation;
/// allocator over-allocation is charged to the same permit, and an unaffordable
/// excess is fully refunded before the read fails. The caller discards the
/// output on failure, which releases its allocation.
///
/// # Errors
/// Returns an `OwnedBytes` resource-limit error when the per-operation or
/// shared-pool budget is exceeded, or an I/O error when capacity arithmetic or
/// allocation fails.
///
/// # Parameters
/// - `output` — read buffer whose capacity is to be extended.
/// - `additional` — number of bytes about to be appended.
/// - `permit` — shared byte permit owning retained capacity.
/// - `maximum` — per-operation maximum retained capacity.
/// - `path` — path attached to structured resource and I/O errors.
fn reserve_capacity(
    output: &mut Vec<u8>,
    additional: usize,
    permit: &Arc<Mutex<Permit>>,
    maximum: usize,
    path: &Path,
) -> Result<(), LocalFileError> {
    let old_capacity = output.capacity();
    let requested = match output.len().checked_add(additional) {
        Some(requested) => requested,
        None => {
            return Err(LocalFileError::from_io(
                LocalFileOperation::Read,
                Some(path.to_path_buf()),
                None,
                std::io::Error::other("read capacity overflow"),
            ));
        }
    };
    let target = requested.max(old_capacity);
    let delta = target - old_capacity;
    let mut lease = match permit.lock() {
        Ok(lease) => lease,
        Err(poisoned) => poisoned.into_inner(),
    };
    if target > maximum {
        return Err(LocalFileError::from_resource_limit(
            LocalFileOperation::Read,
            Some(path.to_path_buf()),
            LocalResourceLimitError::new(
                LocalResourceKind::OwnedBytes,
                maximum,
                maximum.saturating_sub(old_capacity),
                delta,
            ),
        ));
    }
    if let Err((limit, remaining, requested)) = lease.try_grow(delta) {
        return Err(LocalFileError::from_resource_limit(
            LocalFileOperation::Read,
            Some(path.to_path_buf()),
            LocalResourceLimitError::new(LocalResourceKind::OwnedBytes, limit, remaining, requested),
        ));
    }
    if let Err(error) = output.try_reserve_exact(additional) {
        lease.shrink(delta);
        return Err(LocalFileError::from_io(
            LocalFileOperation::Read,
            Some(path.to_path_buf()),
            None,
            error.into(),
        ));
    }
    let actual = output.capacity().saturating_sub(old_capacity);
    if actual < delta {
        lease.shrink(delta - actual);
    } else if actual > delta && !lease.grow(actual - delta) {
        return Err(LocalFileError::from_resource_limit(
            LocalFileOperation::Read,
            Some(path.to_path_buf()),
            LocalResourceLimitError::new(
                LocalResourceKind::OwnedBytes,
                maximum,
                maximum.saturating_sub(old_capacity),
                actual - delta,
            ),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod stop_gate_tests {
    use std::path::Path;
    use std::path::PathBuf;
    use std::sync::Arc;
    use std::sync::atomic::AtomicUsize;
    use std::sync::atomic::Ordering;
    use std::sync::mpsc;

    use qubit_operation::EffectFact;
    use qubit_operation::ManagerOptions;
    use qubit_operation::OperationManager;
    use qubit_operation::OperationOutcome;
    use qubit_operation::StopReason;

    use super::DiscardBufferTestGateGuard;
    use super::DiscardBufferTestStage;
    use super::READ_ALL_TEST_GATE;
    use super::READ_ALL_TEST_GATE_LOCK;
    use super::ReadAllTestGateGuard;
    use super::before_native_call;
    use super::decode_complete_text;
    use super::deletion_effect_facts;
    use super::effective_read_limit;
    use super::effective_read_options;
    use super::read_at_success_tests::ReadAtSuccessTestGateGuard;
    use super::read_limit_error;
    use crate::LocalOperation;
    use crate::error::LocalResourceKind;

    fn assert_partial_read_budget_is_reusable<T: Send + 'static>(
        filesystem: &super::AsyncLocalFileSystem,
        manager: &OperationManager,
        failed: &mut LocalOperation<T>,
        probe_path: PathBuf,
    ) {
        use std::io::SeekFrom;

        use qubit_operation::OperationOutcome;

        use crate::operation::LocalOperationOptions;
        use crate::operation::complete::LocalReadAtRequest;

        const BLOCK: usize = 8 * 1024;
        failed.wait_settlement().unwrap();
        let mut probe = filesystem
            .submit_read_at(
                manager,
                Default::default(),
                LocalReadAtRequest {
                    path: probe_path,
                    read: Default::default(),
                    offset: SeekFrom::Start(0),
                    max_bytes: BLOCK,
                },
                LocalOperationOptions {
                    max_owned_bytes: Some(BLOCK),
                    ..Default::default()
                },
            )
            .unwrap()
            .0;
        assert!(matches!(probe.wait_outcome().unwrap(), OperationOutcome::Succeeded(bytes) if bytes.len() == BLOCK));
        assert!(matches!(
            failed.try_take_outcome().unwrap(),
            Some(OperationOutcome::Failed(error))
                if error.resource_limit_error().is_some_and(|limit|
                    limit.resource() == LocalResourceKind::OwnedBytes)
        ));
    }

    #[test]
    fn test_delete_effect_mapping_distinguishes_no_change_and_completed_change() {
        use crate::LocalPublicationState;
        use crate::error::LocalFileEffectState;

        assert_eq!(
            (
                0,
                LocalPublicationState::NotPublished,
                EffectFact::Known,
                "local deletion failed before changing the namespace"
            ),
            deletion_effect_facts(Some(LocalFileEffectState::Unchanged)),
        );
        assert_eq!(
            (
                1,
                LocalPublicationState::Published,
                EffectFact::Known,
                "local deletion changed the namespace before a later failure"
            ),
            deletion_effect_facts(Some(LocalFileEffectState::Applied)),
        );
    }

    #[test]
    fn test_delete_effect_mapping_preserves_partial_and_unknown_evidence() {
        use crate::LocalPublicationState;
        use crate::error::LocalFileEffectState;

        assert_eq!(
            (
                1,
                LocalPublicationState::Unknown,
                EffectFact::Known,
                "local deletion partially changed the namespace; exact deletion count is unavailable"
            ),
            deletion_effect_facts(Some(LocalFileEffectState::PartiallyApplied)),
        );
        let unknown = (
            0,
            LocalPublicationState::Unknown,
            EffectFact::Unknown,
            "local deletion failed; namespace effects are unknown",
        );
        assert_eq!(
            unknown,
            deletion_effect_facts(Some(LocalFileEffectState::Indeterminate))
        );
        assert_eq!(unknown, deletion_effect_facts(None));
    }

    fn block_on<F: std::future::Future>(future: F) -> F::Output {
        use std::sync::Arc;
        use std::task::Context;
        use std::task::Wake;
        use std::task::Waker;

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
            if let std::task::Poll::Ready(value) = future.as_mut().poll(&mut context) {
                return value;
            }
            std::thread::park_timeout(std::time::Duration::from_secs(10));
        }
    }
    /// Runs the test stopped scope skips the first native call behavior while
    /// preserving the operation lifecycle invariants.
    #[test]
    fn test_stopped_scope_skips_the_first_native_call() {
        let manager = OperationManager::new(ManagerOptions::default()).unwrap();
        let native_calls = Arc::new(AtomicUsize::new(0));
        let native_calls_for_task = Arc::clone(&native_calls);
        let (entered_tx, entered_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let mut operation = manager
            .try_submit(Default::default(), move |scope| {
                entered_tx.send(()).unwrap();
                release_rx.recv().unwrap();
                assert!(matches!(
                    super::stop_outcome::<(), ()>(&scope),
                    Some(OperationOutcome::Stopped(report)) if report.reason == StopReason::UserCancelled
                ));
                assert!(super::stop_outcome::<Vec<u8>, super::LocalFileError>(&scope).is_some());
                assert!(super::stop_outcome::<super::LocalReadPrefix, super::LocalFileError>(&scope).is_some());
                assert!(
                    super::stop_outcome::<super::LocalCopyOperationResult, super::LocalCopyFailure>(&scope).is_some()
                );
                assert!(
                    super::stop_outcome::<super::LocalDeleteOperationResult, super::LocalFileError>(&scope).is_some()
                );
                assert!(
                    super::stop_outcome::<super::LocalRenameOperationResult, super::LocalRenameFailure>(&scope)
                        .is_some()
                );
                assert!(
                    super::stop_outcome::<super::LocalCreateDirectoryOperationResult, super::LocalFileError>(&scope)
                        .is_some()
                );
                let result = before_native_call(&scope, || native_calls_for_task.fetch_add(1, Ordering::SeqCst));
                Ok::<_, ()>(result.map_err(|stop| stop.reason))
            })
            .unwrap();

        entered_rx.recv().unwrap();
        let _ = operation.request_stop(StopReason::UserCancelled);
        release_tx.send(()).unwrap();

        assert!(matches!(
            operation.wait_outcome().unwrap(),
            OperationOutcome::Stopped(report) if report.reason == StopReason::UserCancelled
        ));
        assert_eq!(native_calls.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn test_complete_read_options_inherit_defaults_and_apply_outer_policy() {
        use std::time::Duration;

        use crate::LocalFileSystem;
        use crate::options::LocalReadAllOptions;
        use crate::options::LocalReadOptions;
        use crate::policy::LocalSymlinkPolicy;

        let mut filesystem = LocalFileSystem::host().unwrap();
        filesystem
            .set_default_read_options(
                LocalReadOptions::new()
                    .with_open_retry_timeout(Duration::from_millis(25))
                    .with_symlink_policy(LocalSymlinkPolicy::FollowWithinScope),
            )
            .unwrap();

        let inherited = effective_read_options(
            &filesystem,
            &LocalReadAllOptions::new().with_symlink_policy(LocalSymlinkPolicy::Reject),
        );
        assert_eq!(inherited.open_retry_timeout(), Some(Duration::from_millis(25)));
        assert_eq!(inherited.symlink_policy(), Some(LocalSymlinkPolicy::Reject));

        let replaced = effective_read_options(
            &filesystem,
            &LocalReadAllOptions::new()
                .with_read_options(LocalReadOptions::new())
                .with_symlink_policy(LocalSymlinkPolicy::Reject),
        );
        assert_eq!(replaced.open_retry_timeout(), None);
        assert_eq!(replaced.symlink_policy(), Some(LocalSymlinkPolicy::Reject));
    }

    #[test]
    fn test_effective_complete_read_limit_inherits_and_tightens_executor_ceiling() {
        use crate::options::LocalReadAllOptions;

        assert_eq!((8, false), effective_read_limit(&LocalReadAllOptions::new(), 8));
        assert_eq!(
            (4, true),
            effective_read_limit(&LocalReadAllOptions::new().with_max_bytes(4), 8)
        );
        assert_eq!(
            (8, false),
            effective_read_limit(&LocalReadAllOptions::new().with_max_bytes(12), 8)
        );
        assert_eq!(
            (8, true),
            effective_read_limit(&LocalReadAllOptions::new().with_max_bytes(8), 8)
        );
    }

    #[test]
    fn test_read_limit_error_preserves_explicit_and_memory_limit_facts() {
        use crate::error::LocalFileErrorKind;
        use crate::error::LocalResourceKind;

        let path = Path::new("/bounded/read");
        let explicit = read_limit_error(true, 4, 5, 0, 8, path, None);
        assert_eq!(LocalFileErrorKind::ResourceLimit, explicit.kind());
        assert_eq!(Some(path), explicit.path());
        assert_eq!(
            Some((4, 5)),
            explicit
                .read_limit_error()
                .map(|limit| (limit.limit(), limit.observed()))
        );

        let memory = read_limit_error(false, 4, 5, 4, 4, path, None);
        assert_eq!(LocalFileErrorKind::ResourceLimit, memory.kind());
        assert_eq!(Some(path), memory.path());
        let facts = memory.resource_limit_error().expect("memory limit must be structured");
        assert_eq!(LocalResourceKind::OwnedBytes, facts.resource());
        assert_eq!((4, 0, 1), (facts.limit(), facts.remaining(), facts.requested()));
        assert_eq!(Some(5), facts.observed());
    }

    #[test]
    fn test_complete_text_decoder_preserves_success_and_invalid_utf8_path() {
        use crate::error::LocalFileErrorKind;

        let path = Path::new("invalid-text");
        assert_eq!("valid", decode_complete_text(b"valid".to_vec(), path, None).unwrap());
        let error = decode_complete_text(vec![0xff], path, None).expect_err("invalid UTF-8 must retain a typed error");
        assert_eq!(LocalFileErrorKind::InvalidEncoding, error.kind());
        assert_eq!(Some(path), error.path());
    }

    #[test]
    fn test_read_all_and_text_success_cover_explicit_complete_read_limits() {
        use crate::LocalReadRequest;
        use crate::operation::AsyncLocalFileSystem;
        use crate::operation::LocalExecutionConfig;
        use crate::operation::LocalIoExecutor;
        use crate::options::LocalReadAllOptions;

        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("bounded-text");
        std::fs::write(&path, b"abc").unwrap();
        let manager = OperationManager::new(ManagerOptions::default()).unwrap();
        let executor = LocalIoExecutor::new(manager.clone(), LocalExecutionConfig::default()).unwrap();
        let filesystem = block_on(AsyncLocalFileSystem::host(executor)).unwrap();

        let (mut bytes, _) = filesystem
            .submit_read_all(
                &manager,
                Default::default(),
                LocalReadRequest {
                    path: path.clone(),
                    read: LocalReadAllOptions::new().with_max_bytes(3),
                },
                Default::default(),
            )
            .unwrap();
        assert!(matches!(bytes.wait_outcome().unwrap(), OperationOutcome::Succeeded(value) if value == b"abc"));

        let (mut text, _) = filesystem
            .submit_read_text(
                &manager,
                Default::default(),
                LocalReadRequest {
                    path,
                    read: LocalReadAllOptions::new().with_max_bytes(3),
                },
                Default::default(),
            )
            .unwrap();
        assert!(matches!(text.wait_outcome().unwrap(), OperationOutcome::Succeeded(value) if value == "abc"));
    }

    #[test]
    fn test_stopped_complete_read_releases_owned_bytes_before_outcome_is_taken() {
        use std::sync::mpsc;

        use qubit_operation::OperationOutcome;

        use crate::LocalReadRequest;
        use crate::operation::AsyncLocalFileSystem;
        use crate::operation::LocalExecutionConfig;
        use crate::operation::LocalIoExecutor;
        use crate::options::LocalReadAllOptions;

        let _gate_lock = READ_ALL_TEST_GATE_LOCK
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        const READ_SIZE: usize = 128 * 1024;
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("cancel-after-read");
        std::fs::write(&path, vec![1; READ_SIZE]).unwrap();
        let manager = OperationManager::new(ManagerOptions::default()).unwrap();
        let executor = LocalIoExecutor::new(
            manager.clone(),
            LocalExecutionConfig {
                max_owned_bytes: READ_SIZE,
                ..LocalExecutionConfig::default()
            },
        )
        .unwrap();
        let filesystem = block_on(AsyncLocalFileSystem::host(executor)).unwrap();

        let (entered_tx, entered_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let _gate = ReadAllTestGateGuard::install(Arc::as_ptr(&filesystem.filesystem.core) as usize, path.clone(), entered_tx, release_rx);
        let mut stopped = filesystem
            .submit_read_all(
                &manager,
                Default::default(),
                LocalReadRequest {
                    path: path.clone(),
                    read: LocalReadAllOptions::new(),
                },
                Default::default(),
            )
            .unwrap()
            .0;
        entered_rx
            .recv_timeout(std::time::Duration::from_secs(10))
            .expect("read-all must reach the gate after allocating and reading a block");
        let _ = stopped.request_stop(StopReason::UserCancelled);
        release_tx.send(()).unwrap();
        stopped.wait_settlement().unwrap();

        let mut probe = filesystem
            .submit_read_all(
                &manager,
                Default::default(),
                LocalReadRequest {
                    path,
                    read: LocalReadAllOptions::new(),
                },
                Default::default(),
            )
            .unwrap()
            .0;
        probe.wait_settlement().unwrap();
        assert!(matches!(
            stopped.try_take_outcome().unwrap(),
            Some(OperationOutcome::Stopped(_))
        ));
        assert!(matches!(
            probe.try_take_outcome().unwrap(),
            Some(OperationOutcome::Succeeded(bytes)) if bytes.len() == READ_SIZE
        ));
    }

    #[test]
    fn test_prefix_read_rechecks_stop_after_eof_and_truncation_probe() {
        use crate::operation::AsyncLocalFileSystem;
        use crate::operation::LocalExecutionConfig;
        use crate::operation::LocalIoExecutor;
        use crate::operation::LocalOperationOptions;
        use crate::operation::complete::LocalReadPrefixRequest;
        use crate::options::LocalReadOptions;

        let _gate_lock = READ_ALL_TEST_GATE_LOCK
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        for (label, max_bytes) in [("eof", 8), ("truncated", 2)] {
            let directory = tempfile::tempdir().unwrap();
            let path = directory.path().join(label);
            std::fs::write(&path, b"ab").unwrap();
            let manager = OperationManager::new(ManagerOptions::default()).unwrap();
            let executor = LocalIoExecutor::new(
                manager.clone(),
                LocalExecutionConfig {
                    max_owned_bytes: 2,
                    ..LocalExecutionConfig::default()
                },
            )
            .unwrap();
            let filesystem = block_on(AsyncLocalFileSystem::host(executor.clone())).unwrap();
            let (entered_tx, entered_rx) = mpsc::channel();
            let (release_tx, release_rx) = mpsc::channel();
            let _gate = ReadAllTestGateGuard::install(Arc::as_ptr(&filesystem.filesystem.core) as usize, path.clone(), entered_tx, release_rx);
            let (stage_tx, stage_rx) = mpsc::channel();
            let (discard_release_tx, discard_release_rx) = mpsc::channel();
            let _discard_gate = DiscardBufferTestGateGuard::install(Arc::as_ptr(&filesystem.filesystem.core) as usize, path.clone(), stage_tx, discard_release_rx);
            let (mut stopped, _) = filesystem
                .submit_read_prefix(
                    &manager,
                    Default::default(),
                    LocalReadPrefixRequest {
                        path: path.clone(),
                        read: LocalReadOptions::default(),
                        max_bytes,
                    },
                    LocalOperationOptions {
                        max_owned_bytes: Some(2),
                        ..Default::default()
                    },
                )
                .unwrap();
            entered_rx
                .recv_timeout(std::time::Duration::from_secs(5))
                .expect("prefix read reaches the pre-delivery gate");
            let _ = stopped.request_stop(StopReason::UserCancelled);
            release_tx.send(()).unwrap();
            let before_drop = stage_rx.recv_timeout(std::time::Duration::from_secs(5)).unwrap();
            let reservation_before_drop = executor.reserve_bytes(2).is_none();
            discard_release_tx.send(()).unwrap();
            let after_drop = stage_rx.recv_timeout(std::time::Duration::from_secs(5)).unwrap();
            let reservation_after_drop = executor.reserve_bytes(2).is_none();
            discard_release_tx.send(()).unwrap();
            stopped.wait_settlement().unwrap();
            assert_eq!(before_drop, DiscardBufferTestStage::BeforeDrop);
            assert!(
                reservation_before_drop,
                "stopped output retains its reservation before destruction"
            );
            assert_eq!(after_drop, DiscardBufferTestStage::AfterDrop);
            assert!(reservation_after_drop, "stopped output refunds only after destruction");
            assert!(
                matches!(stopped.try_take_outcome().unwrap(), Some(OperationOutcome::Stopped(report)) if report.reason == StopReason::UserCancelled)
            );

            let (mut probe, _) = filesystem
                .submit_read_prefix(
                    &manager,
                    Default::default(),
                    LocalReadPrefixRequest {
                        path,
                        read: LocalReadOptions::default(),
                        max_bytes: 2,
                    },
                    LocalOperationOptions {
                        max_owned_bytes: Some(2),
                        ..Default::default()
                    },
                )
                .unwrap();
            assert!(
                matches!(probe.wait_outcome().unwrap(), OperationOutcome::Succeeded(value) if value.bytes == b"ab")
            );
        }
    }

    #[test]
    fn test_read_at_rechecks_stop_before_delivering_eof_and_zero_length_success() {
        use std::io::SeekFrom;
        use std::sync::mpsc;

        use qubit_operation::OperationOutcome;
        use qubit_operation::StopReason;

        use crate::operation::AsyncLocalFileSystem;
        use crate::operation::LocalExecutionConfig;
        use crate::operation::LocalIoExecutor;
        use crate::operation::complete::LocalReadAtRequest;

        let _gate_lock = READ_ALL_TEST_GATE_LOCK
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        for (name, payload, max_bytes) in [("eof", b"ab".as_slice(), 8), ("zero", b"ab".as_slice(), 0)] {
            let directory = tempfile::tempdir().unwrap();
            let path = directory.path().join(name);
            let probe_path = directory.path().join("probe");
            std::fs::write(&path, payload).unwrap();
            std::fs::write(&probe_path, b"ok").unwrap();
            let manager = OperationManager::new(ManagerOptions::default()).unwrap();
            let executor = LocalIoExecutor::new(
                manager.clone(),
                LocalExecutionConfig {
                    max_owned_bytes: 2,
                    worker_count: 2,
                    ..Default::default()
                },
            )
            .unwrap();
            let filesystem = block_on(AsyncLocalFileSystem::host(executor.clone())).unwrap();
            let (entered_tx, entered_rx) = mpsc::channel();
            let (release_tx, release_rx) = mpsc::channel();
            let _gate = ReadAtSuccessTestGateGuard::install(Arc::as_ptr(&filesystem.filesystem.core) as usize, path.clone(), entered_tx, release_rx);
            let (stage_tx, stage_rx) = mpsc::channel();
            let (discard_release_tx, discard_release_rx) = mpsc::channel();
            let _discard_gate = if max_bytes == 0 {
                None
            } else {
                Some(DiscardBufferTestGateGuard::install(
                    Arc::as_ptr(&filesystem.filesystem.core) as usize,
                    path.clone(),
                    stage_tx,
                    discard_release_rx,
                ))
            };
            let (mut stopped, _) = filesystem
                .submit_read_at(
                    &manager,
                    Default::default(),
                    LocalReadAtRequest {
                        path: path.clone(),
                        read: Default::default(),
                        offset: SeekFrom::Start(0),
                        max_bytes,
                    },
                    Default::default(),
                )
                .unwrap();
            entered_rx
                .recv_timeout(std::time::Duration::from_secs(5))
                .expect("read-at reaches its success gate");
            assert_eq!(
                stopped.request_stop(StopReason::UserCancelled),
                qubit_operation::StopRequestStatus::Accepted
            );
            release_tx.send(()).unwrap();
            if max_bytes > 0 {
                let before_drop = stage_rx.recv_timeout(std::time::Duration::from_secs(5)).unwrap();
                let reservation_before_drop = executor.reserve_bytes(2).is_none();
                discard_release_tx.send(()).unwrap();
                let after_drop = stage_rx.recv_timeout(std::time::Duration::from_secs(5)).unwrap();
                let reservation_after_drop = executor.reserve_bytes(2).is_none();
                discard_release_tx.send(()).unwrap();
                assert_eq!(before_drop, DiscardBufferTestStage::BeforeDrop);
                assert!(
                    reservation_before_drop,
                    "stopped output retains its reservation before destruction"
                );
                assert_eq!(after_drop, DiscardBufferTestStage::AfterDrop);
                assert!(reservation_after_drop, "stopped output refunds only after destruction");
            }
            stopped.wait_settlement().unwrap();

            let mut probe = filesystem
                .submit_read_at(
                    &manager,
                    Default::default(),
                    LocalReadAtRequest {
                        path: probe_path,
                        read: Default::default(),
                        offset: SeekFrom::Start(0),
                        max_bytes: 2,
                    },
                    Default::default(),
                )
                .unwrap()
                .0;
            assert!(matches!(probe.wait_outcome().unwrap(), OperationOutcome::Succeeded(bytes) if bytes == b"ok"));
            assert!(
                matches!(stopped.try_take_outcome().unwrap(), Some(OperationOutcome::Stopped(report)) if report.reason == StopReason::UserCancelled)
            );
        }
    }

    #[test]
    fn test_prefix_partial_output_refunds_shared_capacity_after_failure() {
        use std::io::SeekFrom;

        use crate::LocalOperationOptions;
        use crate::operation::AsyncLocalFileSystem;
        use crate::operation::LocalExecutionConfig;
        use crate::operation::LocalIoExecutor;
        use crate::operation::complete::LocalReadAtRequest;
        use crate::operation::complete::LocalReadPrefixRequest;
        use crate::options::LocalReadOptions;

        let _gate_lock = READ_ALL_TEST_GATE_LOCK
            .lock()
            .unwrap_or_else(|error| error.into_inner());

        const BLOCK: usize = 8 * 1024;
        const POOL: usize = 2 * BLOCK;
        let directory = tempfile::tempdir().unwrap();
        let occupied_path = directory.path().join("occupied");
        let failed_path = directory.path().join("prefix-failure");
        let probe_path = directory.path().join("probe");
        std::fs::write(&occupied_path, vec![1; BLOCK]).unwrap();
        std::fs::write(&failed_path, vec![2; BLOCK * 3]).unwrap();
        std::fs::write(&probe_path, vec![3; BLOCK]).unwrap();
        let manager = OperationManager::new(ManagerOptions::default()).unwrap();
        let executor = LocalIoExecutor::new(
            manager.clone(),
            LocalExecutionConfig {
                max_owned_bytes: POOL,
                ..Default::default()
            },
        )
        .unwrap();
        let filesystem = block_on(AsyncLocalFileSystem::host(executor.clone())).unwrap();
        let occupied = filesystem
            .submit_read_at(
                &manager,
                Default::default(),
                LocalReadAtRequest {
                    path: occupied_path,
                    read: Default::default(),
                    offset: SeekFrom::Start(0),
                    max_bytes: BLOCK,
                },
                LocalOperationOptions {
                    max_owned_bytes: Some(BLOCK),
                    ..Default::default()
                },
            )
            .unwrap()
            .0;
        occupied.wait_settlement().unwrap();
        let (stage_tx, stage_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let _discard_gate = DiscardBufferTestGateGuard::install(Arc::as_ptr(&filesystem.filesystem.core) as usize, failed_path.clone(), stage_tx, release_rx);
        let mut failed = filesystem
            .submit_read_prefix(
                &manager,
                Default::default(),
                LocalReadPrefixRequest {
                    path: failed_path,
                    read: LocalReadOptions::default(),
                    max_bytes: POOL,
                },
                LocalOperationOptions::default(),
            )
            .unwrap()
            .0;

        assert_eq!(
            stage_rx.recv_timeout(std::time::Duration::from_secs(5)).unwrap(),
            DiscardBufferTestStage::BeforeDrop
        );
        assert!(
            executor.reserve_bytes(BLOCK).is_none(),
            "partial output still owns its reservation before drop"
        );
        release_tx.send(()).unwrap();
        assert_eq!(
            stage_rx.recv_timeout(std::time::Duration::from_secs(5)).unwrap(),
            DiscardBufferTestStage::AfterDrop
        );
        assert!(
            executor.reserve_bytes(BLOCK).is_none(),
            "reservation remains held until after buffer destruction"
        );
        release_tx.send(()).unwrap();

        assert_partial_read_budget_is_reusable(&filesystem, &manager, &mut failed, probe_path);
        drop(occupied);
    }

    #[test]
    fn test_read_at_partial_output_refunds_shared_capacity_after_failure() {
        use std::io::SeekFrom;

        use crate::LocalOperationOptions;
        use crate::operation::AsyncLocalFileSystem;
        use crate::operation::LocalExecutionConfig;
        use crate::operation::LocalIoExecutor;
        use crate::operation::complete::LocalReadAtRequest;

        let _gate_lock = READ_ALL_TEST_GATE_LOCK
            .lock()
            .unwrap_or_else(|error| error.into_inner());

        const BLOCK: usize = 8 * 1024;
        const POOL: usize = 2 * BLOCK;
        let directory = tempfile::tempdir().unwrap();
        let occupied_path = directory.path().join("occupied");
        let failed_path = directory.path().join("read-at-failure");
        let probe_path = directory.path().join("probe");
        std::fs::write(&occupied_path, vec![1; BLOCK]).unwrap();
        std::fs::write(&failed_path, vec![2; BLOCK * 3]).unwrap();
        std::fs::write(&probe_path, vec![3; BLOCK]).unwrap();
        let manager = OperationManager::new(ManagerOptions::default()).unwrap();
        let executor = LocalIoExecutor::new(
            manager.clone(),
            LocalExecutionConfig {
                max_owned_bytes: POOL,
                ..Default::default()
            },
        )
        .unwrap();
        let filesystem = block_on(AsyncLocalFileSystem::host(executor.clone())).unwrap();
        let occupied = filesystem
            .submit_read_at(
                &manager,
                Default::default(),
                LocalReadAtRequest {
                    path: occupied_path,
                    read: Default::default(),
                    offset: SeekFrom::Start(0),
                    max_bytes: BLOCK,
                },
                LocalOperationOptions {
                    max_owned_bytes: Some(BLOCK),
                    ..Default::default()
                },
            )
            .unwrap()
            .0;
        occupied.wait_settlement().unwrap();
        let (stage_tx, stage_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let _discard_gate = DiscardBufferTestGateGuard::install(Arc::as_ptr(&filesystem.filesystem.core) as usize, failed_path.clone(), stage_tx, release_rx);
        let mut failed = filesystem
            .submit_read_at(
                &manager,
                Default::default(),
                LocalReadAtRequest {
                    path: failed_path,
                    read: Default::default(),
                    offset: SeekFrom::Start(0),
                    max_bytes: POOL,
                },
                LocalOperationOptions::default(),
            )
            .unwrap()
            .0;

        assert_eq!(
            stage_rx.recv_timeout(std::time::Duration::from_secs(5)).unwrap(),
            DiscardBufferTestStage::BeforeDrop
        );
        assert!(
            executor.reserve_bytes(BLOCK).is_none(),
            "partial output still owns its reservation before drop"
        );
        release_tx.send(()).unwrap();
        assert_eq!(
            stage_rx.recv_timeout(std::time::Duration::from_secs(5)).unwrap(),
            DiscardBufferTestStage::AfterDrop
        );
        assert!(
            executor.reserve_bytes(BLOCK).is_none(),
            "reservation remains held until after buffer destruction"
        );
        release_tx.send(()).unwrap();

        assert_partial_read_budget_is_reusable(&filesystem, &manager, &mut failed, probe_path);
        drop(occupied);
    }

    #[test]
    fn test_read_all_failure_releases_owned_bytes_before_outcome_is_taken() {
        use std::io::SeekFrom;
        use std::sync::mpsc;

        use qubit_operation::OperationOutcome;

        use crate::LocalReadRequest;
        use crate::operation::AsyncLocalFileSystem;
        use crate::operation::LocalExecutionConfig;
        use crate::operation::LocalIoExecutor;
        use crate::operation::complete::LocalReadAtRequest;
        use crate::options::LocalReadAllOptions;

        let _gate_lock = READ_ALL_TEST_GATE_LOCK
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        const HALF_POOL: usize = 64 * 1024;
        let directory = tempfile::tempdir().unwrap();
        let occupied_path = directory.path().join("occupied");
        let full_path = directory.path().join("full");
        let probe_path = directory.path().join("probe");
        std::fs::write(&occupied_path, vec![1; HALF_POOL]).unwrap();
        std::fs::write(&full_path, vec![2; HALF_POOL * 2]).unwrap();
        std::fs::write(&probe_path, vec![3; HALF_POOL / 2]).unwrap();

        let manager = OperationManager::new(ManagerOptions::default()).unwrap();
        let executor = LocalIoExecutor::new(
            manager.clone(),
            LocalExecutionConfig {
                max_owned_bytes: HALF_POOL * 2,
                ..LocalExecutionConfig::default()
            },
        )
        .unwrap();
        let filesystem = block_on(AsyncLocalFileSystem::host(executor)).unwrap();

        let occupied = filesystem
            .submit_read_at(
                &manager,
                Default::default(),
                LocalReadAtRequest {
                    path: occupied_path,
                    read: Default::default(),
                    offset: SeekFrom::Start(0),
                    max_bytes: HALF_POOL,
                },
                Default::default(),
            )
            .unwrap()
            .0;
        occupied.wait_settlement().unwrap();

        let (entered_tx, entered_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let _gate = ReadAllTestGateGuard::install(Arc::as_ptr(&filesystem.filesystem.core) as usize, full_path.clone(), entered_tx, release_rx);
        let mut failed = filesystem
            .submit_read_all(
                &manager,
                Default::default(),
                LocalReadRequest {
                    path: full_path,
                    read: LocalReadAllOptions::new(),
                },
                Default::default(),
            )
            .unwrap()
            .0;
        entered_rx
            .recv_timeout(std::time::Duration::from_secs(10))
            .expect("read-all must reach the gate after allocating and reading one block");
        release_tx.send(()).unwrap();
        failed.wait_settlement().unwrap();

        let mut probe = filesystem
            .submit_read_all(
                &manager,
                Default::default(),
                LocalReadRequest {
                    path: probe_path,
                    read: LocalReadAllOptions::new(),
                },
                crate::LocalOperationOptions {
                    max_owned_bytes: Some(HALF_POOL / 2),
                    ..Default::default()
                },
            )
            .unwrap()
            .0;
        probe.wait_settlement().unwrap();
        let probe_outcome = probe.try_take_outcome().unwrap();
        assert!(
            matches!(
                &probe_outcome,
                Some(OperationOutcome::Succeeded(bytes)) if bytes.len() == HALF_POOL / 2
            ),
            "probe outcome: {probe_outcome:?}"
        );
        assert!(matches!(
            failed.try_take_outcome().unwrap(),
            Some(OperationOutcome::Failed(error))
                if error.resource_limit_error().is_some_and(|limit|
                    limit.resource() == crate::error::LocalResourceKind::OwnedBytes)
        ));
        drop(occupied);
    }

    #[test]
    fn test_read_all_gate_ignores_unrelated_paths() {
        let _gate_lock = READ_ALL_TEST_GATE_LOCK
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let target = PathBuf::from("target-read");
        let (entered_tx, _entered_rx) = mpsc::channel();
        let (_release_tx, release_rx) = mpsc::channel();
        let _guard = ReadAllTestGateGuard::install(0, target.clone(), entered_tx, release_rx);

        super::pause_read_all_test_gate(0, Path::new("unrelated-read"));
        assert!(
            READ_ALL_TEST_GATE
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .as_ref()
                .is_some_and(|(_, path, _, _)| path == &target)
        );
    }

    #[test]
    fn test_read_all_gate_ignores_same_virtual_path_from_another_root() {
        use std::sync::mpsc;

        use crate::LocalReadRequest;
        use crate::operation::AsyncLocalFileSystem;
        use crate::operation::LocalExecutionConfig;
        use crate::operation::LocalIoExecutor;
        use crate::options::LocalReadAllOptions;

        let _gate_lock = READ_ALL_TEST_GATE_LOCK.lock().unwrap_or_else(|error| error.into_inner());
        let first_root = tempfile::tempdir().unwrap();
        let second_root = tempfile::tempdir().unwrap();
        std::fs::write(first_root.path().join("same"), vec![1; 128 * 1024]).unwrap();
        std::fs::write(second_root.path().join("same"), b"other root").unwrap();
        let manager = OperationManager::new(ManagerOptions::default()).unwrap();
        let executor = LocalIoExecutor::new(
            manager.clone(),
            LocalExecutionConfig { worker_count: 2, ..Default::default() },
        ).unwrap();
        let first = block_on(AsyncLocalFileSystem::rooted(executor.clone(), first_root.path().to_path_buf())).unwrap();
        let second = block_on(AsyncLocalFileSystem::rooted(executor, second_root.path().to_path_buf())).unwrap();
        let path = PathBuf::from("/same");
        let (entered_tx, entered_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let _gate = ReadAllTestGateGuard::install(
            Arc::as_ptr(&first.filesystem.core) as usize,
            path.clone(),
            entered_tx,
            release_rx,
        );
        let mut second_operation = second.submit_read_all(
            &manager,
            Default::default(),
            LocalReadRequest { path: path.clone(), read: LocalReadAllOptions::new() },
            Default::default(),
        ).unwrap().0;
        assert!(matches!(second_operation.wait_outcome().unwrap(), OperationOutcome::Succeeded(bytes) if bytes == b"other root"));

        let mut first_operation = first.submit_read_all(
            &manager,
            Default::default(),
            LocalReadRequest { path, read: LocalReadAllOptions::new() },
            Default::default(),
        ).unwrap().0;
        entered_rx.recv_timeout(std::time::Duration::from_secs(5)).expect("target root reaches read gate");
        release_tx.send(()).unwrap();
        assert!(matches!(first_operation.wait_outcome().unwrap(), OperationOutcome::Succeeded(bytes) if bytes.len() == 128 * 1024));
    }

    #[cfg(panic = "unwind")]
    #[test]
    fn test_gate_helpers_recover_a_poisoned_registry() {
        let _gate_lock = READ_ALL_TEST_GATE_LOCK
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let poison = std::panic::catch_unwind(|| {
            let _registry = READ_ALL_TEST_GATE.lock().unwrap();
            panic!("poison the test-only gate registry");
        });
        assert!(poison.is_err());

        let (entered_tx, _entered_rx) = mpsc::channel();
        let (_release_tx, release_rx) = mpsc::channel();
        let guard = ReadAllTestGateGuard::install(0, PathBuf::from("registered"), entered_tx, release_rx);
        super::pause_read_all_test_gate(0, Path::new("unrelated"));
        drop(guard);
        assert!(
            READ_ALL_TEST_GATE
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .is_none()
        );
    }

    #[cfg(panic = "unwind")]
    #[test]
    fn test_discard_buffer_gate_recovers_poisoned_registry_and_release() {
        let _gate_lock = READ_ALL_TEST_GATE_LOCK
            .lock()
            .unwrap_or_else(|error| error.into_inner());
        let path = PathBuf::from("discard-poisoned-gate");
        let poison_registry = std::panic::catch_unwind(|| {
            let _registry = super::DISCARD_BUFFER_TEST_GATE.lock().unwrap();
            panic!("poison the discard-buffer test gate registry");
        });
        assert!(poison_registry.is_err());

        let (entered_tx, entered_rx) = mpsc::channel();
        let (release_tx, release_rx) = mpsc::channel();
        let guard = DiscardBufferTestGateGuard::install(0, path.clone(), entered_tx, release_rx);
        let release = super::DISCARD_BUFFER_TEST_GATE
            .lock()
            .unwrap_or_else(|error| error.into_inner())
            .as_ref()
            .expect("discard gate installed")
            .3
            .clone();
        let poison_release = std::panic::catch_unwind(|| {
            let _release = release.lock().unwrap();
            panic!("poison the discard-buffer test gate release");
        });
        assert!(poison_release.is_err());

        release_tx.send(()).unwrap();
        super::pause_discard_buffer_test_gate(0, &path, DiscardBufferTestStage::BeforeDrop);
        assert_eq!(
            entered_rx.recv().unwrap(),
            DiscardBufferTestStage::BeforeDrop,
            "the gate must notify after recovering its poisoned registry and release locks"
        );
        drop(guard);
        assert!(
            super::DISCARD_BUFFER_TEST_GATE
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .is_none()
        );
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

        let path = std::path::Path::new("overflow");
        let error = super::reserve_capacity(&mut bytes, usize::MAX, &permit, 8, path).expect_err("length overflow");
        assert!(error.to_string().contains("read capacity overflow"));
        assert_eq!(pool.used(), 1);
        super::reserve_capacity(&mut bytes, 1, &permit, 8, path).expect("grow within byte budget");
        assert_eq!(pool.used(), bytes.capacity());
        assert_eq!(bytes, [1]);
        super::shrink_to_zero(&permit);
        assert_eq!(pool.used(), 0);
    }

    #[test]
    fn test_discarded_read_buffer_keeps_its_budget_until_drop() {
        use crate::operation::internal::permits::PermitPool;

        struct DropProbe(Arc<PermitPool>);

        impl Drop for DropProbe {
            fn drop(&mut self) {
                assert_eq!(self.0.used(), 1, "the buffer's permit remains held during drop");
            }
        }

        let pool = Arc::new(PermitPool::new(1));
        let permit = Arc::new(std::sync::Mutex::new(pool.try_acquire(1).expect("initial permit")));
        super::discard_buffer_and_refund(
            DropProbe(Arc::clone(&pool)),
            &permit,
            std::path::Path::new("discard-probe"),
            0,
        );

        assert_eq!(pool.used(), 0, "the permit is refunded after buffer destruction");
        assert!(
            pool.try_acquire(1).is_some(),
            "another operation can reuse the refunded byte"
        );
    }
}

#[cfg(test)]
mod operation_submission_tests {
    use qubit_operation::ManagerOptions;
    use qubit_operation::OperationManager;
    use qubit_operation::OperationOutcome;

    use super::AsyncLocalFileSystem;
    use crate::LocalFileSystem;
    use crate::LocalOperationOptions;
    use crate::LocalReadRequest;
    use crate::operation::LocalExecutionConfig;
    use crate::operation::LocalIoExecutor;
    use crate::operation::complete::LocalCopyRequest;
    use crate::operation::complete::LocalCreateDirectoryRequest;
    use crate::operation::complete::LocalDeleteRequest;
    use crate::operation::complete::LocalReadPrefixRequest;
    use crate::operation::complete::LocalRenameRequest;
    use crate::options::LocalCopyOptions;
    use crate::options::LocalDeleteOptions;
    use crate::options::LocalReadAllOptions;
    use crate::options::LocalReadOptions;
    use crate::options::LocalRenameOptions;

    #[test]
    fn test_complete_operation_submitters_run_from_the_library_test_target() {
        let directory = tempfile::tempdir().unwrap();
        let source = directory.path().join("source.txt");
        let copied = directory.path().join("copied.txt");
        let renamed = directory.path().join("renamed.txt");
        let created = directory.path().join("created");
        std::fs::write(&source, b"coverage payload").unwrap();

        let manager = OperationManager::new(ManagerOptions::default()).unwrap();
        let executor = LocalIoExecutor::new(manager.clone(), LocalExecutionConfig::default()).unwrap();
        let filesystem = AsyncLocalFileSystem::new(LocalFileSystem::host().unwrap(), executor, None);

        let (mut text, _) = filesystem
            .submit_read_text(
                &manager,
                Default::default(),
                LocalReadRequest {
                    path: source.clone(),
                    read: LocalReadAllOptions::default(),
                },
                LocalOperationOptions::default(),
            )
            .unwrap();
        assert!(
            matches!(text.wait_outcome().unwrap(), OperationOutcome::Succeeded(value) if value == "coverage payload")
        );

        let (mut prefix, _) = filesystem
            .submit_read_prefix(
                &manager,
                Default::default(),
                LocalReadPrefixRequest {
                    path: source.clone(),
                    read: LocalReadOptions::default(),
                    max_bytes: 8,
                },
                LocalOperationOptions::default(),
            )
            .unwrap();
        assert!(
            matches!(prefix.wait_outcome().unwrap(), OperationOutcome::Succeeded(value) if value.bytes == b"coverage")
        );

        let (mut metadata, _) = filesystem
            .submit_metadata(
                &manager,
                Default::default(),
                source.clone(),
                LocalOperationOptions::default(),
            )
            .unwrap();
        assert!(matches!(metadata.wait_outcome().unwrap(), OperationOutcome::Succeeded(value) if value.len() == 16));

        let (mut copy, _) = filesystem
            .submit_copy(
                &manager,
                Default::default(),
                LocalCopyRequest {
                    source: source.clone(),
                    destination: copied.clone(),
                    options: LocalCopyOptions::default(),
                },
                LocalOperationOptions::default(),
            )
            .unwrap();
        assert!(matches!(copy.wait_outcome().unwrap(), OperationOutcome::Succeeded(_)));

        let (mut rename, _) = filesystem
            .submit_rename(
                &manager,
                Default::default(),
                LocalRenameRequest {
                    source: copied.clone(),
                    destination: renamed.clone(),
                    options: LocalRenameOptions::default(),
                },
                LocalOperationOptions::default(),
            )
            .unwrap();
        assert!(matches!(rename.wait_outcome().unwrap(), OperationOutcome::Succeeded(_)));

        let (mut create, _) = filesystem
            .submit_create_directory(
                &manager,
                Default::default(),
                LocalCreateDirectoryRequest {
                    path: created.clone(),
                    options: Default::default(),
                },
                LocalOperationOptions::default(),
            )
            .unwrap();
        assert!(matches!(create.wait_outcome().unwrap(), OperationOutcome::Succeeded(_)));

        let (mut delete, _) = filesystem
            .submit_delete(
                &manager,
                Default::default(),
                LocalDeleteRequest {
                    path: renamed.clone(),
                    options: LocalDeleteOptions::default(),
                    directory: false,
                },
                LocalOperationOptions::default(),
            )
            .unwrap();
        assert!(matches!(delete.wait_outcome().unwrap(), OperationOutcome::Succeeded(_)));
        assert!(created.is_dir());
        assert!(!renamed.exists());
    }
}
