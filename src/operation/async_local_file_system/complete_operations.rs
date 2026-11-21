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
use super::AsyncLocalFileSystem;
use super::OwnedSubmission;
use crate::LocalDurabilityState;
use crate::LocalFactSnapshot;
use crate::LocalFileError;
use crate::LocalFileErrorKind;
use crate::LocalFileMetadata;
use crate::LocalPublicationState;
use crate::error::LocalFileOperation;
use crate::error::LocalReadLimitError;
use crate::error::LocalResourceKind;
use crate::error::LocalResourceLimitError;
use crate::options::LocalMetadataPreservePolicy;
use crate::options::LocalReadAllOptions;
use crate::options::LocalReadOptions;
use crate::outcome::LocalCopyFailure;
use crate::outcome::LocalCopyFailureState;
use crate::outcome::LocalRenameFailure;
use crate::outcome::LocalRenameFailureState;

#[cfg(test)]
type ReadAllTestGate = (PathBuf, std::sync::mpsc::Sender<()>, std::sync::mpsc::Receiver<()>);
#[cfg(test)]
static READ_ALL_TEST_GATE: Mutex<Option<ReadAllTestGate>> = Mutex::new(None);
#[cfg(test)]
static READ_ALL_TEST_GATE_LOCK: Mutex<()> = Mutex::new(());

#[cfg(test)]
struct ReadAllTestGateGuard(PathBuf);

#[cfg(test)]
impl ReadAllTestGateGuard {
    fn install(path: PathBuf, entered: std::sync::mpsc::Sender<()>, release: std::sync::mpsc::Receiver<()>) -> Self {
        *READ_ALL_TEST_GATE.lock().unwrap_or_else(|error| error.into_inner()) = Some((path.clone(), entered, release));
        Self(path)
    }
}

#[cfg(test)]
impl Drop for ReadAllTestGateGuard {
    fn drop(&mut self) {
        let mut gate = READ_ALL_TEST_GATE.lock().unwrap_or_else(|error| error.into_inner());
        if gate.as_ref().is_some_and(|(path, _, _)| path == &self.0) {
            gate.take();
        }
    }
}

impl AsyncLocalFileSystem {
    /// Submits a finite complete read with per-block cooperative cancellation.
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
                let read_limit = request
                    .read
                    .max_bytes()
                    .map_or(maximum as u64, |limit| limit.min(maximum as u64));
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
                    outcome => outcome,
                }
            },
        )
    }
    /// Submits a complete UTF-8 read using the same allocation accounting as
    /// read-all.
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
                let read_limit = request
                    .read
                    .max_bytes()
                    .map_or(maximum as u64, |limit| limit.min(maximum as u64));
                match read_complete_cancellable(
                    &filesystem,
                    &execution_path,
                    &request.read,
                    read_limit,
                    scope,
                    &budget,
                    maximum,
                ) {
                    OperationOutcome::Succeeded(bytes) => match String::from_utf8(bytes) {
                        Ok(text) => OperationOutcome::Succeeded(text),
                        Err(source) => OperationOutcome::Failed(LocalFileError::from_utf8(
                            LocalFileOperation::Read,
                            Some(execution_path.clone()),
                            source,
                        )),
                    },
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
                    filesystem.open_reader_with_options(&execution_path, &request.read)
                }) {
                    Ok(result) => result,
                    Err(stop) => return OperationOutcome::Stopped(stop),
                };
                let mut reader = match reader_result {
                    Ok(reader) => reader,
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
                        shrink_to_zero(&budget);
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
                                shrink_to_zero(&budget);
                                return OperationOutcome::Failed(LocalFileError::from_io(
                                    LocalFileOperation::Read,
                                    Some(execution_path.clone()),
                                    None,
                                    error,
                                ));
                            }
                        }
                    }
                    let count = (request.max_bytes - bytes.len()).min(buffer.len());
                    match reader.read(&mut buffer[..count]) {
                        Ok(0) => break,
                        Ok(read) => {
                            if let Err(error) = reserve_capacity(&mut bytes, read, &budget, maximum, &execution_path) {
                                shrink_to_zero(&budget);
                                return OperationOutcome::Failed(error);
                            }
                            bytes.extend_from_slice(&buffer[..read]);
                        }
                        Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                        Err(error) => {
                            shrink_to_zero(&budget);
                            return OperationOutcome::Failed(LocalFileError::from_io(
                                LocalFileOperation::Read,
                                Some(execution_path.clone()),
                                None,
                                error,
                            ));
                        }
                    }
                }
                OperationOutcome::Succeeded(LocalReadPrefix { bytes, complete })
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
                    filesystem.open_reader_with_options(&execution_path, &request.read)
                }) {
                    Ok(result) => result,
                    Err(stop) => return OperationOutcome::Stopped(stop),
                };
                let mut reader = match reader_result {
                    Ok(reader) => reader,
                    Err(error) => return OperationOutcome::Failed(error),
                };
                if let Some(stopped) = stop_outcome(_scope) {
                    return stopped;
                }
                if let Err(error) = reader.seek(request.offset) {
                    return OperationOutcome::Failed(LocalFileError::from_io(
                        LocalFileOperation::Read,
                        Some(execution_path.clone()),
                        None,
                        error,
                    ));
                }
                let target = request.max_bytes.min(maximum);
                let mut bytes = Vec::new();
                let mut buffer = [0_u8; 8192];
                while bytes.len() < target {
                    if let Some(stopped) = stop_outcome(_scope) {
                        shrink_to_zero(&budget);
                        return stopped;
                    }
                    let count = (target - bytes.len()).min(buffer.len());
                    match reader.read(&mut buffer[..count]) {
                        Ok(0) => break,
                        Ok(read) => {
                            if let Err(error) = reserve_capacity(&mut bytes, read, &budget, maximum, &execution_path) {
                                shrink_to_zero(&budget);
                                return OperationOutcome::Failed(error);
                            }
                            bytes.extend_from_slice(&buffer[..read]);
                            if let Some(stopped) = stop_outcome(_scope) {
                                shrink_to_zero(&budget);
                                return stopped;
                            }
                        }
                        Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                        Err(error) => {
                            shrink_to_zero(&budget);
                            return OperationOutcome::Failed(LocalFileError::from_io(
                                LocalFileOperation::Read,
                                Some(execution_path.clone()),
                                None,
                                error,
                            ));
                        }
                    }
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
                        let metadata_only =
                            !published && outcome.metadata_preservation() == LocalMetadataPreservePolicy::Permissions;
                        record_local_effect(
                            scope,
                            &observer,
                            stats
                                .files()
                                .saturating_add(stats.directories())
                                .max(u64::from(metadata_only)),
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
                            stats.files().saturating_add(stats.directories()),
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
                            if recursive_directory {
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
                            deleted_entries: if recursive_directory {
                                None
                            } else if deleted {
                                Some(1)
                            } else {
                                Some(0)
                            },
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
                            "local deletion failed; partial recursive deletion count is unavailable",
                        );
                        OperationOutcome::Failed(error)
                    }
                }
            },
        )
    }
    /// Runs the submit owned behavior while preserving the operation lifecycle
    /// invariants.
    /// # Type Parameters
    /// - `R` — type used by this operation.
    /// - `T` — type used by this operation.
    /// - `E` — type used by this operation.
    /// - `F` — type used by this operation.
    /// # Parameters
    /// - `manager` — supplies data or policy for this operation.
    /// - `operation` — supplies data or policy for this operation.
    /// - `request` — supplies data or policy for this operation.
    /// - `path_is_relative` — supplies data or policy for this operation.
    /// - `required_handles` — supplies data or policy for this operation.
    /// - `limits` — supplies data or policy for this operation.
    /// - `task` — supplies data or policy for this operation.
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
    /// - `path_is_relative` — input value used by this operation.
    /// - `required_handles` — input value used by this operation.
    /// - `limits` — per-operation resource ceilings.
    /// - `initial_owned_bytes` — input value used by this operation.
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
/// Runs the read complete cancellable behavior while preserving the operation
/// lifecycle invariants.
/// # Parameters
/// - `filesystem` — supplies data or policy for this operation.
/// - `path` — supplies data or policy for this operation.
/// - `options` — supplies data or policy for this operation.
/// - `maximum` — supplies data or policy for this operation.
/// - `scope` — supplies data or policy for this operation.
/// - `budget` — supplies data or policy for this operation.
/// - `allocation_limit` — supplies data or policy for this operation.
fn read_complete_cancellable(
    filesystem: &crate::LocalFileSystem,
    path: &Path,
    options: &LocalReadAllOptions,
    maximum: u64,
    scope: &OperationScope,
    budget: &Arc<Mutex<Permit>>,
    allocation_limit: usize,
) -> OperationOutcome<Vec<u8>, LocalFileError> {
    use crate::observation::LocalObservationRequirement;
    use crate::observation::reader::LocalObservedReader;

    if let Some(stopped) = stop_outcome(scope) {
        return stopped;
    }
    let read_options = effective_read_options(filesystem, options);
    let reader_result = match before_native_call(scope, || filesystem.open_reader_with_options(path, &read_options)) {
        Ok(result) => result,
        Err(stop) => return OperationOutcome::Stopped(stop),
    };
    let reader = match reader_result {
        Ok(reader) => reader,
        Err(error) => return OperationOutcome::Failed(error),
    };
    let mut observed = match LocalObservedReader::from_reader(reader, LocalObservationRequirement::Required) {
        Ok(reader) => reader.bind_error_context(path.to_path_buf(), None),
        Err(error) => return OperationOutcome::Failed(error),
    };
    let before = observed.observation().clone();
    let observed_len = before.metadata().len();
    if observed_len > maximum {
        return OperationOutcome::Failed(LocalFileError::from_read_limit(
            LocalFileOperation::Read,
            Some(path.to_path_buf()),
            LocalReadLimitError::new(maximum, observed_len),
        ));
    }

    let mut bytes = Vec::new();
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        if let Some(stopped) = stop_outcome(scope) {
            return stopped;
        }
        if bytes.len() as u64 == maximum {
            let mut sentinel = [0_u8; 1];
            match observed.read(&mut sentinel) {
                Ok(0) => break,
                Ok(_) => {
                    return OperationOutcome::Failed(LocalFileError::from_read_limit(
                        LocalFileOperation::Read,
                        Some(path.to_path_buf()),
                        LocalReadLimitError::new(maximum, maximum.saturating_add(1)),
                    ));
                }
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(error) => {
                    return OperationOutcome::Failed(LocalFileError::from_io(
                        LocalFileOperation::Read,
                        Some(path.to_path_buf()),
                        None,
                        error,
                    ));
                }
            }
        }
        let remaining = (maximum - bytes.len() as u64).min(buffer.len() as u64) as usize;
        match observed.read(&mut buffer[..remaining]) {
            Ok(0) => break,
            Ok(read) => {
                if let Err(error) = reserve_capacity(&mut bytes, read, budget, allocation_limit, path) {
                    return OperationOutcome::Failed(error);
                }
                bytes.extend_from_slice(&buffer[..read]);
                #[cfg(test)]
                pause_read_all_test_gate(path);
                if let Some(stopped) = stop_outcome(scope) {
                    return stopped;
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(error) => {
                return OperationOutcome::Failed(LocalFileError::from_io(
                    LocalFileOperation::Read,
                    Some(path.to_path_buf()),
                    None,
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
        return OperationOutcome::Failed(
            LocalFileError::new(LocalFileErrorKind::ChangedDuringRead, LocalFileOperation::Read)
                .with_path(path.to_path_buf()),
        );
    }
    if let Some(stopped) = stop_outcome(scope) {
        return stopped;
    }
    OperationOutcome::Succeeded(bytes)
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
fn pause_read_all_test_gate(path: &Path) {
    let gate = {
        let mut registered = READ_ALL_TEST_GATE.lock().unwrap_or_else(|error| error.into_inner());
        if registered.as_ref().is_some_and(|(target, _, _)| target == path) {
            registered.take()
        } else {
            None
        }
    };
    if let Some((_, entered, release)) = gate {
        let _ = entered.send(());
        let _ = release.recv();
    }
}
/// Runs the stop outcome behavior while preserving the operation lifecycle
/// invariants.
/// # Type Parameters
/// - `T` — type used by this operation.
/// - `E` — type used by this operation.
/// # Parameters
/// - `scope` — supplies data or policy for this operation.
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
/// Runs the before native call behavior while preserving the operation
/// lifecycle invariants.
/// # Type Parameters
/// - `T` — type used by this operation.
/// # Parameters
/// - `scope` — supplies data or policy for this operation.
/// - `native_call` — supplies data or policy for this operation.
fn before_native_call<T>(scope: &OperationScope, native_call: impl FnOnce() -> T) -> Result<T, StopReport> {
    match scope.context().stop_reason() {
        Some(reason) => Err(StopReport {
            reason,
            accepted_at: None,
        }),
        None => Ok(native_call()),
    }
}
/// Runs the record local effect behavior while preserving the operation
/// lifecycle invariants.
/// # Parameters
/// - `scope` — supplies data or policy for this operation.
/// - `observer` — supplies data or policy for this operation.
/// - `modified_count` — supplies data or policy for this operation.
/// - `modified_bytes` — supplies data or policy for this operation.
/// - `publication` — supplies data or policy for this operation.
/// - `durability` — supplies data or policy for this operation.
/// - `fact` — supplies data or policy for this operation.
/// - `description` — supplies data or policy for this operation.
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
/// Runs the shrink to zero behavior while preserving the operation lifecycle
/// invariants.
/// # Parameters
/// - `permit` — supplies data or policy for this operation.
fn shrink_to_zero(permit: &Arc<Mutex<Permit>>) {
    let mut permit = match permit.lock() {
        Ok(permit) => permit,
        Err(poisoned) => poisoned.into_inner(),
    };
    let amount = permit.amount();
    permit.shrink(amount);
}

/// Runs the reserve capacity behavior while preserving the operation lifecycle
/// invariants.
/// # Parameters
/// - `output` — supplies data or policy for this operation.
/// - `additional` — supplies data or policy for this operation.
/// - `permit` — supplies data or policy for this operation.
/// - `maximum` — supplies data or policy for this operation.
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
        let amount = lease.amount();
        lease.shrink(amount);
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

    use qubit_operation::ManagerOptions;
    use qubit_operation::OperationManager;
    use qubit_operation::OperationOutcome;
    use qubit_operation::StopReason;

    use super::READ_ALL_TEST_GATE;
    use super::READ_ALL_TEST_GATE_LOCK;
    use super::ReadAllTestGateGuard;
    use super::before_native_call;
    use super::effective_read_options;

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
        let _gate = ReadAllTestGateGuard::install(path.clone(), entered_tx, release_rx);
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
        let _gate = ReadAllTestGateGuard::install(full_path.clone(), entered_tx, release_rx);
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
        let _guard = ReadAllTestGateGuard::install(target.clone(), entered_tx, release_rx);

        super::pause_read_all_test_gate(Path::new("unrelated-read"));
        assert!(
            READ_ALL_TEST_GATE
                .lock()
                .unwrap_or_else(|error| error.into_inner())
                .as_ref()
                .is_some_and(|(path, _, _)| path == &target)
        );
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
        let guard = ReadAllTestGateGuard::install(PathBuf::from("registered"), entered_tx, release_rx);
        super::pause_read_all_test_gate(Path::new("unrelated"));
        drop(guard);
        assert!(
            READ_ALL_TEST_GATE
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
