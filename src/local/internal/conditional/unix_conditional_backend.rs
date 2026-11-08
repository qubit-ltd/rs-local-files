// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Unix conditional publication backend and its state transitions.

use std::ffi::CString;
use std::fs::File;
use std::fs::Metadata;
use std::io;
use std::io::Read;
use std::io::Write;
use std::mem::take;
use std::os::unix::fs::MetadataExt;
use std::path::Path;

use libc::EIO;
use qubit_io::Output;

use super::super::ConditionalBackend;
use super::super::ConditionalEntry;
use super::super::ConditionalHookAction;
use super::super::ConditionalHooks;
use super::super::ConditionalPoint;
use super::super::NativeAttempt;
use super::install;
use super::metadata;
use super::namespace;
use super::namespace::Namespace;
use super::private_entry_guard::PrivateEntryGuard;
use super::recovery;
#[cfg(test)]
use super::test_support;
use crate::options::LocalWriteOptions;
use crate::outcome::LocalConditionalPublicationState;
use crate::outcome::LocalWritePublicationMethod;
use crate::recovery::LocalRecoveryFileRole;

const COPY_BUFFER_SIZE: usize = 64 * 1024;

/// Selects the atomic exchange method used for content publication.
#[cfg(feature = "digest")]
fn exchange_publication_method() -> LocalWritePublicationMethod {
    LocalWritePublicationMethod::AtomicExchange
}

/// Selects atomic rename when digest-backed exchange is unavailable.
#[cfg(not(feature = "digest"))]
fn exchange_publication_method() -> LocalWritePublicationMethod {
    LocalWritePublicationMethod::AtomicRename
}

/// Performs conditional publication within a retained Unix directory namespace.
///
/// The backend keeps file descriptors and entry identities so publication and
/// rollback can verify names relative to the same directory authority.
#[derive(Debug)]
pub(crate) struct UnixConditionalBackend {
    /// The retained directory namespace used for every relative entry
    /// operation.
    pub(super) namespace: Namespace,
    /// The private namespace entry currently holding the staged candidate.
    pub(super) candidate_name: CString,
    /// The open file descriptor used to write and synchronize candidate bytes.
    pub(super) candidate: File,
    /// The device and inode captured when the staged candidate was created.
    pub(super) candidate_identity: (u64, u64),
    /// The optional private name of the independently verified candidate copy.
    pub(super) candidate_copy_name: Option<CString>,
    /// The device and inode expected for the verified candidate copy.
    pub(super) candidate_copy_identity: Option<(u64, u64)>,
    /// The recovery handle for the staged candidate.
    pub(super) candidate_entry: ConditionalEntry,
    /// Whether abort still owns cleanup of the unpublished candidate.
    pub(super) candidate_guard: bool,
    /// Entries retained for recovery after incomplete cleanup or copying.
    pub(super) recovery_entries: Vec<ConditionalEntry>,
    /// Deterministic checkpoint actions used by tests and test-support builds.
    pub(super) hooks: ConditionalHooks,
    /// A competing target change deferred until rollback begins.
    pub(super) deferred_rollback_action: Option<ConditionalHookAction>,
    /// Whether the next successful exchange should report an injected partial
    /// result.
    pub(super) partial_publish: bool,
}

impl UnixConditionalBackend {
    /// Opens the target through its host path and initializes a guarded
    /// candidate.
    ///
    /// # Parameters
    ///
    /// * `path` - Destination path whose parent namespace is retained.
    /// * `options` - Local write policy used while opening that namespace.
    ///
    /// # Returns
    ///
    /// A backend with a private staged candidate.
    ///
    /// # Errors
    ///
    /// Returns an I/O error if the namespace or candidate cannot be opened.
    pub(crate) fn open_host(path: &Path, options: &LocalWriteOptions) -> io::Result<Self> {
        let namespace = Namespace::open_host(path, options)?;
        Self::open(namespace)
    }

    /// Opens the target beneath a retained root descriptor and initializes a
    /// candidate.
    ///
    /// # Parameters
    ///
    /// * `root` - Open directory descriptor that anchors path resolution.
    /// * `relative` - Target path resolved beneath `root`.
    /// * `namespace_absolute` - Absolute diagnostic path for the namespace.
    /// * `diagnostic` - Optional path used to enrich error reporting.
    /// * `options` - Local write policy applied during namespace setup.
    ///
    /// # Returns
    ///
    /// A backend retaining the rooted namespace and a private staged candidate.
    ///
    /// # Errors
    ///
    /// Returns an I/O error if rooted resolution or candidate setup fails.
    pub(crate) fn open_rooted(
        root: File,
        relative: &Path,
        namespace_absolute: &Path,
        diagnostic: Option<&Path>,
        options: &LocalWriteOptions,
    ) -> io::Result<Self> {
        let namespace = Namespace::open_rooted(root, relative, namespace_absolute, diagnostic, options)?;
        Self::open(namespace)
    }

    /// Creates and identity-checks a private staging entry in the supplied
    /// namespace.
    ///
    /// # Parameters
    ///
    /// * `namespace` - Retained directory namespace in which the candidate is
    ///   created.
    ///
    /// # Errors
    ///
    /// Returns an I/O error if candidate creation, identity inspection, or
    /// entry registration fails.
    pub(in crate::local::internal::conditional::unix) fn open(namespace: Namespace) -> io::Result<Self> {
        let (candidate_name, candidate) = namespace.create_private("candidate")?;
        let mut guard = match PrivateEntryGuard::new(&namespace, candidate_name.clone(), &candidate) {
            Ok(guard) => guard,
            Err((primary, mut guard)) => return Err(guard.finish_with(primary)),
        };
        let metadata = match candidate_metadata(&candidate) {
            Ok(metadata) => metadata,
            Err(primary) => return Err(guard.finish_with(primary)),
        };
        let candidate_identity = (metadata.dev(), metadata.ino());
        let candidate_entry =
            match namespace.entry(LocalRecoveryFileRole::StagedCandidate, candidate_name.clone(), None) {
                Ok(entry) => entry,
                Err(primary) => return Err(guard.finish_with(primary)),
            };
        guard.disarm();
        drop(guard);
        Ok(Self {
            namespace,
            candidate_name,
            candidate,
            candidate_identity,
            candidate_copy_name: None,
            candidate_copy_identity: None,
            candidate_entry,
            candidate_guard: true,
            recovery_entries: Vec::new(),
            hooks: ConditionalHooks::default(),
            deferred_rollback_action: None,
            partial_publish: false,
        })
    }

    /// Reopens a named entry with the requested recovery role.
    ///
    /// # Parameters
    ///
    /// * `role` - Recovery role assigned to the reopened entry.
    /// * `name` - NUL-terminated name resolved relative to the retained
    ///   namespace.
    ///
    /// # Errors
    ///
    /// Returns an I/O error if the entry cannot be reopened or validated.
    fn entry(&self, role: LocalRecoveryFileRole, name: CString) -> io::Result<ConditionalEntry> {
        self.namespace.entry(role, name, None)
    }

    /// Exchanges two private namespace entries, including deterministic fault
    /// injection.
    ///
    /// # Parameters
    ///
    /// * `left` - Name of the first entry to exchange.
    /// * `right` - Name of the second entry to exchange.
    ///
    /// # Errors
    ///
    /// Returns an I/O error if the exchange syscall or configured test fault
    /// fails.
    fn exchange(&self, left: &CString, right: &CString) -> io::Result<()> {
        #[cfg(test)]
        if test_support::fail_if_requested(test_support::FailurePoint::ExchangeEioBeforeSuccess).is_err() {
            return Err(io::Error::from_raw_os_error(EIO));
        }
        install::exchange(&self.namespace, left, right)?;
        #[cfg(test)]
        if test_support::fail_if_requested(test_support::FailurePoint::ExchangeEioAfterSuccess).is_err() {
            return Err(io::Error::from_raw_os_error(EIO));
        }
        Ok(())
    }

    /// Classifies the result of an exchange from the observed entry identities.
    ///
    /// # Parameters
    ///
    /// * `error` - Error returned by the exchange operation, if any.
    ///
    /// # Returns
    ///
    /// The publication attempt inferred from observed candidate and displaced
    /// entries.
    fn attempt_after_exchange(&self, error: Option<io::Error>) -> NativeAttempt {
        let installed = self
            .entry(
                LocalRecoveryFileRole::StagedCandidate,
                self.namespace.target_name.clone(),
            )
            .ok();
        let displaced = self
            .entry(LocalRecoveryFileRole::Displaced, self.candidate_name.clone())
            .ok();
        #[cfg(test)]
        let candidate_identity =
            test_support::fail_if_requested(test_support::FailurePoint::CandidateIdentityAfterExchange)
                .ok()
                .and_then(|()| self.candidate.metadata().ok())
                .map(|metadata| (metadata.dev(), metadata.ino()));
        #[cfg(not(test))]
        let candidate_identity = self
            .candidate
            .metadata()
            .ok()
            .map(|metadata| (metadata.dev(), metadata.ino()));
        #[cfg(test)]
        let installed_identity =
            test_support::fail_if_requested(test_support::FailurePoint::InstalledIdentityAfterExchange)
                .ok()
                .and_then(|()| {
                    installed
                        .as_ref()
                        .and_then(|entry| entry.open_reader().ok())
                        .and_then(|reader| reader.as_file().metadata().ok())
                        .map(|metadata| (metadata.dev(), metadata.ino()))
                });
        #[cfg(test)]
        let installed_is_candidate = candidate_identity.is_some_and(|expected| installed_identity == Some(expected));
        #[cfg(not(test))]
        let installed_is_candidate = installed
            .as_ref()
            .and_then(|entry| entry.open_reader().ok())
            .and_then(|reader| reader.as_file().metadata().ok())
            .map(|metadata| (metadata.dev(), metadata.ino()))
            .is_some_and(|observed| candidate_identity.is_some_and(|expected| observed == expected));
        #[cfg(test)]
        let staged_identity = test_support::fail_if_requested(test_support::FailurePoint::StagedIdentityAfterExchange)
            .ok()
            .and_then(|()| {
                self.entry(LocalRecoveryFileRole::StagedCandidate, self.candidate_name.clone())
                    .ok()
                    .and_then(|entry| entry.open_reader().ok())
                    .and_then(|reader| reader.as_file().metadata().ok())
                    .map(|metadata| (metadata.dev(), metadata.ino()))
            });
        #[cfg(test)]
        let staged_is_candidate = candidate_identity.is_some_and(|expected| staged_identity == Some(expected));
        #[cfg(not(test))]
        let staged_is_candidate = self
            .entry(LocalRecoveryFileRole::StagedCandidate, self.candidate_name.clone())
            .ok()
            .and_then(|entry| entry.open_reader().ok())
            .and_then(|reader| reader.as_file().metadata().ok())
            .map(|metadata| (metadata.dev(), metadata.ino()))
            .is_some_and(|observed| candidate_identity.is_some_and(|expected| observed == expected));
        let publication = classify_exchange_publication(
            error.is_some(),
            installed_is_candidate,
            staged_is_candidate,
            displaced.is_some(),
        );
        NativeAttempt {
            publication_method: exchange_publication_method(),
            error,
            publication,
            installed: if publication == LocalConditionalPublicationState::NotPublished {
                None
            } else {
                installed
            },
            displaced: if publication == LocalConditionalPublicationState::NotPublished {
                Vec::new()
            } else {
                displaced.into_iter().collect()
            },
        }
    }

    /// Applies or defers the configured action at a commit protocol checkpoint.
    ///
    /// # Parameters
    ///
    /// * `point` - Protocol checkpoint whose configured action is applied.
    ///
    /// # Errors
    ///
    /// Returns an error when the configured checkpoint action fails.
    fn checkpoint_action(&mut self, point: ConditionalPoint) -> io::Result<()> {
        #[cfg(all(feature = "digest", feature = "test-support"))]
        self.hooks.run_callback(point)?;
        let action = self.hooks.take(point);
        if point == ConditionalPoint::RollbackPrecheck {
            if let Some(
                action @ (ConditionalHookAction::ReplaceTarget(_) | ConditionalHookAction::OverwriteTargetInPlace(_)),
            ) = action
            {
                self.deferred_rollback_action = Some(action);
                return Ok(());
            }
            if action.is_none() {
                return Ok(());
            }
        }
        match action {
            Some(ConditionalHookAction::Fail(kind)) => Err(io::Error::new(kind, "injected Unix conditional failure")),
            Some(ConditionalHookAction::ReplaceTarget(bytes)) => {
                self.namespace.replace_target(&bytes)?;
                Ok(())
            }
            Some(ConditionalHookAction::OverwriteTargetInPlace(bytes)) => {
                self.namespace.overwrite_target(&bytes)?;
                Ok(())
            }
            Some(ConditionalHookAction::PartialPublish) if point == ConditionalPoint::Publish => {
                self.partial_publish = true;
                Ok(())
            }
            Some(ConditionalHookAction::PartialCandidateCopy(_))
            | Some(ConditionalHookAction::CorruptCandidateCopy(_))
            | Some(ConditionalHookAction::PartialPublish)
            | Some(ConditionalHookAction::Noop)
            | None => Ok(()),
        }
    }
}

/// Classifies exchange state from observed identities without changing
/// receipts.
///
/// # Parameters
///
/// * `has_error` - Whether the exchange syscall reported an error.
/// * `installed_is_candidate` - Whether the destination has the candidate
///   identity.
/// * `staged_is_candidate` - Whether the staging name still has the candidate
///   identity.
/// * `displaced_exists` - Whether the displaced entry was observed.
///
/// # Returns
///
/// The publication state supported by the observed identities and syscall
/// result.
fn classify_exchange_publication(
    has_error: bool,
    installed_is_candidate: bool,
    staged_is_candidate: bool,
    displaced_exists: bool,
) -> LocalConditionalPublicationState {
    match (has_error, installed_is_candidate, staged_is_candidate, displaced_exists) {
        (false, true, _, true) => LocalConditionalPublicationState::Published,
        (true, _, true, _) => LocalConditionalPublicationState::NotPublished,
        (_, true, _, true) => LocalConditionalPublicationState::Published,
        _ => LocalConditionalPublicationState::Indeterminate,
    }
}

/// Reads candidate metadata, preserving deterministic failure injection in
/// tests.
///
/// # Parameters
///
/// * `candidate` - Open candidate file whose metadata is inspected.
///
/// # Errors
///
/// Returns an I/O error if injected failure occurs or metadata cannot be read.
fn candidate_metadata(candidate: &File) -> io::Result<Metadata> {
    #[cfg(test)]
    test_support::fail_if_requested(test_support::FailurePoint::CandidateMetadata)?;
    candidate.metadata()
}

impl Output for UnixConditionalBackend {
    /// Individual bytes accepted by the staging output.
    type Item = u8;

    /// Writes the supplied slices to the staged candidate in one vectored
    /// operation.
    ///
    /// # Parameters
    ///
    /// * `slices` - Ordered byte slices offered to the candidate file.
    ///
    /// # Errors
    ///
    /// Returns an I/O error if writing the slices fails.
    fn write_vectored(&mut self, slices: &[&[u8]]) -> io::Result<usize> {
        let slices: Vec<io::IoSlice<'_>> = slices.iter().map(|slice| io::IoSlice::new(slice)).collect();
        Write::write_vectored(&mut self.candidate, &slices)
    }

    /// Writes the caller-validated input range directly to the candidate.
    ///
    /// # Safety
    /// `index..index + count` must be within `input`.
    ///
    /// # Parameters
    ///
    /// * `input` - Bytes containing the range to write.
    /// * `index` - Start of the range within `input`.
    /// * `count` - Number of bytes in the range.
    ///
    /// # Errors
    ///
    /// Returns an I/O error if writing the selected bytes fails.
    unsafe fn write_unchecked(&mut self, input: &[u8], index: usize, count: usize) -> io::Result<usize> {
        Write::write(&mut self.candidate, &input[index..index + count])
    }

    /// Flushes buffered candidate output to its underlying file.
    #[inline]
    fn flush(&mut self) -> io::Result<()> {
        Write::flush(&mut self.candidate)
    }
}

impl UnixConditionalBackend {
    /// Opens the target only when identity and bytes match the verified
    /// candidate.
    fn open_verified_candidate_target(&self) -> io::Result<ConditionalEntry> {
        let target = self.entry(LocalRecoveryFileRole::Displaced, self.namespace.target_name.clone())?;
        let target_reader = target.open_reader().map_err(io::Error::other)?;
        let target_metadata = target_reader.as_file().metadata()?;
        if (target_metadata.dev(), target_metadata.ino()) != self.candidate_identity {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "rollback target no longer has the candidate identity",
            ));
        }
        let (Some(copy_name), Some(copy_identity)) = (&self.candidate_copy_name, self.candidate_copy_identity) else {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "verified candidate copy is unavailable for rollback",
            ));
        };
        let copy = self.entry(LocalRecoveryFileRole::CandidateCopy, copy_name.clone())?;
        let copy_reader = copy.open_reader().map_err(io::Error::other)?;
        let copy_metadata = copy_reader.as_file().metadata()?;
        if (copy_metadata.dev(), copy_metadata.ino()) != copy_identity
            || !same_contents(target_reader.as_file(), copy_reader.as_file())?
        {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "rollback target content no longer matches the verified candidate copy",
            ));
        }
        Ok(target)
    }

    /// Builds the rollback outcome from observed restored and moved identities.
    ///
    /// # Parameters
    ///
    /// * `old_identity` - Device and inode expected at the restored
    ///   destination.
    /// * `syscall_error` - Error reported by the exchange syscall, if any.
    ///
    /// # Returns
    ///
    /// The rollback outcome classified from the observed entry identities.
    fn rollback_receipt(&self, old_identity: (u64, u64), syscall_error: Option<io::Error>) -> NativeAttempt {
        let restored = self
            .entry(LocalRecoveryFileRole::Displaced, self.namespace.target_name.clone())
            .ok();
        let moved = self
            .entry(LocalRecoveryFileRole::RollbackDisplaced, self.candidate_name.clone())
            .ok();
        let restored_identity = restored
            .as_ref()
            .and_then(|entry| entry.open_reader().ok())
            .and_then(|reader| reader.as_file().metadata().ok())
            .map(|metadata| (metadata.dev(), metadata.ino()));
        let moved_identity = moved
            .as_ref()
            .and_then(|entry| entry.open_reader().ok())
            .and_then(|reader| reader.as_file().metadata().ok())
            .map(|metadata| (metadata.dev(), metadata.ino()));
        let moved_matches_candidate = moved_identity == Some(self.candidate_identity)
            && moved
                .as_ref()
                .is_some_and(|entry| self.matches_candidate_copy(entry).unwrap_or(false));
        let publication = if syscall_error.is_some() {
            LocalConditionalPublicationState::Indeterminate
        } else {
            LocalConditionalPublicationState::Restored
        };
        if restored_identity == Some(old_identity) && moved_matches_candidate {
            NativeAttempt {
                publication_method: exchange_publication_method(),
                error: syscall_error,
                publication,
                installed: restored,
                displaced: moved.into_iter().collect(),
            }
        } else {
            NativeAttempt {
                publication_method: exchange_publication_method(),
                error: syscall_error,
                publication: LocalConditionalPublicationState::Indeterminate,
                installed: restored,
                displaced: moved.into_iter().collect(),
            }
        }
    }

    /// Checks candidate identity and bytes against the independent verified
    /// copy.
    ///
    /// # Parameters
    ///
    /// * `entry` - Entry whose identity and contents are compared with the
    ///   verified copy.
    ///
    /// # Errors
    ///
    /// Returns an I/O error if either entry cannot be opened or read.
    fn matches_candidate_copy(&self, entry: &ConditionalEntry) -> io::Result<bool> {
        let Some(copy_name) = &self.candidate_copy_name else {
            return Ok(false);
        };
        let Some(copy_identity) = self.candidate_copy_identity else {
            return Ok(false);
        };
        let copy = self.entry(LocalRecoveryFileRole::CandidateCopy, copy_name.clone())?;
        let copy_reader = copy.open_reader().map_err(io::Error::other)?;
        let copy_metadata = copy_reader.as_file().metadata()?;
        let moved_reader = entry.open_reader().map_err(io::Error::other)?;
        let moved_metadata = moved_reader.as_file().metadata()?;
        Ok((copy_metadata.dev(), copy_metadata.ino()) == copy_identity
            && (moved_metadata.dev(), moved_metadata.ino()) == self.candidate_identity
            && same_contents(moved_reader.as_file(), copy_reader.as_file())?)
    }

    /// Builds a rollback outcome when the namespace state cannot be
    /// established.
    ///
    /// # Parameters
    ///
    /// * `error` - Error explaining why rollback state is indeterminate.
    /// * `installed` - Destination entry observed after the failed rollback, if
    ///   available.
    /// * `displaced` - Displaced entry observed after the failed rollback, if
    ///   available.
    ///
    /// # Returns
    ///
    /// An attempt marked indeterminate with the available recovery entries.
    fn indeterminate_rollback(
        &self,
        error: io::Error,
        installed: Option<ConditionalEntry>,
        displaced: Option<ConditionalEntry>,
    ) -> NativeAttempt {
        NativeAttempt {
            publication_method: exchange_publication_method(),
            error: Some(error),
            publication: LocalConditionalPublicationState::Indeterminate,
            installed,
            displaced: displaced.into_iter().collect(),
        }
    }
}

impl ConditionalBackend for UnixConditionalBackend {
    /// Returns the recovery handle for the staged candidate.
    #[inline]
    fn candidate(&self) -> &ConditionalEntry {
        &self.candidate_entry
    }

    /// Creates and registers a private copy of the staged candidate.
    ///
    /// The recovery entry temporarily owns responsibility for the copy while
    /// bytes are copied. The guard is disarmed only after that recovery entry
    /// is registered; on success the candidate-copy identity is recorded and
    /// the temporary recovery entry is removed. Each read uses only the
    /// initialized prefix of the fixed-size buffer.
    /// Copies staged bytes into a separately registered recovery entry.
    fn create_candidate_copy(&mut self) -> io::Result<ConditionalEntry> {
        let (name, mut copy) = self.namespace.create_private("candidate-copy")?;
        let mut guard = match PrivateEntryGuard::new(&self.namespace, name.clone(), &copy) {
            Ok(guard) => guard,
            Err((primary, mut guard)) => return Err(guard.finish_with(primary)),
        };
        let entry = match self.entry(LocalRecoveryFileRole::StagedCandidate, name.clone()) {
            Ok(entry) => entry,
            Err(primary) => return Err(guard.finish_with(primary)),
        };
        let recovery_entry = match self.entry(LocalRecoveryFileRole::StagedCandidate, name.clone()) {
            Ok(entry) => entry,
            Err(primary) => return Err(guard.finish_with(primary)),
        };
        #[cfg(test)]
        if let Err(primary) = test_support::fail_if_requested(test_support::FailurePoint::CandidateCopyRegistration) {
            return Err(guard.finish_with(primary));
        }
        self.recovery_entries.push(recovery_entry);
        guard.disarm();
        drop(guard);
        let mut reader = self.namespace.open_name(&self.candidate_name)?;
        let mut buffer = [0_u8; COPY_BUFFER_SIZE];
        loop {
            let count = match reader.read(&mut buffer) {
                Ok(0) => break,
                Ok(count) => count,
                Err(error) => return Err(error),
            };
            copy.write_all(&buffer[..count])?;
        }
        Write::flush(&mut copy)?;
        let metadata = copy.metadata()?;
        self.candidate_copy_name = Some(name);
        self.candidate_copy_identity = Some((metadata.dev(), metadata.ino()));
        self.recovery_entries.pop();
        Ok(entry)
    }

    /// Opens the current destination as a displaced recovery entry.
    #[inline]
    fn open_target(&self) -> io::Result<ConditionalEntry> {
        self.entry(LocalRecoveryFileRole::Displaced, self.namespace.target_name.clone())
    }

    /// Copies requested destination metadata onto the staged candidate.
    ///
    /// # Parameters
    ///
    /// * `observed` - Existing destination entry whose requested metadata is
    ///   preserved.
    ///
    /// # Errors
    ///
    /// Returns an I/O error if metadata cannot be read or applied.
    #[inline]
    fn preserve_metadata(&mut self, observed: &ConditionalEntry) -> io::Result<()> {
        metadata::preserve(observed, &self.candidate)
    }

    /// Synchronizes staged candidate contents and metadata.
    #[inline]
    fn sync_candidate(&self) -> io::Result<()> {
        self.candidate.sync_all()
    }
    /// Synchronizes the file represented by a retained entry.
    ///
    /// # Parameters
    ///
    /// * `entry` - Retained entry whose file data and metadata are
    ///   synchronized.
    ///
    /// # Errors
    ///
    /// Returns an I/O error if synchronization fails.
    #[inline]
    fn sync_entry(&self, entry: &ConditionalEntry) -> io::Result<()> {
        entry.sync_file()
    }
    /// Synchronizes the retained namespace and its parent directory.
    #[inline]
    fn sync_namespace(&self) -> io::Result<()> {
        self.namespace.sync_all()
    }

    /// Compares an entry identity with the staged candidate identity.
    ///
    /// # Parameters
    ///
    /// * `entry` - Entry whose opened file identity is checked.
    ///
    /// # Errors
    ///
    /// Returns an I/O error if either file's metadata cannot be read.
    fn is_candidate(&self, entry: &ConditionalEntry) -> io::Result<bool> {
        let installed = entry.open_reader().map_err(io::Error::other)?;
        let candidate = self.candidate.metadata()?;
        let installed = installed.as_file().metadata()?;
        Ok(candidate.dev() == installed.dev() && candidate.ino() == installed.ino())
    }

    /// Checks that an entry still matches its retained namespace identity.
    ///
    /// # Parameters
    ///
    /// * `entry` - Retained entry to verify against its namespace name.
    ///
    /// # Errors
    ///
    /// Returns an I/O error if the entry no longer matches its retained
    /// identity.
    #[inline]
    fn verify_entry(&self, entry: &ConditionalEntry) -> io::Result<()> {
        self.namespace.verify(entry)
    }

    /// Marks the candidate as potentially published before the native
    /// operation.
    fn begin_publication(&mut self) -> io::Result<()> {
        self.candidate_guard = false;
        Ok(())
    }

    /// Installs the candidate only when the destination is absent.
    fn publish_absent(&mut self) -> NativeAttempt {
        match install::install_absent(&self.namespace, &self.candidate_name) {
            Ok(()) => {
                self.candidate_guard = false;
                let installed = self
                    .entry(
                        LocalRecoveryFileRole::StagedCandidate,
                        self.namespace.target_name.clone(),
                    )
                    .ok();
                NativeAttempt {
                    publication_method: LocalWritePublicationMethod::AtomicRename,
                    error: None,
                    publication: if installed.is_some() {
                        LocalConditionalPublicationState::Published
                    } else {
                        LocalConditionalPublicationState::Indeterminate
                    },
                    installed,
                    displaced: Vec::new(),
                }
            }
            Err((error, publication)) => {
                let installed = if publication != LocalConditionalPublicationState::NotPublished {
                    self.entry(
                        LocalRecoveryFileRole::StagedCandidate,
                        self.namespace.target_name.clone(),
                    )
                    .ok()
                } else {
                    None
                };
                let staged_alias = if publication == LocalConditionalPublicationState::Published {
                    self.entry(LocalRecoveryFileRole::StagedCandidate, self.candidate_name.clone())
                        .ok()
                } else {
                    None
                };
                NativeAttempt {
                    publication_method: LocalWritePublicationMethod::AtomicRename,
                    error: Some(error),
                    publication,
                    installed,
                    displaced: staged_alias.into_iter().collect(),
                }
            }
        }
    }

    /// Publishes candidate content while retaining the displaced entry.
    fn publish_content(&mut self) -> NativeAttempt {
        match self.exchange(&self.candidate_name, &self.namespace.target_name) {
            Ok(()) => {
                self.candidate_guard = false;
                if self.partial_publish {
                    self.partial_publish = false;
                    self.attempt_after_exchange(Some(io::Error::other("injected partial publication")))
                } else {
                    self.attempt_after_exchange(None)
                }
            }
            Err(error) => {
                let indeterminate = error.raw_os_error() == Some(EIO);
                if indeterminate {
                    self.attempt_after_exchange(Some(error))
                } else {
                    NativeAttempt {
                        publication_method: exchange_publication_method(),
                        error: Some(error),
                        publication: LocalConditionalPublicationState::NotPublished,
                        installed: None,
                        displaced: Vec::new(),
                    }
                }
            }
        }
    }

    /// Attempts to restore the displaced entry after revalidating rollback
    /// inputs.
    ///
    /// # Parameters
    ///
    /// * `displaced` - Retained prior destination entry to restore.
    ///
    /// # Returns
    ///
    /// The publication and recovery state observed after attempting
    /// restoration.
    fn rollback(&mut self, displaced: &ConditionalEntry) -> NativeAttempt {
        if let Some(action) = self.deferred_rollback_action.take() {
            let result = match action {
                ConditionalHookAction::ReplaceTarget(bytes) => self.namespace.replace_target(&bytes),
                ConditionalHookAction::OverwriteTargetInPlace(bytes) => self.namespace.overwrite_target(&bytes),
                _ => Ok(()),
            };
            if let Err(error) = result {
                return self.indeterminate_rollback(error, None, None);
            }
        }
        let old_target = match displaced.open_reader() {
            Ok(reader) => reader,
            Err(error) => return self.indeterminate_rollback(io::Error::other(error), None, None),
        };
        let old_metadata = match old_target.as_file().metadata() {
            Ok(metadata) => (metadata.dev(), metadata.ino()),
            Err(error) => return self.indeterminate_rollback(error, None, None),
        };
        let displaced_name = match recovery::name(displaced) {
            Ok(name) => name,
            Err(error) => return self.indeterminate_rollback(error, None, None),
        };
        if let Err(error) = self.namespace.verify(displaced) {
            return self.indeterminate_rollback(error, None, None);
        }
        // Revalidate the target as the final observation before exchange. The
        // verified independent copy carries the exact bytes whose digest the
        // state machine checked before publication. This narrows, but cannot
        // eliminate, the race because POSIX exchange has no compare-and-swap.
        let _current_candidate = match self.open_verified_candidate_target() {
            Ok(entry) => entry,
            Err(error) => {
                return self.indeterminate_rollback(error, None, None);
            }
        };
        if let Err(error) = self.checkpoint_action(ConditionalPoint::RollbackExchange) {
            return self.indeterminate_rollback(error, None, None);
        }
        let syscall_error = self.exchange(&displaced_name, &self.namespace.target_name).err();
        let hook_error = self.checkpoint_action(ConditionalPoint::AfterRollbackSyscall).err();
        self.rollback_receipt(old_metadata, syscall_error.or(hook_error))
    }

    /// Removes an entry after the caller has verified its identity.
    ///
    /// # Parameters
    ///
    /// * `entry` - Verified entry to remove from the retained namespace.
    ///
    /// # Errors
    ///
    /// Returns an I/O error if removal fails.
    #[inline]
    fn remove_observed(&mut self, entry: &ConditionalEntry) -> io::Result<()> {
        self.namespace.remove(entry)
    }
    /// Removes the candidate when publication has not begun.
    fn abort(&mut self) -> io::Result<()> {
        if self.candidate_guard {
            self.namespace.remove(&self.candidate_entry)?;
            self.candidate_guard = false;
        }
        Ok(())
    }
    /// Transfers entries that must remain available for recovery.
    fn take_recovery_entries(&mut self) -> Vec<ConditionalEntry> {
        if let Ok((device, inode)) = namespace::identity_at(&self.namespace.parent, &self.candidate_name)
            && self
                .candidate
                .metadata()
                .is_ok_and(|metadata| metadata.dev() == device && metadata.ino() == inode)
            && !self
                .recovery_entries
                .iter()
                .any(|entry| entry.namespace_path().file_name() == self.candidate_entry.namespace_path().file_name())
            && let Ok(entry) = self.entry(LocalRecoveryFileRole::StagedCandidate, self.candidate_name.clone())
        {
            self.recovery_entries.push(entry);
        }
        take(&mut self.recovery_entries)
    }
    /// Installs deterministic actions for backend checkpoints.
    ///
    /// # Parameters
    ///
    /// * `hooks` - Checkpoint actions to install for this backend.
    #[cfg(any(test, all(feature = "digest", feature = "test-support")))]
    fn set_hooks(&mut self, hooks: ConditionalHooks) {
        self.hooks = hooks;
    }
    /// Runs the configured action for one commit protocol checkpoint.
    ///
    /// # Parameters
    ///
    /// * `point` - Protocol checkpoint whose configured action is run.
    ///
    /// # Errors
    ///
    /// Returns an error if the configured checkpoint action fails.
    fn checkpoint(&mut self, point: ConditionalPoint) -> io::Result<()> {
        self.checkpoint_action(point)
    }
}

/// Compares two files in bounded chunks without allocating their full contents.
///
/// # Parameters
///
/// * `left` - First file to compare, read from its current cursor position.
/// * `right` - Second file to compare, read from its current cursor position.
///
/// # Errors
///
/// Returns an I/O error if either file cannot be read.
fn same_contents(left: &File, right: &File) -> io::Result<bool> {
    let mut left = left;
    let mut right = right;
    let mut left_buffer = [0_u8; COPY_BUFFER_SIZE];
    let mut right_buffer = [0_u8; COPY_BUFFER_SIZE];
    loop {
        let left_count = left.read(&mut left_buffer)?;
        let right_count = right.read(&mut right_buffer)?;
        if left_count != right_count || left_buffer[..left_count] != right_buffer[..right_count] {
            return Ok(false);
        }
        if left_count == 0 {
            return Ok(true);
        }
    }
}
