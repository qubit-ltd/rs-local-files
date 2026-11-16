// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::io::Seek;
use std::io::SeekFrom;
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
use super::super::internal::permits::Permit;
use super::super::internal::prepared_local_job::PreparedLocalJob;
use super::AsyncLocalFileSystem;
use crate::LocalDurabilityState;
use crate::LocalFactSnapshot;
use crate::LocalFileError;
use crate::LocalFileErrorKind;
use crate::LocalFileMetadata;
use crate::LocalPublicationState;
use crate::error::LocalFileOperation;
use crate::error::LocalReadLimitError;
use crate::options::LocalCopyOptions;
use crate::options::LocalCreateDirectoryOptions;
use crate::options::LocalDeleteOptions;
use crate::options::LocalReadAllOptions;
use crate::options::LocalReadOptions;
use crate::options::LocalRenameOptions;
use crate::outcome::LocalCopyFailure;
use crate::outcome::LocalCopyFailureState;
use crate::outcome::LocalCopyOutcome;
use crate::outcome::LocalCreateDirectoryOutcome;
use crate::outcome::LocalDeleteOutcome;
use crate::outcome::LocalRenameFailure;
use crate::outcome::LocalRenameFailureState;
use crate::outcome::LocalRenameOutcome;

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
            execution_path.is_relative(),
            1,
            limits,
            move |request, scope, _observer, budget, maximum| {
                let read_limit = request
                    .read
                    .max_bytes()
                    .map_or(maximum as u64, |limit| limit.min(maximum as u64));
                read_complete_cancellable(
                    &filesystem,
                    &execution_path,
                    &request.read,
                    read_limit,
                    scope,
                    &budget,
                    maximum,
                )
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
            execution_path.is_relative(),
            1,
            limits,
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
                    &scope,
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
    /// # Parameters
    /// - `manager` — operation manager that owns the admission scope.
    /// - `operation` — operation options used for admission and cancellation.
    /// - `request` — owned input retained if submission is rejected.
    /// - `limits` — per-operation resource ceilings.
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
            execution_path.is_relative(),
            1,
            limits,
            move |request, _scope, _observer, budget, maximum| {
                let requested = request.max_bytes.saturating_add(1);
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
                let target = requested;
                while bytes.len() < target {
                    let capacity_remaining = maximum.saturating_sub(bytes.capacity());
                    if capacity_remaining == 0 {
                        let mut sentinel = [0_u8; 1];
                        match reader.read(&mut sentinel) {
                            Ok(0) => {
                                if let Some(stopped) = stop_outcome(_scope) {
                                    shrink_to_zero(&budget);
                                    return stopped;
                                }
                                break;
                            }
                            Ok(_) => {
                                if let Some(stopped) = stop_outcome(_scope) {
                                    shrink_to_zero(&budget);
                                    return stopped;
                                }
                                shrink_to_zero(&budget);
                                return OperationOutcome::Failed(LocalFileError::from_io(
                                    LocalFileOperation::Read,
                                    Some(execution_path.clone()),
                                    None,
                                    std::io::Error::other("executor owned-byte limit exceeded"),
                                ));
                            }
                            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => {
                                continue;
                            }
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
                    let count = (target - bytes.len()).min(buffer.len()).min(capacity_remaining);
                    if let Err(error) = reserve_capacity(&mut bytes, count, &budget, maximum) {
                        shrink_to_zero(&budget);
                        return OperationOutcome::Failed(LocalFileError::from_io(
                            LocalFileOperation::Read,
                            Some(execution_path.clone()),
                            None,
                            error,
                        ));
                    }
                    match reader.read(&mut buffer[..count]) {
                        Ok(0) => break,
                        Ok(read) => {
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
                let complete = bytes.len() <= request.max_bytes;
                bytes.truncate(request.max_bytes);
                OperationOutcome::Succeeded(LocalReadPrefix { bytes, complete })
            },
        )
    }
    /// Seeks and reads on one opened reader; it never reopens the path.
    ///
    /// # Parameters
    /// - `manager` — operation manager that owns the admission scope.
    /// - `operation` — operation options used for admission and cancellation.
    /// - `request` — owned input retained if submission is rejected.
    /// - `limits` — per-operation resource ceilings.
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
            execution_path.is_relative(),
            1,
            limits,
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
                    let count = (target - bytes.len()).min(buffer.len());
                    if let Err(error) = reserve_capacity(&mut bytes, count, &budget, maximum) {
                        shrink_to_zero(&budget);
                        return OperationOutcome::Failed(LocalFileError::from_io(
                            LocalFileOperation::Read,
                            Some(execution_path.clone()),
                            None,
                            error,
                        ));
                    }
                    match reader.read(&mut buffer[..count]) {
                        Ok(0) => break,
                        Ok(read) => {
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
            execution_path.is_relative(),
            1,
            limits,
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
            source.is_relative() || destination.is_relative(),
            required_handles,
            limits,
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
                        record_local_effect(
                            scope,
                            &observer,
                            stats.files().saturating_add(stats.directories()),
                            stats.bytes(),
                            LocalPublicationState::Published,
                            if outcome.durable() {
                                LocalDurabilityState::Durable
                            } else {
                                LocalDurabilityState::Unknown
                            },
                            EffectFact::Known,
                            "local copy published destination",
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
            source.is_relative() || destination.is_relative(),
            2,
            limits,
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
            path.is_relative(),
            1,
            limits,
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
            path.is_relative(),
            1,
            limits,
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
        path_is_relative: bool,
        required_handles: usize,
        limits: LocalOperationOptions,
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
            path_is_relative,
            required_handles,
            limits,
            0,
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
        path_is_relative: bool,
        required_handles: usize,
        limits: LocalOperationOptions,
        initial_owned_bytes: usize,
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
    path: &PathBuf,
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
    let reader_result = match before_native_call(scope, || match (options.read_options(), options.symlink_policy()) {
        (None, None) => filesystem.open_reader(path),
        (read_options, policy) => {
            let mut read_options = read_options.copied().unwrap_or_default();
            if let Some(policy) = policy {
                read_options = read_options.with_symlink_policy(policy);
            }
            filesystem.open_reader_with_options(path, &read_options)
        }
    }) {
        Ok(result) => result,
        Err(stop) => return OperationOutcome::Stopped(stop),
    };
    let reader = match reader_result {
        Ok(reader) => reader,
        Err(error) => return OperationOutcome::Failed(error),
    };
    let mut observed = match LocalObservedReader::from_reader(reader, LocalObservationRequirement::Required) {
        Ok(reader) => reader.bind_error_context(path.clone(), None),
        Err(error) => return OperationOutcome::Failed(error),
    };
    let before = observed.observation().clone();
    let observed_len = before.metadata().len();
    if observed_len > maximum {
        return OperationOutcome::Failed(LocalFileError::from_read_limit(
            LocalFileOperation::Read,
            Some(path.clone()),
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
                        Some(path.clone()),
                        LocalReadLimitError::new(maximum, maximum.saturating_add(1)),
                    ));
                }
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(error) => {
                    return OperationOutcome::Failed(LocalFileError::from_io(
                        LocalFileOperation::Read,
                        Some(path.clone()),
                        None,
                        error,
                    ));
                }
            }
        }
        let remaining = (maximum - bytes.len() as u64).min(buffer.len() as u64) as usize;
        if let Err(error) = reserve_capacity(&mut bytes, remaining, budget, allocation_limit) {
            return OperationOutcome::Failed(LocalFileError::from_io(
                LocalFileOperation::Read,
                Some(path.clone()),
                None,
                error,
            ));
        }
        match observed.read(&mut buffer[..remaining]) {
            Ok(0) => break,
            Ok(read) => {
                bytes.extend_from_slice(&buffer[..read]);
                if let Some(stopped) = stop_outcome(scope) {
                    return stopped;
                }
            }
            Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
            Err(error) => {
                return OperationOutcome::Failed(LocalFileError::from_io(
                    LocalFileOperation::Read,
                    Some(path.clone()),
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
                .with_path(path.clone()),
        );
    }
    if let Some(stopped) = stop_outcome(scope) {
        return stopped;
    }
    OperationOutcome::Succeeded(bytes)
}

/// Runs the stop outcome behavior while preserving the operation lifecycle
/// invariants.

/// # Type Parameters
/// - `T` — type used by this operation.
/// - `E` — type used by this operation.

/// # Parameters
/// - `scope` — supplies data or policy for this operation.
fn stop_outcome<T, E>(scope: &OperationScope) -> Option<OperationOutcome<T, E>> {
    scope.context().stop_reason().map(|reason| {
        OperationOutcome::Stopped(StopReport {
            reason,
            accepted_at: None,
        })
    })
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
    let mut permit = permit.lock().unwrap_or_else(|e| e.into_inner());
    let amount = permit.amount();
    permit.shrink(amount);
}

#[cfg(test)]
mod stop_gate_tests {
    use std::sync::Arc;
    use std::sync::atomic::AtomicUsize;
    use std::sync::atomic::Ordering;
    use std::sync::mpsc;

    use qubit_operation::ManagerOptions;
    use qubit_operation::OperationManager;
    use qubit_operation::OperationOutcome;
    use qubit_operation::StopReason;

    use super::before_native_call;

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
) -> std::io::Result<()> {
    let old_capacity = output.capacity();
    let requested = output
        .len()
        .checked_add(additional)
        .ok_or_else(|| std::io::Error::other("read capacity overflow"))?;
    let target = requested.max(old_capacity);
    let delta = target - old_capacity;
    let mut lease = permit.lock().unwrap_or_else(|e| e.into_inner());
    if target > maximum || !lease.grow(delta) {
        return Err(std::io::Error::other("executor owned-byte limit exceeded"));
    }
    if let Err(error) = output.try_reserve_exact(additional) {
        lease.shrink(delta);
        return Err(error.into());
    }
    let actual = output.capacity().saturating_sub(old_capacity);
    if actual < delta {
        lease.shrink(delta - actual);
    } else if actual > delta && !lease.grow(actual - delta) {
        let amount = lease.amount();
        lease.shrink(amount);
        return Err(std::io::Error::other(
            "allocator capacity exceeds executor owned-byte limit",
        ));
    }
    Ok(())
}
