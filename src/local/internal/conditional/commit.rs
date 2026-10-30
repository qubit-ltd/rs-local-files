// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::io;
use std::path::PathBuf;

use digest::Digest;
use digest::Output;

use super::ConditionalBackend;
use super::ConditionalPoint;
use super::candidate_copy::prepare_copy;
use super::cleanup::cleanup_entries;
use super::digest_reader::digest_entry;
use crate::LocalFileError;
use crate::LocalFileOperation;
use crate::LocalWriteOutcome;
use crate::LocalWriterState;
use crate::error::LocalConditionalCommitError;
use crate::error::LocalConditionalFailureReason;
use crate::error::LocalConditionalWriteStage;
use crate::options::LocalWriteOptions;
use crate::options::LocalWritePrecondition;
use crate::outcome::LocalConditionalPublicationState;
use crate::outcome::LocalConditionalWriteOutcome;
use crate::outcome::LocalWritePublicationMethod;
use crate::policy::LocalDurabilityRequirement;
use crate::policy::LocalFileSyncRequirement;
use crate::recovery::LocalRecoveryFileRole;

#[allow(clippy::result_large_err)]
/// Runs the conditional publication protocol using the backend's retained
/// namespace authority.
///
/// The candidate is flushed and digested, then copied to an independently
/// verified recovery entry before the target precondition is checked. After
/// publication, the installed entry and displaced entries are verified. A
/// detected post-publication conflict is rolled back only after rechecking the
/// installed candidate and displaced entry; uncertainty retains the observed
/// entries for recovery. A successful return means publication and required
/// synchronization completed; cleanup failures are returned alongside any
/// retained files and do not change that success. This protocol does not
/// provide a strict compare-and-swap guarantee.
#[cfg(test)]
pub(crate) fn commit_conditional<D, B>(
    backend: B,
    precondition: LocalWritePrecondition<Output<D>>,
    options: LocalWriteOptions,
    bytes_written: usize,
) -> Result<LocalConditionalWriteOutcome<D>, LocalConditionalCommitError>
where
    D: Digest,
    B: ConditionalBackend,
{
    commit_conditional_with_directory(backend, precondition, options, bytes_written, None)
}

/// Executes the commit protocol and binds fallback diagnostics to the resolver
/// snapshot captured when the public writer was opened.
#[allow(clippy::result_large_err)]
pub(crate) fn commit_conditional_with_directory<D, B>(
    mut backend: B,
    precondition: LocalWritePrecondition<Output<D>>,
    options: LocalWriteOptions,
    bytes_written: usize,
    current_directory: Option<PathBuf>,
) -> Result<LocalConditionalWriteOutcome<D>, LocalConditionalCommitError>
where
    D: Digest,
    B: ConditionalBackend,
{
    let mut entries = Vec::new();
    let mut stage = LocalConditionalWriteStage::Prepare;
    let mut publication = LocalConditionalPublicationState::NotPublished;
    let mut post_publication_conflict = false;
    let mut publication_attempt_error = None;
    let absent_precondition = matches!(&precondition, LocalWritePrecondition::Absent);
    let mut publication_method = LocalWritePublicationMethod::AtomicRename;
    let mut durable = options.durability() != LocalDurabilityRequirement::NotRequired;
    let mut file_synced = false;
    let result = (|| {
        qubit_io::Output::flush(&mut backend).map_err(|error| (stage, error))?;
        stage = LocalConditionalWriteStage::VerifyCandidate;
        let candidate_digest = digest_entry::<D>(backend.candidate()).map_err(|error| (stage, error))?;

        stage = LocalConditionalWriteStage::PrepareCandidateCopy;
        backend
            .checkpoint(ConditionalPoint::PrepareCandidateCopy)
            .map_err(|error| (stage, error))?;
        let copy = prepare_copy::<D, B>(&mut backend, &candidate_digest).map_err(|(error, entry)| {
            stage = if entry.is_some() {
                LocalConditionalWriteStage::VerifyCandidateCopy
            } else {
                LocalConditionalWriteStage::PrepareCandidateCopy
            };
            if let Some(entry) = entry {
                entries.push(entry);
            }
            (stage, error)
        })?;
        stage = LocalConditionalWriteStage::VerifyCandidateCopy;
        entries.push(copy);
        backend
            .checkpoint(ConditionalPoint::VerifyCandidateCopy)
            .map_err(|error| (stage, error))?;

        stage = LocalConditionalWriteStage::Sync;
        let sync_requirement = match (options.durability(), options.file_sync()) {
            (LocalDurabilityRequirement::Required, _) | (_, LocalFileSyncRequirement::Required) => {
                LocalFileSyncRequirement::Required
            }
            (LocalDurabilityRequirement::Preferred, _) | (_, LocalFileSyncRequirement::Preferred) => {
                LocalFileSyncRequirement::Preferred
            }
            _ => LocalFileSyncRequirement::NotRequired,
        };
        if sync_requirement != LocalFileSyncRequirement::NotRequired {
            // The native protocol installs the original candidate and retains
            // the independently copied entry for recovery. Sync both before
            // publication; installed-entry identity is verified after the
            // native operation before this result can report file_synced.
            let sync = backend
                .checkpoint(ConditionalPoint::BeforeRequiredFileSync)
                .and_then(|()| backend.sync_candidate().and_then(|()| backend.sync_entry(&entries[0])));
            if let Err(error) = sync {
                if sync_requirement == LocalFileSyncRequirement::Required {
                    return Err((stage, error));
                }
                if options.durability() == LocalDurabilityRequirement::Preferred {
                    durable = false;
                }
            }
            if options.durability() != LocalDurabilityRequirement::NotRequired
                && let Err(error) = backend.sync_namespace()
            {
                if options.durability() == LocalDurabilityRequirement::Required {
                    return Err((stage, error));
                }
                durable = false;
            }
        }

        stage = LocalConditionalWriteStage::Precheck;
        backend
            .checkpoint(ConditionalPoint::Precheck)
            .map_err(|error| (stage, error))?;
        let observed = backend.open_target();
        match (&precondition, observed) {
            (LocalWritePrecondition::Absent, Ok(_)) => {
                return Err((stage, io::Error::new(io::ErrorKind::AlreadyExists, "target exists")));
            }
            (LocalWritePrecondition::Absent, Err(error)) if error.kind() == io::ErrorKind::NotFound => {}
            (LocalWritePrecondition::Absent, Err(error)) => return Err((stage, error)),
            (LocalWritePrecondition::Content(_), Err(error)) => return Err((stage, error)),
            (LocalWritePrecondition::Content(expected), Ok(target)) => {
                if digest_entry::<D>(&target).map_err(|error| (stage, error))? != *expected {
                    return Err((
                        stage,
                        io::Error::new(io::ErrorKind::InvalidData, "content precondition mismatch"),
                    ));
                }
                if options.metadata_policy() == crate::options::LocalWriteMetadataPolicy::PreserveExisting {
                    backend.preserve_metadata(&target).map_err(|error| (stage, error))?;
                    if sync_requirement != LocalFileSyncRequirement::NotRequired
                        && let Err(error) = backend.sync_candidate()
                    {
                        if sync_requirement == LocalFileSyncRequirement::Required {
                            return Err((stage, error));
                        }
                        if options.durability() == LocalDurabilityRequirement::Preferred {
                            durable = false;
                        }
                    }
                }
                // The observed target is only a precheck/metadata source. The
                // backend returns the actual displaced object after publish.
            }
        }

        backend
            .checkpoint(ConditionalPoint::AfterPrecheck)
            .map_err(|error| (stage, error))?;

        backend
            .verify_entry(backend.candidate())
            .map_err(|error| (stage, error))?;
        stage = LocalConditionalWriteStage::Publish;
        backend
            .checkpoint(ConditionalPoint::Publish)
            .map_err(|error| (stage, error))?;
        backend.begin_publication().map_err(|error| (stage, error))?;
        let attempt = match precondition {
            LocalWritePrecondition::Absent => backend.publish_absent(),
            LocalWritePrecondition::Content(_) => backend.publish_content(),
        };
        if attempt.publication != LocalConditionalPublicationState::NotPublished {
            publication_method = attempt.publication_method;
        }
        if let Err(error) = backend.checkpoint(ConditionalPoint::AfterPublish) {
            publication = attempt.publication;
            if let Some(installed) = attempt.installed {
                entries.push(installed);
            }
            entries.extend(attempt.displaced);
            return Err((stage, error));
        }
        publication = attempt.publication;
        if !absent_precondition
            && attempt.publication != LocalConditionalPublicationState::NotPublished
            && attempt.displaced.is_empty()
        {
            if let Some(installed) = attempt.installed {
                entries.push(installed);
            }
            publication = LocalConditionalPublicationState::Indeterminate;
            let error = attempt.error.unwrap_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    "content publication did not report the displaced target",
                )
            });
            return Err((stage, error));
        }
        if let Some(error) = attempt.error {
            if !absent_precondition && attempt.publication == LocalConditionalPublicationState::Published {
                publication_attempt_error = Some(error);
                post_publication_conflict = true;
            } else {
                if let Some(mut installed) = attempt.installed {
                    installed.set_role(LocalRecoveryFileRole::StagedCandidate);
                    entries.push(installed);
                }
                entries.extend(attempt.displaced);
                return Err((stage, error));
            }
        }
        let Some(installed) = attempt.installed else {
            entries.extend(attempt.displaced);
            publication = LocalConditionalPublicationState::Indeterminate;
            return Err((
                stage,
                io::Error::new(io::ErrorKind::InvalidData, "backend did not report installed entry"),
            ));
        };
        let mut displaced_entries = Vec::new();
        for mut displaced in attempt.displaced {
            stage = LocalConditionalWriteStage::VerifyDisplaced;
            if let Err(error) = backend
                .checkpoint(ConditionalPoint::VerifyDisplaced)
                .and_then(|()| backend.verify_entry(&displaced))
            {
                entries.push(installed);
                entries.push(displaced);
                entries.extend(displaced_entries);
                publication = LocalConditionalPublicationState::Indeterminate;
                return Err((stage, error));
            }
            if let LocalWritePrecondition::Content(expected) = &precondition {
                let actual = match digest_entry::<D>(&displaced) {
                    Ok(digest) => digest,
                    Err(error) => {
                        entries.push(installed);
                        entries.push(displaced);
                        entries.extend(displaced_entries);
                        publication = LocalConditionalPublicationState::Indeterminate;
                        return Err((stage, error));
                    }
                };
                if actual != *expected {
                    displaced.set_role(LocalRecoveryFileRole::RollbackDisplaced);
                    post_publication_conflict = true;
                }
            }
            displaced_entries.push(displaced);
        }
        stage = LocalConditionalWriteStage::VerifyInstalled;
        if let Err(error) = backend.checkpoint(ConditionalPoint::VerifyInstalled) {
            entries.push(installed);
            entries.extend(displaced_entries);
            publication = LocalConditionalPublicationState::Indeterminate;
            return Err((stage, error));
        }
        let candidate_identity = match backend.is_candidate(&installed) {
            Ok(identity) => identity,
            Err(error) => {
                entries.push(installed);
                entries.extend(displaced_entries);
                publication = LocalConditionalPublicationState::Indeterminate;
                return Err((stage, error));
            }
        };
        let installed_digest = match digest_entry::<D>(&installed) {
            Ok(digest) => digest,
            Err(error) => {
                entries.push(installed);
                entries.extend(displaced_entries);
                publication = LocalConditionalPublicationState::Indeterminate;
                return Err((stage, error));
            }
        };
        let installed_valid = candidate_identity && installed_digest == candidate_digest;
        if !installed_valid {
            post_publication_conflict = true;
        }
        if post_publication_conflict {
            let Some(displaced) = displaced_entries.first() else {
                entries.push(installed);
                return Err((
                    stage,
                    io::Error::new(
                        io::ErrorKind::InvalidData,
                        "publication verification failed without a displaced entry",
                    ),
                ));
            };
            stage = LocalConditionalWriteStage::RollbackPrecheck;
            let old_digest = match digest_entry::<D>(displaced) {
                Ok(digest) => digest,
                Err(error) => {
                    entries.push(installed);
                    entries.extend(displaced_entries);
                    publication = LocalConditionalPublicationState::Indeterminate;
                    return Err((stage, error));
                }
            };
            if let Err(error) = backend.checkpoint(ConditionalPoint::RollbackPrecheck) {
                entries.push(installed);
                entries.extend(displaced_entries);
                publication = LocalConditionalPublicationState::Indeterminate;
                return Err((stage, error));
            }
            stage = LocalConditionalWriteStage::Rollback;
            if let Err(error) = backend.checkpoint(ConditionalPoint::Rollback) {
                entries.push(installed);
                entries.extend(displaced_entries);
                return Err((stage, error));
            }
            // Recheck immediately before the reverse native operation. Identity alone
            // misses in-place writes to the installed candidate after the earlier
            // publication readback.
            stage = LocalConditionalWriteStage::RollbackPrecheck;
            let candidate_still_installed = match backend.is_candidate(&installed) {
                Ok(identity) => identity,
                Err(error) => {
                    entries.push(installed);
                    entries.extend(displaced_entries);
                    publication = LocalConditionalPublicationState::Indeterminate;
                    return Err((stage, error));
                }
            };
            let current_installed_digest = match digest_entry::<D>(&installed) {
                Ok(digest) => digest,
                Err(error) => {
                    entries.push(installed);
                    entries.extend(displaced_entries);
                    publication = LocalConditionalPublicationState::Indeterminate;
                    return Err((stage, error));
                }
            };
            if !candidate_still_installed || current_installed_digest != candidate_digest {
                entries.push(installed);
                entries.extend(displaced_entries);
                publication = LocalConditionalPublicationState::Indeterminate;
                return Err((
                    stage,
                    io::Error::new(
                        io::ErrorKind::InvalidData,
                        "rollback precheck no longer observes the installed candidate",
                    ),
                ));
            }
            if let Err(error) = backend.verify_entry(displaced) {
                entries.push(installed);
                entries.extend(displaced_entries);
                publication = LocalConditionalPublicationState::Indeterminate;
                return Err((stage, error));
            }
            if let Err(error) = backend.checkpoint(ConditionalPoint::RollbackValidated) {
                entries.push(installed);
                entries.extend(displaced_entries);
                publication = LocalConditionalPublicationState::Indeterminate;
                return Err((stage, error));
            }
            stage = LocalConditionalWriteStage::Rollback;
            let rollback = backend.rollback(displaced);
            publication = rollback.publication;
            if let Err(error) = backend.checkpoint(ConditionalPoint::RollbackCompleted) {
                if let Some(restored) = rollback.installed {
                    entries.push(restored);
                }
                entries.extend(rollback.displaced);
                entries.extend(displaced_entries);
                publication = LocalConditionalPublicationState::Indeterminate;
                return Err((stage, error));
            }
            if let Some(error) = rollback.error {
                let rollback_displaced_files_reported = !rollback.displaced.is_empty();
                for mut displaced_entry in displaced_entries {
                    let represented_by_receipt = rollback
                        .installed
                        .iter()
                        .chain(rollback.displaced.iter())
                        .any(|receipt| receipt.namespace_path() == displaced_entry.namespace_path());
                    if !represented_by_receipt && backend.verify_entry(&displaced_entry).is_ok() {
                        if !rollback_displaced_files_reported {
                            displaced_entry.set_role(LocalRecoveryFileRole::Displaced);
                        }
                        entries.push(displaced_entry);
                    }
                }
                if let Some(mut current_target) = rollback.installed {
                    current_target.set_role(LocalRecoveryFileRole::Displaced);
                    entries.push(current_target);
                }
                entries.extend(rollback.displaced.into_iter().map(|mut entry| {
                    entry.set_role(LocalRecoveryFileRole::RollbackDisplaced);
                    entry
                }));
                return Err((stage, error));
            }
            let Some(restored) = rollback.installed else {
                entries.push(installed);
                entries.extend(displaced_entries);
                entries.extend(rollback.displaced);
                publication = LocalConditionalPublicationState::Indeterminate;
                return Err((
                    stage,
                    io::Error::new(io::ErrorKind::InvalidData, "rollback did not report restored target"),
                ));
            };
            entries.extend(rollback.displaced.into_iter().map(|mut entry| {
                entry.set_role(LocalRecoveryFileRole::RollbackDisplaced);
                entry
            }));
            stage = LocalConditionalWriteStage::VerifyRollback;
            let restored_digest = match backend
                .checkpoint(ConditionalPoint::VerifyRollback)
                .and_then(|()| digest_entry::<D>(&restored))
            {
                Ok(digest) => digest,
                Err(error) => {
                    entries.push(restored);
                    publication = LocalConditionalPublicationState::Indeterminate;
                    return Err((stage, error));
                }
            };
            if publication != LocalConditionalPublicationState::Restored || restored_digest != old_digest {
                entries.push(restored);
                publication = LocalConditionalPublicationState::Indeterminate;
                return Err((
                    stage,
                    io::Error::new(io::ErrorKind::InvalidData, "rollback verification failed"),
                ));
            }
            if options.durability() == LocalDurabilityRequirement::Required
                || options.file_sync() == LocalFileSyncRequirement::Required
            {
                stage = LocalConditionalWriteStage::Sync;
                let sync_result = backend.checkpoint(ConditionalPoint::SyncRollback).and_then(|()| {
                    backend.sync_entry(&restored).and_then(|()| {
                        if options.durability() == LocalDurabilityRequirement::Required {
                            backend.sync_namespace()
                        } else {
                            Ok(())
                        }
                    })
                });
                if let Err(error) = sync_result {
                    entries.push(restored);
                    return Err((stage, error));
                }
            }
            entries.push(restored);
            let error = publication_attempt_error.take().unwrap_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidData,
                    "post-publication conflict; prior target restored",
                )
            });
            return Err((stage, error));
        }
        entries.extend(displaced_entries);
        if !installed_valid {
            entries.push(installed);
            return Err((
                stage,
                io::Error::new(io::ErrorKind::InvalidData, "installed candidate verification failed"),
            ));
        }
        if sync_requirement != LocalFileSyncRequirement::NotRequired {
            stage = LocalConditionalWriteStage::Sync;
            let sync_result = backend
                .checkpoint(ConditionalPoint::SyncInstalled)
                .and_then(|()| backend.sync_entry(&installed));
            match sync_result {
                Ok(()) => file_synced = true,
                Err(error) if sync_requirement == LocalFileSyncRequirement::Required => {
                    entries.push(installed);
                    return Err((stage, error));
                }
                Err(_) => {
                    file_synced = false;
                    if options.durability() == LocalDurabilityRequirement::Preferred {
                        durable = false;
                    }
                }
            }
        }
        if options.durability() == LocalDurabilityRequirement::Required {
            stage = LocalConditionalWriteStage::Sync;
            let sync = backend
                .checkpoint(ConditionalPoint::BeforeRequiredParentSync)
                .and_then(|()| backend.sync_namespace());
            if let Err(error) = sync {
                entries.push(installed);
                return Err((stage, error));
            }
        } else if options.durability() == LocalDurabilityRequirement::Preferred {
            stage = LocalConditionalWriteStage::Sync;
            if backend.sync_namespace().is_err() {
                durable = false;
            }
        }
        Ok((installed, candidate_digest))
    })();
    let (installed, candidate_digest) = match result {
        Ok(value) => value,
        Err((failed_stage, source)) => {
            stage = failed_stage;
            let reason = match (failed_stage, source.kind()) {
                (LocalConditionalWriteStage::Precheck, io::ErrorKind::AlreadyExists) => {
                    LocalConditionalFailureReason::TargetExists
                }
                (LocalConditionalWriteStage::Precheck, io::ErrorKind::NotFound) => {
                    LocalConditionalFailureReason::TargetMissing
                }
                (LocalConditionalWriteStage::Precheck, io::ErrorKind::InvalidData) => {
                    LocalConditionalFailureReason::PreconditionMismatch
                }
                (_, io::ErrorKind::AlreadyExists) => LocalConditionalFailureReason::TargetExists,
                (_, _) if post_publication_conflict => LocalConditionalFailureReason::PostPublicationConflict,
                _ => LocalConditionalFailureReason::Io,
            };
            let mut recovery: Vec<_> = backend
                .take_recovery_entries()
                .into_iter()
                .map(|entry| {
                    let role = entry.role();
                    entry.into_recovery(role)
                })
                .collect();
            recovery.extend(entries.into_iter().map(|entry| {
                let role = entry.role();
                entry.into_recovery(role)
            }));
            return Err(LocalConditionalCommitError::new(
                LocalFileError::from_io(LocalFileOperation::Commit, None, None, source),
                reason,
                stage,
                publication,
                recovery,
            ));
        }
    };

    let (mut cleanup_errors, retained_files) = match backend.checkpoint(ConditionalPoint::SaveComplete) {
        Ok(()) => cleanup_entries(&mut backend, entries),
        Err(source) => {
            let retained = entries
                .into_iter()
                .map(|entry| {
                    let role = entry.role();
                    entry.into_recovery(role)
                })
                .collect();
            (
                vec![LocalFileError::from_io(LocalFileOperation::Cleanup, None, None, source)],
                retained,
            )
        }
    };
    if options.durability() != LocalDurabilityRequirement::NotRequired
        && let Err(source) = backend
            .checkpoint(ConditionalPoint::CleanupSync)
            .and_then(|()| backend.sync_namespace())
    {
        cleanup_errors.push(LocalFileError::from_io(LocalFileOperation::Commit, None, None, source));
    }
    let outcome = LocalWriteOutcome::new(
        LocalWriterState::Committed,
        true,
        publication_method,
        durable,
        bytes_written,
        None,
    )
    .with_file_synced(file_synced);
    let _ = installed;
    Ok(LocalConditionalWriteOutcome::new(
        candidate_digest,
        outcome,
        retained_files,
        cleanup_errors,
        current_directory,
    ))
}

#[cfg(all(test, feature = "digest"))]
mod backend_phase_error_contract_tests {
    use std::path::PathBuf;

    use qubit_io::Output as ByteOutput;
    use sha2::Digest;
    use sha2::Sha256;

    use super::super::ConditionalEntry;
    use super::super::ConditionalHooks;
    use super::super::NativeAttempt;
    use super::super::RecoveryAccess;
    use super::super::fake_backend::FakeBackend;
    use super::ConditionalBackend;
    use super::ConditionalPoint;
    use super::commit_conditional;
    use super::io;
    use crate::LocalFileError;
    use crate::LocalFileOperation;
    use crate::error::LocalConditionalFailureReason;
    use crate::error::LocalConditionalWriteStage;
    use crate::options::LocalWritePrecondition;
    use crate::outcome::LocalConditionalPublicationState;
    use crate::recovery::LocalRecoveryFileRole;

    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    enum FailurePhase {
        Never,
        CandidateRead,
        TargetRead,
        Metadata,
        Verify,
        BeginPublication,
    }

    #[derive(Debug)]
    struct FailedReadAccess {
        path: PathBuf,
    }

    impl RecoveryAccess for FailedReadAccess {
        fn open_reader(&self) -> crate::LocalResult<crate::LocalFileReader> {
            Err(LocalFileError::from_io(
                LocalFileOperation::OpenReader,
                Some(self.path.clone()),
                None,
                io::Error::new(io::ErrorKind::PermissionDenied, "model backend denied retained read"),
            ))
        }
        fn sync_file(&self) -> io::Result<()> {
            std::fs::File::open(&self.path)?.sync_all()
        }
    }

    struct ModelBackend {
        inner: FakeBackend,
        phase: FailurePhase,
        failed_candidate: ConditionalEntry,
    }

    impl ByteOutput for ModelBackend {
        type Item = u8;
        unsafe fn write_unchecked(&mut self, bytes: &[u8], index: usize, count: usize) -> io::Result<usize> {
            ByteOutput::write(&mut self.inner, &bytes[index..index + count])
        }
        fn flush(&mut self) -> io::Result<()> {
            ByteOutput::flush(&mut self.inner)
        }
    }

    impl ConditionalBackend for ModelBackend {
        fn candidate(&self) -> &ConditionalEntry {
            if self.phase == FailurePhase::CandidateRead {
                &self.failed_candidate
            } else {
                self.inner.candidate()
            }
        }
        fn create_candidate_copy(&mut self) -> io::Result<ConditionalEntry> {
            self.inner.create_candidate_copy()
        }
        fn open_target(&self) -> io::Result<ConditionalEntry> {
            let target = self.inner.open_target()?;
            if self.phase == FailurePhase::TargetRead {
                let path = target.namespace_path().to_path_buf();
                Ok(ConditionalEntry::new(
                    target.role(),
                    path.clone(),
                    Some(path.clone()),
                    Box::new(FailedReadAccess { path }),
                ))
            } else {
                Ok(target)
            }
        }
        fn preserve_metadata(&mut self, entry: &ConditionalEntry) -> io::Result<()> {
            if self.phase == FailurePhase::Metadata {
                Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "model metadata preservation failure",
                ))
            } else {
                self.inner.preserve_metadata(entry)
            }
        }
        fn sync_candidate(&self) -> io::Result<()> {
            self.inner.sync_candidate()
        }
        fn sync_entry(&self, entry: &ConditionalEntry) -> io::Result<()> {
            self.inner.sync_entry(entry)
        }
        fn sync_namespace(&self) -> io::Result<()> {
            self.inner.sync_namespace()
        }
        fn is_candidate(&self, entry: &ConditionalEntry) -> io::Result<bool> {
            self.inner.is_candidate(entry)
        }
        fn verify_entry(&self, entry: &ConditionalEntry) -> io::Result<()> {
            if self.phase == FailurePhase::Verify {
                Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "model candidate verification failure",
                ))
            } else {
                self.inner.verify_entry(entry)
            }
        }
        fn begin_publication(&mut self) -> io::Result<()> {
            if self.phase == FailurePhase::BeginPublication {
                Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "model publication setup failure",
                ))
            } else {
                self.inner.begin_publication()
            }
        }
        fn publish_absent(&mut self) -> NativeAttempt {
            self.inner.publish_absent()
        }
        fn publish_content(&mut self) -> NativeAttempt {
            self.inner.publish_content()
        }
        fn rollback(&mut self, displaced: &ConditionalEntry) -> NativeAttempt {
            self.inner.rollback(displaced)
        }
        fn remove_observed(&mut self, entry: &ConditionalEntry) -> io::Result<()> {
            self.inner.remove_observed(entry)
        }
        fn abort(&mut self) -> io::Result<()> {
            self.inner.abort()
        }
        fn take_recovery_entries(&mut self) -> Vec<ConditionalEntry> {
            self.inner.take_recovery_entries()
        }
        fn set_hooks(&mut self, hooks: ConditionalHooks) {
            self.inner.set_hooks(hooks);
        }
        fn checkpoint(&mut self, point: ConditionalPoint) -> io::Result<()> {
            self.inner.checkpoint(point)
        }
    }

    #[test]
    fn model_backend_errors_preserve_stage_recovery_and_unpublished_destination() {
        use super::super::fake_backend::required_options;
        use crate::options::LocalWriteMetadataPolicy;

        for (phase, expected_stage) in [
            (FailurePhase::CandidateRead, LocalConditionalWriteStage::VerifyCandidate),
            (FailurePhase::TargetRead, LocalConditionalWriteStage::Precheck),
            (FailurePhase::Metadata, LocalConditionalWriteStage::Precheck),
            (FailurePhase::Verify, LocalConditionalWriteStage::Precheck),
            (FailurePhase::BeginPublication, LocalConditionalWriteStage::Publish),
        ] {
            let inner = FakeBackend::new(b"old", b"candidate");
            let (_lease, target) = inner.target_observer();
            let path = inner.candidate().namespace_path().to_path_buf();
            let failed_candidate = ConditionalEntry::new(
                LocalRecoveryFileRole::StagedCandidate,
                path.clone(),
                Some(path.clone()),
                Box::new(FailedReadAccess { path }),
            );
            let backend = ModelBackend {
                inner,
                phase,
                failed_candidate,
            };
            let options = required_options().with_metadata_policy(LocalWriteMetadataPolicy::PreserveExisting);
            let error = commit_conditional::<Sha256, _>(
                backend,
                LocalWritePrecondition::Content(Sha256::digest(b"old")),
                options,
                9,
            )
            .expect_err("backend phase failure must propagate");
            assert_eq!(error.stage(), expected_stage, "{phase:?}");
            assert_eq!(error.reason(), LocalConditionalFailureReason::Io, "{phase:?}");
            assert_eq!(
                error.error().io_error_kind(),
                if matches!(phase, FailurePhase::CandidateRead | FailurePhase::TargetRead) {
                    io::ErrorKind::Other
                } else {
                    io::ErrorKind::PermissionDenied
                },
                "{phase:?}"
            );
            assert_eq!(
                error.publication(),
                LocalConditionalPublicationState::NotPublished,
                "{phase:?}"
            );
            assert_eq!(std::fs::read(&target).expect("target remains old"), b"old");
            if phase == FailurePhase::CandidateRead {
                assert!(error.candidate().is_none());
            } else {
                let candidate_copy = error.candidate().expect("independent candidate copy retained");
                let mut reader = candidate_copy.open_reader().expect("recovery copy opens");
                let mut bytes = Vec::new();
                qubit_io::Streams::copy_input_to_output(&mut reader, &mut bytes).expect("recovery copy reads");
                assert_eq!(bytes, b"candidate");
            }
        }
    }

    #[test]
    fn transparent_model_adapter_preserves_successful_publication_sync_and_cleanup() {
        use super::super::ConditionalHookAction;
        use super::super::fake_backend::Event;
        use super::super::fake_backend::required_options;
        use crate::options::LocalWriteMetadataPolicy;

        for absent in [false, true] {
            let inner = FakeBackend::new(b"old", b"candidate");
            if absent {
                inner.remove_target();
            }
            let (lease, target) = inner.target_observer();
            let events = inner.events();
            let namespace_syncs = inner.namespace_sync_observer();
            let candidate = inner.candidate().namespace_path().to_path_buf();
            let failed_candidate = ConditionalEntry::new(
                LocalRecoveryFileRole::StagedCandidate,
                candidate.clone(),
                Some(candidate.clone()),
                Box::new(FailedReadAccess {
                    path: candidate.clone(),
                }),
            );
            let mut backend = ModelBackend {
                inner,
                phase: FailurePhase::Never,
                failed_candidate,
            };
            let hooks = ConditionalHooks::sequence(vec![(
                ConditionalPoint::BeforeRequiredFileSync,
                ConditionalHookAction::Noop,
            )]);
            backend.set_hooks(hooks.clone());
            ByteOutput::write_fully(&mut backend, b" appended").expect("adapter forwards candidate writes");
            let precondition = if absent {
                LocalWritePrecondition::Absent
            } else {
                LocalWritePrecondition::Content(Sha256::digest(b"old"))
            };
            let outcome = commit_conditional::<Sha256, _>(
                backend,
                precondition,
                required_options().with_metadata_policy(LocalWriteMetadataPolicy::PreserveExisting),
                18,
            )
            .expect("transparent adapter preserves successful commit");
            assert_eq!(
                std::fs::read(&target).expect("published candidate"),
                b"candidate appended"
            );
            assert_eq!(outcome.digest(), &Sha256::digest(b"candidate appended"));
            assert!(outcome.write_outcome().durable());
            assert!(outcome.write_outcome().file_synced());
            assert!(outcome.cleanup_errors().is_empty());
            assert!(outcome.retained_files().is_empty());
            assert!(!candidate.exists(), "candidate name was published");
            assert!(
                hooks.take(ConditionalPoint::BeforeRequiredFileSync).is_none(),
                "configured sync checkpoint consumed"
            );
            assert_eq!(
                namespace_syncs.get(),
                3,
                "prepublication, publication and cleanup namespace syncs"
            );
            assert_eq!(
                events
                    .borrow()
                    .iter()
                    .filter(|event| **event == Event::RequiredSync)
                    .count(),
                if absent { 3 } else { 4 },
                "candidate, copy, installed and optional preserved metadata synchronization"
            );
            assert_eq!(
                std::fs::read_dir(lease.path()).expect("final namespace").count(),
                1,
                "successful commit leaves exactly the published target"
            );
        }
    }

    #[test]
    fn transparent_model_adapter_abort_preserves_unpublished_data_and_releases_ownership() {
        let inner = FakeBackend::new(b"old", b"candidate");
        let (lease, target) = inner.target_observer();
        let root = lease.path().to_path_buf();
        let candidate = inner.candidate().namespace_path().to_path_buf();
        let failed_candidate = ConditionalEntry::new(
            LocalRecoveryFileRole::StagedCandidate,
            candidate.clone(),
            Some(candidate.clone()),
            Box::new(FailedReadAccess {
                path: candidate.clone(),
            }),
        );
        let mut backend = ModelBackend {
            inner,
            phase: FailurePhase::Never,
            failed_candidate,
        };
        ByteOutput::write_fully(&mut backend, b" pending").expect("unpublished candidate write");
        backend.abort().expect("adapter forwards fake backend abort");
        assert_eq!(std::fs::read(&target).expect("target untouched"), b"old");
        assert_eq!(
            std::fs::read(&candidate).expect("fake backend retains its candidate until ownership ends"),
            b"candidate pending"
        );
        drop(backend);
        assert_eq!(
            std::fs::read(&target).expect("external lease still retains namespace"),
            b"old"
        );
        assert!(candidate.exists());
        drop(lease);
        assert!(
            !root.exists(),
            "adapter must not retain a hidden native namespace lease"
        );
    }
}
