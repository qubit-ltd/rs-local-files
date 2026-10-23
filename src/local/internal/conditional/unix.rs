// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Unix conditional publication using retained directory descriptors.

mod install;
mod metadata;
mod namespace;
mod recovery;

#[cfg(test)]
mod tests;

use std::ffi::CString;
use std::fs::File;
use std::io;
use std::io::Read;
use std::io::Write;
use std::os::unix::fs::MetadataExt;
use std::path::Path;

use namespace::Namespace;

use super::ConditionalBackend;
use super::ConditionalEntry;
use super::ConditionalHooks;
use super::ConditionalPoint;
use super::NativeAttempt;
use super::RecoveryAccess;
use crate::options::LocalWriteOptions;
use crate::outcome::LocalConditionalPublicationState;
use crate::recovery::LocalRecoveryFileRole;

const COPY_BUFFER_SIZE: usize = 64 * 1024;

#[derive(Debug)]
pub(crate) struct UnixConditionalBackend {
    namespace: Namespace,
    candidate_name: CString,
    candidate: File,
    candidate_identity: (u64, u64),
    candidate_copy_name: Option<CString>,
    candidate_copy_identity: Option<(u64, u64)>,
    candidate_entry: ConditionalEntry,
    candidate_guard: bool,
    recovery_entries: Vec<ConditionalEntry>,
    hooks: ConditionalHooks,
    deferred_rollback_action: Option<super::ConditionalHookAction>,
    partial_publish: bool,
}

impl UnixConditionalBackend {
    pub(crate) fn open_host(path: &Path, options: &LocalWriteOptions) -> io::Result<Self> {
        let namespace = Namespace::open_host(path, options)?;
        Self::open(namespace)
    }

    pub(crate) fn open_rooted(
        root: File,
        relative: &Path,
        diagnostic: &Path,
        options: &LocalWriteOptions,
    ) -> io::Result<Self> {
        let namespace = Namespace::open_rooted(root, relative, diagnostic, options)?;
        Self::open(namespace)
    }

    fn open(namespace: Namespace) -> io::Result<Self> {
        let (candidate_name, candidate) = namespace.create_private("candidate")?;
        let metadata = candidate.metadata()?;
        let candidate_identity = (metadata.dev(), metadata.ino());
        let candidate_entry = namespace.entry(LocalRecoveryFileRole::StagedCandidate, candidate_name.clone(), None)?;
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

    fn entry(&self, role: LocalRecoveryFileRole, name: CString) -> io::Result<ConditionalEntry> {
        self.namespace.entry(role, name, None)
    }

    fn exchange(&self, left: &CString, right: &CString) -> io::Result<()> {
        install::exchange(&self.namespace, left, right)
    }

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
        let candidate_identity = self
            .candidate
            .metadata()
            .ok()
            .map(|metadata| (metadata.dev(), metadata.ino()));
        let installed_is_candidate = installed
            .as_ref()
            .and_then(|entry| entry.open_reader().ok())
            .and_then(|reader| reader.as_file().metadata().ok())
            .map(|metadata| (metadata.dev(), metadata.ino()))
            == candidate_identity;
        let staged_is_candidate = self
            .entry(LocalRecoveryFileRole::StagedCandidate, self.candidate_name.clone())
            .ok()
            .and_then(|entry| entry.open_reader().ok())
            .and_then(|reader| reader.as_file().metadata().ok())
            .map(|metadata| (metadata.dev(), metadata.ino()))
            == candidate_identity;
        let publication = match (error.is_some(), installed_is_candidate, staged_is_candidate, &displaced) {
            (false, true, _, Some(_)) => LocalConditionalPublicationState::Published,
            (true, _, true, _) => LocalConditionalPublicationState::NotPublished,
            (_, true, _, Some(_)) => LocalConditionalPublicationState::Published,
            _ => LocalConditionalPublicationState::Indeterminate,
        };
        NativeAttempt {
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

    fn checkpoint_action(&mut self, point: ConditionalPoint) -> io::Result<()> {
        #[cfg(all(feature = "digest", feature = "test-support"))]
        self.hooks.run_callback(point)?;
        let action = self.hooks.take(point);
        if point == ConditionalPoint::RollbackPrecheck {
            if let Some(
                action @ (super::ConditionalHookAction::ReplaceTarget(_)
                | super::ConditionalHookAction::OverwriteTargetInPlace(_)),
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
            Some(super::ConditionalHookAction::Fail(kind)) => {
                Err(io::Error::new(kind, "injected Unix conditional failure"))
            }
            Some(super::ConditionalHookAction::ReplaceTarget(bytes)) => {
                self.namespace.replace_target(&bytes)?;
                Ok(())
            }
            Some(super::ConditionalHookAction::OverwriteTargetInPlace(bytes)) => {
                self.namespace.overwrite_target(&bytes)?;
                Ok(())
            }
            Some(super::ConditionalHookAction::PartialPublish) if point == ConditionalPoint::Publish => {
                self.partial_publish = true;
                Ok(())
            }
            Some(super::ConditionalHookAction::PartialCandidateCopy(_))
            | Some(super::ConditionalHookAction::CorruptCandidateCopy(_))
            | Some(super::ConditionalHookAction::PartialPublish)
            | Some(super::ConditionalHookAction::Noop)
            | None => Ok(()),
        }
    }
}

impl qubit_io::Output for UnixConditionalBackend {
    type Item = u8;

    fn write_vectored(&mut self, slices: &[&[u8]]) -> io::Result<usize> {
        let slices: Vec<io::IoSlice<'_>> = slices.iter().map(|slice| io::IoSlice::new(slice)).collect();
        Write::write_vectored(&mut self.candidate, &slices)
    }

    unsafe fn write_unchecked(&mut self, input: &[u8], index: usize, count: usize) -> io::Result<usize> {
        Write::write(&mut self.candidate, &input[index..index + count])
    }

    fn flush(&mut self) -> io::Result<()> {
        Write::flush(&mut self.candidate)
    }
}

impl UnixConditionalBackend {
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
                error: syscall_error,
                publication,
                installed: restored,
                displaced: moved.into_iter().collect(),
            }
        } else {
            NativeAttempt {
                error: syscall_error,
                publication: LocalConditionalPublicationState::Indeterminate,
                installed: restored,
                displaced: moved.into_iter().collect(),
            }
        }
    }

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

    fn indeterminate_rollback(
        &self,
        error: io::Error,
        installed: Option<ConditionalEntry>,
        displaced: Option<ConditionalEntry>,
    ) -> NativeAttempt {
        NativeAttempt {
            error: Some(error),
            publication: LocalConditionalPublicationState::Indeterminate,
            installed,
            displaced: displaced.into_iter().collect(),
        }
    }
}

impl ConditionalBackend for UnixConditionalBackend {
    fn candidate(&self) -> &ConditionalEntry {
        &self.candidate_entry
    }

    fn create_candidate_copy(&mut self) -> io::Result<ConditionalEntry> {
        let (name, mut copy) = self.namespace.create_private("candidate-copy")?;
        let entry = self.entry(LocalRecoveryFileRole::StagedCandidate, name.clone())?;
        self.recovery_entries
            .push(self.entry(LocalRecoveryFileRole::StagedCandidate, name.clone())?);
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
        copy.flush()?;
        let metadata = copy.metadata()?;
        self.candidate_copy_name = Some(name);
        self.candidate_copy_identity = Some((metadata.dev(), metadata.ino()));
        self.recovery_entries.pop();
        Ok(entry)
    }

    fn open_target(&self) -> io::Result<ConditionalEntry> {
        self.entry(LocalRecoveryFileRole::Displaced, self.namespace.target_name.clone())
    }

    fn preserve_metadata(&mut self, observed: &ConditionalEntry) -> io::Result<()> {
        metadata::preserve(observed, &self.candidate)
    }

    fn sync_candidate(&self) -> io::Result<()> {
        self.candidate.sync_all()
    }
    fn sync_entry(&self, entry: &ConditionalEntry) -> io::Result<()> {
        self.namespace.sync_entry(entry)
    }
    fn sync_namespace(&self) -> io::Result<()> {
        self.namespace.sync_all()
    }

    fn is_candidate(&self, entry: &ConditionalEntry) -> io::Result<bool> {
        let installed = entry.open_reader().map_err(io::Error::other)?;
        let candidate = self.candidate.metadata()?;
        let installed = installed.as_file().metadata()?;
        Ok(candidate.dev() == installed.dev() && candidate.ino() == installed.ino())
    }

    fn verify_entry(&self, entry: &ConditionalEntry) -> io::Result<()> {
        self.namespace.verify(entry)
    }

    fn begin_publication(&mut self) {
        self.candidate_guard = false;
    }

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
                    error: Some(error),
                    publication,
                    installed,
                    displaced: staged_alias.into_iter().collect(),
                }
            }
        }
    }

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
                let indeterminate = error.raw_os_error() == Some(libc::EIO);
                if indeterminate {
                    self.attempt_after_exchange(Some(error))
                } else {
                    NativeAttempt {
                        error: Some(error),
                        publication: LocalConditionalPublicationState::NotPublished,
                        installed: None,
                        displaced: Vec::new(),
                    }
                }
            }
        }
    }

    fn rollback(&mut self, displaced: &ConditionalEntry) -> NativeAttempt {
        if let Some(action) = self.deferred_rollback_action.take() {
            let result = match action {
                super::ConditionalHookAction::ReplaceTarget(bytes) => self.namespace.replace_target(&bytes),
                super::ConditionalHookAction::OverwriteTargetInPlace(bytes) => self.namespace.overwrite_target(&bytes),
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

    fn remove_observed(&mut self, entry: &ConditionalEntry) -> io::Result<()> {
        self.namespace.remove(entry)
    }
    fn abort(&mut self) -> io::Result<()> {
        if self.candidate_guard {
            self.namespace.remove(&self.candidate_entry)?;
            self.candidate_guard = false;
        }
        Ok(())
    }
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
        std::mem::take(&mut self.recovery_entries)
    }
    fn set_hooks(&mut self, hooks: ConditionalHooks) {
        self.hooks = hooks;
    }
    fn checkpoint(&mut self, point: ConditionalPoint) -> io::Result<()> {
        self.checkpoint_action(point)
    }
}

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
