// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Windows conditional publication backend lifecycle and native operations.

use std::ffi::OsStr;
use std::ffi::OsString;
use std::fs::File;
use std::io;
use std::io::Read;
use std::io::Seek;
use std::io::Write;
use std::path::Path;
use std::path::PathBuf;

use qubit_io::Output;

use super::super::ConditionalBackend;
use super::super::ConditionalEntry;
use super::super::ConditionalHookAction;
use super::super::ConditionalHooks;
use super::super::ConditionalPoint;
use super::super::NativeAttempt;
use super::combine_observation_error;
use super::install::install_absent;
use super::install::replace_with_backup;
use super::namespace::FileIdentity;
use super::namespace::Namespace;
use super::namespace::file_identity;
use super::observe_optional_entry;
use super::observe_with_error;
use super::open_verified_candidate_entry;
use super::open_verified_private_entry;
use super::private_entry_guard::PrivateEntryGuard;
use super::receipt_publication;
use super::recovery::classify_replace_failure;
use super::replace_with_backup_publication_method;
use crate::options::LocalWriteMetadataPolicy;
use crate::options::LocalWriteOptions;
use crate::outcome::LocalConditionalPublicationState;
use crate::recovery::LocalRecoveryFileRole;

const COPY_BUFFER_SIZE: usize = 64 * 1024;

/// Stages candidate bytes and performs identity-checked Windows publication.
///
/// The backend retains recovery entries when native operations leave a partial
/// namespace change, allowing the shared commit protocol to report that state.
#[derive(Debug)]
pub(crate) struct WindowsConditionalBackend {
    /// The namespace that constrains all candidate and recovery names.
    pub(super) namespace: Namespace,
    /// The private namespace name currently bound to the staged candidate.
    pub(super) candidate_name: OsString,
    /// The writable handle for candidate bytes until publication begins.
    candidate_file: Option<File>,
    /// The verified candidate entry used for identity checks and cleanup.
    candidate_entry: ConditionalEntry,
    /// The stable identity expected for the staged candidate.
    candidate_identity: FileIdentity,
    /// The optional private copy name and identity retained for verification.
    candidate_copy: Option<(OsString, FileIdentity)>,
    /// Whether Drop still owns cleanup responsibility for the candidate.
    candidate_guard: bool,
    /// The policies controlling this conditional publication attempt.
    options: LocalWriteOptions,
    /// Deterministic actions applied at native operation checkpoints.
    hooks: ConditionalHooks,
    /// Verified entries retained for recovery after failed cleanup.
    recovery_entries: Vec<ConditionalEntry>,
    /// Private containers created to hold displaced entries.
    recovery_containers: Vec<PathBuf>,
    /// Cleanup failures retained for the caller after backend completion.
    cleanup_errors: Vec<crate::LocalFileError>,
}

impl WindowsConditionalBackend {
    /// Opens a host namespace and stages a verified candidate for publication.
    ///
    /// # Parameters
    ///
    /// * `path` - Destination path whose namespace will be used for
    ///   publication.
    /// * `options` - Policies applied to the conditional write attempt.
    ///
    /// # Returns
    ///
    /// The backend that owns a verified private candidate entry.
    ///
    /// # Errors
    ///
    /// Returns an I/O error if the namespace or private candidate cannot be
    /// opened.
    pub(crate) fn open_host(path: &Path, options: &LocalWriteOptions) -> io::Result<Self> {
        Self::open(Namespace::open_host(path, options)?, options)
    }

    /// Opens a rooted namespace and stages a verified candidate within it.
    ///
    /// # Parameters
    ///
    /// * `root` - Open directory handle that anchors namespace access.
    /// * `relative` - Destination path relative to `root`.
    /// * `namespace_absolute` - Absolute namespace path used for identity
    ///   reporting.
    /// * `diagnostic` - Optional path retained for diagnostic context.
    /// * `options` - Policies applied to the conditional write attempt.
    ///
    /// # Returns
    ///
    /// The backend that owns a verified private candidate entry.
    ///
    /// # Errors
    ///
    /// Returns an I/O error if the rooted namespace or candidate cannot be
    /// opened.
    pub(crate) fn open_rooted(
        root: File,
        relative: &Path,
        namespace_absolute: &Path,
        diagnostic: Option<&Path>,
        options: &LocalWriteOptions,
    ) -> io::Result<Self> {
        Self::open(
            Namespace::open_rooted(root, relative, namespace_absolute, diagnostic, options)?,
            options,
        )
    }

    /// Creates and verifies the private candidate entry owned by this backend.
    ///
    /// # Parameters
    ///
    /// * `namespace` - Constrained namespace used to create the candidate.
    /// * `options` - Policies retained for this publication attempt.
    ///
    /// # Returns
    ///
    /// A backend owning the verified private candidate entry.
    ///
    /// # Errors
    ///
    /// Returns an I/O error if candidate creation or identity verification
    /// fails.
    fn open(namespace: Namespace, options: &LocalWriteOptions) -> io::Result<Self> {
        let (candidate_name, mut candidate_file) = namespace.create_private("candidate")?;
        let (candidate_entry, candidate_identity) =
            open_verified_candidate_entry(&namespace, &candidate_name, &mut candidate_file)?;
        Ok(Self {
            namespace,
            candidate_name,
            candidate_file: Some(candidate_file),
            candidate_entry,
            candidate_identity,
            candidate_copy: None,
            candidate_guard: true,
            options: *options,
            hooks: ConditionalHooks::default(),
            recovery_entries: Vec::new(),
            recovery_containers: Vec::new(),
            cleanup_errors: Vec::new(),
        })
    }

    /// Builds a receipt after an operation fails before publication.
    ///
    /// # Parameters
    ///
    /// * `error` - Error returned by the operation that failed.
    ///
    /// # Returns
    ///
    /// A receipt describing the candidate and any observation failure.
    pub(super) fn candidate_receipt(&self, error: io::Error) -> NativeAttempt {
        let (candidate, observation_error) = observe_with_error(self.observe_candidate_entry());
        let observation_failed = observation_error.is_some();
        NativeAttempt {
            publication_method: replace_with_backup_publication_method(),
            error: combine_observation_error(Some(error), observation_error),
            publication: if observation_failed {
                LocalConditionalPublicationState::Indeterminate
            } else {
                LocalConditionalPublicationState::NotPublished
            },
            installed: None,
            displaced: candidate.into_iter().collect(),
        }
    }

    /// Classifies native publication results from reopened namespace entries.
    ///
    /// # Parameters
    ///
    /// * `error` - Native operation error, if one occurred.
    /// * `backup_container` - Recovery container used for the backup, if any.
    /// * `backup_path` - Backup name within the recovery container, if any.
    ///
    /// # Returns
    ///
    /// A receipt classified from reopened namespace entries.
    pub(super) fn collect_native_receipt(
        &self,
        error: Option<io::Error>,
        backup_container: Option<&Path>,
        backup_path: Option<&Path>,
    ) -> NativeAttempt {
        // Classify documented partial results for diagnostics, but base the
        // receipt on names reopened after the native call.
        let _failure_class = error
            .as_ref()
            .map(|source| classify_replace_failure(source.raw_os_error()));
        let (installed, mut observation_error) = observe_with_error(
            self.entry_if_present(LocalRecoveryFileRole::StagedCandidate, self.namespace.target_name()),
        );
        let (backup, backup_error) = match backup_container.zip(backup_path) {
            Some((container, path)) => observe_with_error(observe_optional_entry(|| {
                self.namespace
                    .entry_in_container(LocalRecoveryFileRole::Displaced, container, path)
            })),
            None => (None, None),
        };
        let observation_error = observation_error.take().or(backup_error);
        let (candidate, candidate_error) = observe_with_error(self.observe_candidate_entry());
        let observation_error = observation_error.or(candidate_error);
        let (installed_is_candidate, identity_error) = self.observe_candidate_identity(installed.as_ref());
        let observation_error = observation_error.or(identity_error);
        let publication = receipt_publication(
            installed_is_candidate,
            candidate.is_some(),
            backup.is_some(),
            observation_error.is_some(),
        );
        let mut displaced = Vec::new();
        if let Some(backup) = backup {
            displaced.push(backup);
        }
        if let Some(candidate) = candidate
            && installed_is_candidate != Some(true)
        {
            displaced.push(candidate);
        }
        NativeAttempt {
            publication_method: if backup_container.is_some() {
                replace_with_backup_publication_method()
            } else {
                crate::outcome::LocalWritePublicationMethod::AtomicRename
            },
            error: combine_observation_error(error, observation_error),
            publication,
            installed,
            displaced,
        }
    }

    /// Classifies rollback results from the restored target and backup entries.
    ///
    /// # Parameters
    ///
    /// * `error` - Rollback operation error, if one occurred.
    /// * `expected_restored` - Identity expected at the destination after
    ///   restoration.
    /// * `backup_paths` - Recovery container and backup name, when created.
    ///
    /// # Returns
    ///
    /// A receipt classified from the restored destination and backup entries.
    pub(super) fn collect_rollback_receipt(
        &self,
        error: Option<io::Error>,
        expected_restored: Option<FileIdentity>,
        backup_paths: Option<(&Path, &Path)>,
    ) -> NativeAttempt {
        let _failure_class = error
            .as_ref()
            .map(|source| classify_replace_failure(source.raw_os_error()));
        let (installed, mut observation_error) =
            observe_with_error(self.entry_if_present(LocalRecoveryFileRole::Displaced, self.namespace.target_name()));
        let (backup, backup_error) = match backup_paths {
            Some((container, path)) => observe_with_error(observe_optional_entry(|| {
                self.namespace
                    .entry_in_container(LocalRecoveryFileRole::RollbackDisplaced, container, path)
            })),
            None => (None, None),
        };
        observation_error = observation_error.or(backup_error);
        let (installed_identity, identity_error) = self.observe_entry_identity(installed.as_ref());
        observation_error = observation_error.or(identity_error);
        // A failed reverse operation cannot undo the earlier publication fact.
        // Only a verified restoration or the candidate's current target identity
        // establishes a definite state; other observations remain indeterminate.
        let publication = if installed_identity == Some(self.candidate_identity) {
            LocalConditionalPublicationState::Published
        } else if observation_error.is_none() && expected_restored.is_some() && installed_identity == expected_restored
        {
            LocalConditionalPublicationState::Restored
        } else {
            LocalConditionalPublicationState::Indeterminate
        };
        NativeAttempt {
            publication_method: replace_with_backup_publication_method(),
            error: combine_observation_error(error, observation_error),
            publication,
            installed,
            displaced: backup.into_iter().collect(),
        }
    }

    /// Reopens a named recovery entry through the constrained namespace.
    ///
    /// # Parameters
    ///
    /// * `role` - Recovery role assigned to the entry.
    /// * `name` - Namespace name to reopen.
    ///
    /// # Errors
    ///
    /// Returns an I/O error if the entry cannot be opened or verified.
    fn entry(&self, role: LocalRecoveryFileRole, name: OsString) -> io::Result<ConditionalEntry> {
        self.namespace.entry(role, name)
    }

    /// Reopens a named entry, treating only a missing name as absence.
    ///
    /// # Parameters
    ///
    /// * `role` - Recovery role assigned to the entry.
    /// * `name` - Borrowed namespace name to reopen.
    ///
    /// # Returns
    ///
    /// `None` only when the named entry is absent; other observation failures
    /// are errors.
    ///
    /// # Errors
    ///
    /// Returns an I/O error when reopening fails for a reason other than
    /// absence.
    fn entry_if_present(&self, role: LocalRecoveryFileRole, name: &OsStr) -> io::Result<Option<ConditionalEntry>> {
        observe_optional_entry(|| self.entry(role, name.to_os_string()))
    }

    /// Reopens the candidate name and rejects an identity change.
    ///
    /// # Returns
    ///
    /// The verified candidate entry, or `None` when its name is absent.
    ///
    /// # Errors
    ///
    /// Returns an I/O error if reopening fails or the candidate identity
    /// changed.
    fn observe_candidate_entry(&self) -> io::Result<Option<ConditionalEntry>> {
        let candidate = self.entry_if_present(LocalRecoveryFileRole::StagedCandidate, &self.candidate_name)?;
        let Some(candidate) = candidate else {
            return Ok(None);
        };
        if !self.is_candidate(&candidate)? {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "conditional candidate name refers to a different file identity",
            ));
        }
        Ok(Some(candidate))
    }

    /// Compares an installed entry with the staged candidate identity.
    ///
    /// # Parameters
    ///
    /// * `installed` - Optional reopened destination entry.
    ///
    /// # Returns
    ///
    /// The identity comparison when observable, plus any observation error.
    fn observe_candidate_identity(&self, installed: Option<&ConditionalEntry>) -> (Option<bool>, Option<io::Error>) {
        match installed {
            Some(entry) => match entry
                .open_reader()
                .map_err(io::Error::other)
                .and_then(|reader| file_identity(reader.as_file()))
            {
                Ok(identity) => (Some(identity == self.candidate_identity), None),
                Err(error) => (None, Some(error)),
            },
            None => (Some(false), None),
        }
    }

    /// Reads the stable identity of an observed entry when it is present.
    ///
    /// # Parameters
    ///
    /// * `entry` - Optional verified entry whose identity should be read.
    ///
    /// # Returns
    ///
    /// The observed identity when present, plus any observation error.
    fn observe_entry_identity(&self, entry: Option<&ConditionalEntry>) -> (Option<FileIdentity>, Option<io::Error>) {
        match entry {
            Some(entry) => match entry
                .open_reader()
                .map_err(io::Error::other)
                .and_then(|reader| file_identity(reader.as_file()))
            {
                Ok(identity) => (Some(identity), None),
                Err(error) => (None, Some(error)),
            },
            None => (None, None),
        }
    }

    /// Creates and records a private recovery container and backup name.
    ///
    /// # Returns
    ///
    /// The recovery container and the backup name within it.
    ///
    /// # Errors
    ///
    /// Returns an I/O error if creating the recovery container fails.
    pub(super) fn make_backup(&mut self) -> io::Result<(PathBuf, PathBuf)> {
        let (container, backup) = self.namespace.backup_container()?;
        self.recovery_containers.push(container.clone());
        Ok((container, backup))
    }

    /// Runs the configured action at a native-operation checkpoint.
    ///
    /// # Parameters
    ///
    /// * `point` - Checkpoint whose configured action should run.
    ///
    /// # Errors
    ///
    /// Returns an injected or callback error when the action fails.
    fn checkpoint_action(&mut self, point: ConditionalPoint) -> io::Result<()> {
        #[cfg(all(feature = "digest", feature = "test-support"))]
        self.hooks.run_callback(point)?;
        match self.hooks.take(point) {
            Some(ConditionalHookAction::Fail(kind)) => {
                Err(io::Error::new(kind, "injected Windows conditional failure"))
            }
            Some(ConditionalHookAction::ReplaceTarget(bytes)) => self.write_target_for_hook(&bytes, false),
            Some(ConditionalHookAction::OverwriteTargetInPlace(bytes)) => self.write_target_for_hook(&bytes, true),
            Some(ConditionalHookAction::PartialCandidateCopy(_))
            | Some(ConditionalHookAction::CorruptCandidateCopy(_))
            | Some(ConditionalHookAction::PartialPublish)
            | Some(ConditionalHookAction::Noop)
            | None => Ok(()),
        }
    }

    /// Applies a test hook that creates or overwrites the target.
    ///
    /// # Parameters
    ///
    /// * `bytes` - Contents written by the hook.
    /// * `in_place` - Whether to overwrite an existing destination.
    ///
    /// # Errors
    ///
    /// Returns an I/O error if opening or writing the destination fails.
    fn write_target_for_hook(&self, bytes: &[u8], in_place: bool) -> io::Result<()> {
        let path = self.namespace.target_path();
        if in_place {
            let mut file = std::fs::OpenOptions::new().write(true).truncate(true).open(path)?;
            file.write_all(bytes)
        } else {
            let mut file = std::fs::OpenOptions::new().write(true).create_new(true).open(path)?;
            file.write_all(bytes)
        }
    }

    /// Publishes host content using ReplaceFileW and verifies its receipt.
    ///
    /// # Returns
    ///
    /// The publication receipt, including any native or observation error.
    fn publish_host_content(&mut self) -> NativeAttempt {
        if self.options.metadata_policy() != LocalWriteMetadataPolicy::PreserveExisting {
            return self.candidate_receipt(io::Error::new(
                io::ErrorKind::Unsupported,
                "Windows conditional Content requires PreserveExisting metadata",
            ));
        }
        let (container, backup) = match self.make_backup() {
            Ok(paths) => paths,
            Err(error) => {
                return self.candidate_receipt(error);
            }
        };
        let candidate_path = match self.namespace.private_file_path(&self.candidate_name) {
            Ok(path) => path,
            Err(error) => {
                let attempt = self.collect_native_receipt(Some(error), Some(&container), Some(&backup));
                return self.cleanup_empty_container(container, backup, attempt);
            }
        };
        let attempt = match replace_with_backup(self.namespace.target_path(), &candidate_path, &backup) {
            Ok(()) => {
                let mut attempt = self.collect_native_receipt(None, Some(&container), Some(&backup));
                if attempt.publication != LocalConditionalPublicationState::Published || attempt.displaced.is_empty() {
                    attempt.error = Some(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "ReplaceFileW succeeded without a verified target and backup receipt",
                    ));
                    attempt.publication = LocalConditionalPublicationState::Indeterminate;
                }
                attempt
            }
            Err(error) => self.collect_native_receipt(Some(error), Some(&container), Some(&backup)),
        };
        self.cleanup_empty_container(container, backup, attempt)
    }

    /// Removes an unused recovery container and records cleanup failure.
    ///
    /// # Parameters
    ///
    /// * `container` - Recovery container to remove when unused.
    /// * `backup` - Backup path whose retention prevents container removal.
    /// * `attempt` - Publication receipt augmented with cleanup outcome.
    ///
    /// # Returns
    ///
    /// The receipt after any eligible recovery-container cleanup.
    pub(super) fn cleanup_empty_container(
        &mut self,
        container: PathBuf,
        backup: PathBuf,
        mut attempt: NativeAttempt,
    ) -> NativeAttempt {
        if attempt.displaced.iter().any(|entry| entry.namespace_path() == backup) {
            return attempt;
        }
        match self.namespace.remove_container(&container) {
            Ok(()) => self.recovery_containers.retain(|path| path != &container),
            Err(cleanup) => {
                let error = io::Error::new(
                    cleanup.kind(),
                    format!(
                        "empty recovery container cleanup failed at {}: {}",
                        container.display(),
                        cleanup
                    ),
                );
                attempt.error = combine_observation_error(attempt.error, Some(error));
            }
        }
        attempt
    }

    /// Transfers the original candidate into recovery ownership.
    fn retain_original_candidate(&mut self) {
        let Some(file) = self.candidate_file.as_ref() else {
            return;
        };
        if let Ok(entry) = self.namespace.entry_bound_to(
            LocalRecoveryFileRole::StagedCandidate,
            self.candidate_name.clone(),
            file,
        ) {
            self.recovery_entries.push(entry);
            self.candidate_guard = false;
            self.candidate_file.take();
        }
    }

    /// Classifies a successful rollback syscall from its verified namespace
    /// receipt.
    ///
    /// # Parameters
    ///
    /// * `expected_restored` - Identity expected for the restored destination
    ///   entry.
    /// * `hook_error` - Error reported by the rollback checkpoint hook, if any.
    /// * `container` - Recovery container holding the rollback backup.
    /// * `backup` - Name of the rollback backup within the recovery container.
    ///
    /// # Returns
    ///
    /// A receipt describing whether restoration was verified.
    fn collect_successful_rollback_receipt(
        &self,
        expected_restored: FileIdentity,
        hook_error: Option<io::Error>,
        container: &Path,
        backup: &Path,
    ) -> NativeAttempt {
        let (installed, installed_error) =
            observe_with_error(self.entry_if_present(LocalRecoveryFileRole::Displaced, self.namespace.target_name()));
        let (replacement_displaced, displaced_error) = observe_with_error(observe_optional_entry(|| {
            self.namespace
                .entry_in_container(LocalRecoveryFileRole::RollbackDisplaced, container, backup)
        }));
        let (restored_identity, identity_error) = match installed.as_ref() {
            Some(entry) => match entry
                .open_reader()
                .map_err(io::Error::other)
                .and_then(|reader| file_identity(reader.as_file()))
            {
                Ok(identity) => (Some(identity), None),
                Err(error) => (None, Some(error)),
            },
            None => (None, None),
        };
        let observation_error = hook_error.or(installed_error).or(displaced_error).or(identity_error);
        let complete = observation_error.is_none()
            && restored_identity == Some(expected_restored)
            && replacement_displaced.is_some();
        NativeAttempt {
            publication_method: replace_with_backup_publication_method(),
            error: observation_error.or_else(|| {
                (!complete).then(|| {
                    io::Error::new(
                        io::ErrorKind::InvalidData,
                        "ReplaceFileW rollback succeeded without a verified restoration receipt",
                    )
                })
            }),
            publication: if complete {
                LocalConditionalPublicationState::Restored
            } else {
                LocalConditionalPublicationState::Indeterminate
            },
            installed,
            displaced: replacement_displaced.into_iter().collect(),
        }
    }

    /// Observes the names and cleans up the candidate after an absent install.
    ///
    /// # Returns
    ///
    /// The receipt classified from the observed names and cleanup result.
    fn collect_absent_receipt(&self) -> NativeAttempt {
        let (installed, installed_error) = observe_with_error(
            self.entry_if_present(LocalRecoveryFileRole::StagedCandidate, self.namespace.target_name()),
        );
        let (candidate, candidate_error) = if !self.namespace.is_rooted() {
            observe_with_error(self.observe_candidate_entry())
        } else {
            (None, None)
        };
        let observation_error = installed_error.or(candidate_error);
        let observation_failed = observation_error.is_some();
        if let Some(candidate) = candidate
            && let Err(error) = self.namespace.remove(&candidate)
        {
            return NativeAttempt {
                publication_method: crate::outcome::LocalWritePublicationMethod::AtomicRename,
                error: combine_observation_error(Some(error), observation_error),
                publication: if observation_failed {
                    LocalConditionalPublicationState::Indeterminate
                } else {
                    LocalConditionalPublicationState::Published
                },
                installed,
                displaced: vec![candidate],
            };
        }
        let publication = if observation_failed {
            LocalConditionalPublicationState::Indeterminate
        } else if installed.is_some() {
            LocalConditionalPublicationState::Published
        } else {
            LocalConditionalPublicationState::Indeterminate
        };
        NativeAttempt {
            publication_method: crate::outcome::LocalWritePublicationMethod::AtomicRename,
            error: observation_error,
            publication,
            installed,
            displaced: Vec::new(),
        }
    }
}

impl Output for WindowsConditionalBackend {
    /// Byte type accepted by the staged output writer.
    type Item = u8;

    /// Writes vectored candidate bytes to the staged file.
    ///
    /// # Parameters
    ///
    /// * `slices` - Byte slices submitted in order as one vectored operation.
    ///
    /// # Returns
    ///
    /// The number of bytes accepted, which may be less than the total input.
    ///
    /// # Errors
    ///
    /// Returns an I/O error if the candidate handle is closed or the write
    /// fails.
    fn write_vectored(&mut self, slices: &[&[u8]]) -> io::Result<usize> {
        let file = self
            .candidate_file
            .as_mut()
            .ok_or_else(|| io::Error::new(io::ErrorKind::BrokenPipe, "conditional candidate is closed"))?;
        let slices: Vec<io::IoSlice<'_>> = slices.iter().map(|slice| io::IoSlice::new(slice)).collect();
        Write::write_vectored(file, &slices)
    }

    /// Writes the requested candidate byte range without additional bounds
    /// checks.
    ///
    /// # Safety
    /// The caller must ensure `index..index + count` is within `input` and that
    /// the addition does not overflow.
    ///
    /// # Parameters
    ///
    /// * `input` - Source bytes.
    /// * `index` - Start of the requested range.
    /// * `count` - Length of the requested range.
    ///
    /// # Returns
    ///
    /// The number of requested bytes accepted by the staged file; writes may be
    /// short.
    ///
    /// # Errors
    ///
    /// Returns an I/O error if the candidate handle is closed or the write
    /// fails.
    unsafe fn write_unchecked(&mut self, input: &[u8], index: usize, count: usize) -> io::Result<usize> {
        let file = self
            .candidate_file
            .as_mut()
            .ok_or_else(|| io::Error::new(io::ErrorKind::BrokenPipe, "conditional candidate is closed"))?;
        Write::write(file, &input[index..index + count])
    }

    /// Flushes buffered candidate bytes to the staged file.
    ///
    /// # Returns
    ///
    /// `Ok(())` when the file handle has no pending buffered bytes.
    ///
    /// # Errors
    ///
    /// Returns an I/O error if the candidate handle is closed or flushing
    /// fails.
    fn flush(&mut self) -> io::Result<()> {
        Write::flush(
            self.candidate_file
                .as_mut()
                .ok_or_else(|| io::Error::new(io::ErrorKind::BrokenPipe, "conditional candidate is closed"))?,
        )
    }
}

impl ConditionalBackend for WindowsConditionalBackend {
    /// Returns the verified entry that contains the staged candidate.
    ///
    /// # Returns
    ///
    /// The entry that currently owns the staged candidate.
    #[inline]
    fn candidate(&self) -> &ConditionalEntry {
        &self.candidate_entry
    }

    /// Copies the staged candidate into a separately named private entry.
    ///
    /// The guard retains cleanup responsibility until the copy has been
    /// flushed, identified, and reopened as a verified recovery entry. If any
    /// step fails, the guard attempts identity-checked cleanup and the backend
    /// keeps its original candidate available. Each write uses only the
    /// initialized prefix returned by the corresponding read.
    ///
    /// # Returns
    ///
    /// The independently verified candidate copy.
    ///
    /// # Errors
    ///
    /// Returns the I/O error encountered while copying or verifying the entry.
    fn create_candidate_copy(&mut self) -> io::Result<ConditionalEntry> {
        let (name, mut destination) = match self.namespace.create_private("candidate-copy") {
            Ok(created) => created,
            Err(error) => {
                self.retain_original_candidate();
                return Err(error);
            }
        };
        let mut guard = match PrivateEntryGuard::new(&self.namespace, &name, &mut destination) {
            Ok(guard) => guard,
            Err((primary, mut guard)) => return Err(guard.finish_with(primary)),
        };
        let result = (|| {
            let source = self
                .candidate_file
                .as_ref()
                .ok_or_else(|| io::Error::new(io::ErrorKind::BrokenPipe, "conditional candidate is closed"))?;
            let mut source = source.try_clone()?;
            source.rewind()?;
            let mut buffer = [0_u8; COPY_BUFFER_SIZE];
            loop {
                let read = source.read(&mut buffer)?;
                if read == 0 {
                    break;
                }
                guard.file_mut().write_all(&buffer[..read])?;
            }
            Write::flush(guard.file_mut())?;
            let identity = file_identity(guard.file_mut())?;
            let entry = open_verified_private_entry(
                &self.namespace,
                LocalRecoveryFileRole::StagedCandidate,
                &name,
                guard.file_mut(),
                identity,
            )?;
            Ok((entry, identity))
        })();
        match result {
            Ok((entry, identity)) => {
                guard.disarm();
                drop(guard);
                self.candidate_copy = Some((name, identity));
                Ok(entry)
            }
            Err(error) => {
                let error = guard.finish_with(error);
                drop(guard);
                self.retain_original_candidate();
                Err(error)
            }
        }
    }

    /// Opens the current destination as a displaced recovery entry.
    ///
    /// # Returns
    ///
    /// The verified entry currently bound to the destination name.
    ///
    /// # Errors
    ///
    /// Returns an I/O error if the destination cannot be opened or verified.
    fn open_target(&self) -> io::Result<ConditionalEntry> {
        let entry = self.entry(
            LocalRecoveryFileRole::Displaced,
            self.namespace.target_name().to_os_string(),
        )?;
        Ok(entry)
    }

    /// Verifies the observed destination before replacement preserves its
    /// metadata.
    ///
    /// # Parameters
    ///
    /// * `observed` - Destination entry whose namespace identity is
    ///   revalidated.
    ///
    /// # Errors
    ///
    /// Returns an I/O error if the observed entry no longer verifies.
    fn preserve_metadata(&mut self, observed: &ConditionalEntry) -> io::Result<()> {
        self.namespace.verify_entry(observed)?;
        // ReplaceFileW carries the existing destination's attributes and ACL
        // onto the replacement. Applying read-only permissions to the staged
        // file here can itself prevent ReplaceFileW from replacing it.
        Ok(())
    }

    /// Synchronizes staged candidate data to the underlying file.
    ///
    /// # Errors
    ///
    /// Returns an I/O error if the candidate handle is closed or
    /// synchronization fails.
    fn sync_candidate(&self) -> io::Result<()> {
        self.candidate_file
            .as_ref()
            .ok_or_else(|| io::Error::new(io::ErrorKind::BrokenPipe, "conditional candidate is closed"))?
            .sync_all()
    }

    /// Synchronizes data for a verified namespace entry.
    ///
    /// # Parameters
    ///
    /// * `entry` - Verified entry whose file data should be synchronized.
    ///
    /// # Errors
    ///
    /// Returns an I/O error if synchronizing the entry fails.
    #[inline]
    fn sync_entry(&self, entry: &ConditionalEntry) -> io::Result<()> {
        entry.sync_file()
    }

    /// Reports whether the requested namespace durability is supported.
    ///
    /// # Errors
    ///
    /// Returns `Unsupported` when namespace durability is required.
    fn sync_namespace(&self) -> io::Result<()> {
        if self.options.durability() == crate::policy::LocalDurabilityRequirement::NotRequired {
            return Ok(());
        }
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "Windows directory durability is not guaranteed by the conditional backend",
        ))
    }

    /// Checks whether an entry refers to this backend candidate.
    ///
    /// # Parameters
    ///
    /// * `entry` - Verified entry to compare with the candidate identity.
    ///
    /// # Returns
    ///
    /// Whether the entry identifies the staged candidate.
    ///
    /// # Errors
    ///
    /// Returns an I/O error if validation or identity observation fails.
    fn is_candidate(&self, entry: &ConditionalEntry) -> io::Result<bool> {
        self.verify_entry(entry)?;
        let reader = entry.open_reader().map_err(io::Error::other)?;
        Ok(file_identity(reader.as_file())? == self.candidate_identity)
    }

    /// Validates the entry and any retained candidate-copy identity.
    ///
    /// # Parameters
    ///
    /// * `entry` - Entry whose namespace membership and identity are checked.
    ///
    /// # Errors
    ///
    /// Returns an I/O error if the entry or retained copy has changed.
    fn verify_entry(&self, entry: &ConditionalEntry) -> io::Result<()> {
        self.namespace.verify_entry(entry)?;
        if let Some((name, expected)) = &self.candidate_copy {
            match self.namespace.identity_at(name)? {
                Some(observed) if observed == *expected => {}
                Some(_) => {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidData,
                        "conditional candidate copy name changed identity",
                    ));
                }
                None => {
                    return Err(io::Error::new(
                        io::ErrorKind::NotFound,
                        "conditional candidate copy name disappeared",
                    ));
                }
            }
        }
        Ok(())
    }

    /// Revalidates the candidate and transfers it to publication ownership.
    ///
    /// # Errors
    ///
    /// Returns an I/O error if reopening or identity verification fails.
    fn begin_publication(&mut self) -> io::Result<()> {
        let observed = self
            .namespace
            .entry(LocalRecoveryFileRole::StagedCandidate, self.candidate_name.clone())?;
        let reader = observed.open_reader().map_err(io::Error::other)?;
        if file_identity(reader.as_file())? != self.candidate_identity {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "conditional candidate changed identity before publication",
            ));
        }
        self.candidate_entry = observed;
        self.candidate_guard = false;
        self.candidate_file.take();
        Ok(())
    }

    /// Installs the candidate only when the destination is absent.
    ///
    /// # Returns
    ///
    /// The receipt classified from the installed and candidate entries.
    fn publish_absent(&mut self) -> NativeAttempt {
        match install_absent(&self.namespace, &self.candidate_name) {
            Ok(()) => self.collect_absent_receipt(),
            Err(error) => self.collect_native_receipt(Some(error), None, None),
        }
    }

    /// Publishes replacement content when the namespace supports it.
    ///
    /// # Returns
    ///
    /// The publication receipt, including any unsupported-operation error.
    fn publish_content(&mut self) -> NativeAttempt {
        if self.namespace.is_rooted() {
            return self.candidate_receipt(io::Error::new(
                io::ErrorKind::Unsupported,
                "Windows Rooted conditional Content is unsupported",
            ));
        }
        self.publish_host_content()
    }

    /// Restores a verified displaced entry and records the resulting state.
    ///
    /// # Parameters
    ///
    /// * `displaced` - Verified entry to restore as the destination.
    ///
    /// # Returns
    ///
    /// The receipt classified from the rollback attempt and namespace state.
    fn rollback(&mut self, displaced: &ConditionalEntry) -> NativeAttempt {
        if self.namespace.is_rooted() || self.options.metadata_policy() != LocalWriteMetadataPolicy::PreserveExisting {
            return self.collect_rollback_receipt(
                Some(io::Error::new(
                    io::ErrorKind::Unsupported,
                    "Windows conditional rollback is unsupported for this namespace",
                )),
                None,
                None,
            );
        }
        if let Err(error) = self.namespace.verify_entry(displaced) {
            return self.collect_rollback_receipt(Some(error), None, None);
        }
        let expected_restored = match displaced
            .open_reader()
            .map_err(io::Error::other)
            .and_then(|reader| file_identity(reader.as_file()))
        {
            Ok(identity) => identity,
            Err(error) => return self.collect_rollback_receipt(Some(error), None, None),
        };
        let displaced_path = displaced.namespace_path();
        let (container, backup) = match self.make_backup() {
            Ok(paths) => paths,
            Err(error) => return self.collect_rollback_receipt(Some(error), None, None),
        };
        if let Err(error) = self.checkpoint_action(ConditionalPoint::RollbackExchange) {
            let attempt =
                self.collect_rollback_receipt(Some(error), Some(expected_restored), Some((&container, &backup)));
            return self.cleanup_empty_container(container, backup, attempt);
        }
        match replace_with_backup(self.namespace.target_path(), displaced_path, &backup) {
            Ok(()) => {
                let hook_error = self.checkpoint_action(ConditionalPoint::AfterRollbackSyscall).err();
                let attempt =
                    self.collect_successful_rollback_receipt(expected_restored, hook_error, &container, &backup);
                self.cleanup_empty_container(container, backup, attempt)
            }
            Err(error) => {
                let hook_error = self.checkpoint_action(ConditionalPoint::AfterRollbackSyscall).err();
                let attempt = self.collect_rollback_receipt(
                    Some(error).or(hook_error),
                    Some(expected_restored),
                    Some((&container, &backup)),
                );
                self.cleanup_empty_container(container, backup, attempt)
            }
        }
    }

    /// Removes a previously verified entry and its empty recovery container.
    ///
    /// # Parameters
    ///
    /// * `entry` - Verified recovery entry to remove.
    ///
    /// # Errors
    ///
    /// Returns an I/O error if removing the entry fails.
    fn remove_observed(&mut self, entry: &ConditionalEntry) -> io::Result<()> {
        self.namespace.remove(entry)?;
        if self
            .candidate_copy
            .as_ref()
            .is_some_and(|(name, _)| entry.namespace_path().file_name() == Some(name.as_os_str()))
        {
            self.candidate_copy = None;
        }
        if let Some(parent) = entry.namespace_path().parent()
            && self.recovery_containers.iter().any(|container| container == parent)
        {
            if let Err(source) = self.namespace.remove_container(parent) {
                self.cleanup_errors.push(crate::LocalFileError::from_io(
                    crate::LocalFileOperation::Cleanup,
                    Some(parent.to_path_buf()),
                    None,
                    source,
                ));
            } else {
                self.recovery_containers.retain(|container| container != parent);
            }
        }
        Ok(())
    }

    /// Removes candidate and recovery entries still owned by this backend.
    ///
    /// # Errors
    ///
    /// Returns an I/O error if removing an owned entry fails.
    fn abort(&mut self) -> io::Result<()> {
        if !self.candidate_guard {
            return Ok(());
        }
        self.candidate_file.take();
        self.namespace.remove(&self.candidate_entry)?;
        self.candidate_guard = false;
        for entry in std::mem::take(&mut self.recovery_entries) {
            self.namespace.remove(&entry)?;
        }
        Ok(())
    }

    /// Transfers retained recovery entries to the caller.
    ///
    /// # Returns
    ///
    /// The entries transferred without resolving their names again.
    fn take_recovery_entries(&mut self) -> Vec<ConditionalEntry> {
        if !self.candidate_guard || self.candidate_copy.is_none() {
            self.retain_original_candidate();
        }
        std::mem::take(&mut self.recovery_entries)
    }

    /// Transfers recorded cleanup errors to the caller.
    ///
    /// # Returns
    ///
    /// The cleanup errors accumulated by this backend.
    fn take_cleanup_errors(&mut self) -> Vec<crate::LocalFileError> {
        std::mem::take(&mut self.cleanup_errors)
    }

    /// Replaces checkpoint actions used by deterministic tests.
    ///
    /// # Parameters
    ///
    /// * `hooks` - Actions installed for subsequent checkpoints.
    #[cfg(any(test, all(feature = "digest", feature = "test-support")))]
    fn set_hooks(&mut self, hooks: ConditionalHooks) {
        self.hooks = hooks;
    }

    /// Runs the configured action for a shared commit checkpoint.
    ///
    /// # Parameters
    ///
    /// * `point` - Shared commit checkpoint whose action should run.
    ///
    /// # Errors
    ///
    /// Returns an injected or callback error when the action fails.
    fn checkpoint(&mut self, point: ConditionalPoint) -> io::Result<()> {
        self.checkpoint_action(point)
    }
}

impl Drop for WindowsConditionalBackend {
    /// Cleans up the candidate when publication ownership was not transferred.
    fn drop(&mut self) {
        if self.candidate_guard {
            let _ = self.abort();
        }
    }
}
