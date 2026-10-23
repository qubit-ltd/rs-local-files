// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Windows Host and Rooted conditional publication.

mod install;
mod namespace;
mod recovery;

#[cfg(test)]
mod tests;

use std::ffi::OsStr;
use std::ffi::OsString;
use std::fmt;
use std::fs::File;
use std::io;
use std::io::Read;
use std::io::Seek;
use std::io::Write;
use std::path::Path;
use std::path::PathBuf;

use namespace::FileIdentity;
use namespace::Namespace;
use namespace::file_identity;
use qubit_io::Output;
use recovery::classify_replace_failure;

use super::ConditionalBackend;
use super::ConditionalEntry;
use super::ConditionalHookAction;
use super::ConditionalHooks;
use super::ConditionalPoint;
use super::NativeAttempt;
use crate::options::LocalWriteMetadataPolicy;
use crate::options::LocalWriteOptions;
use crate::outcome::LocalConditionalPublicationState;
use crate::recovery::LocalRecoveryFileRole;

const COPY_BUFFER_SIZE: usize = 64 * 1024;

fn observe_optional_entry<T>(open: impl FnOnce() -> io::Result<T>) -> io::Result<Option<T>> {
    match open() {
        Ok(entry) => Ok(Some(entry)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

fn observe_with_error<T>(result: io::Result<Option<T>>) -> (Option<T>, Option<io::Error>) {
    match result {
        Ok(entry) => (entry, None),
        Err(error) => (None, Some(error)),
    }
}

fn combine_observation_error(
    native_error: Option<io::Error>,
    observation_error: Option<io::Error>,
) -> Option<io::Error> {
    match (native_error, observation_error) {
        (Some(native), Some(observation)) => {
            let kind = native.kind();
            Some(io::Error::new(kind, NativeReceiptError { native, observation }))
        }
        (Some(error), None) | (None, Some(error)) => Some(error),
        (None, None) => None,
    }
}

#[derive(Debug)]
struct NativeReceiptError {
    native: io::Error,
    observation: io::Error,
}

impl fmt::Display for NativeReceiptError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{}; receipt observation failed: {}",
            self.native, self.observation
        )
    }
}

impl std::error::Error for NativeReceiptError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.native)
    }
}

impl NativeReceiptError {
    #[allow(dead_code)]
    fn observation_source(&self) -> &(dyn std::error::Error + 'static) {
        &self.observation
    }
}

fn receipt_publication(
    installed_is_candidate: Option<bool>,
    candidate_present: bool,
    backup_present: bool,
    observation_failed: bool,
) -> LocalConditionalPublicationState {
    if observation_failed {
        LocalConditionalPublicationState::Indeterminate
    } else if installed_is_candidate == Some(true) {
        LocalConditionalPublicationState::Published
    } else if candidate_present && !backup_present {
        LocalConditionalPublicationState::NotPublished
    } else {
        LocalConditionalPublicationState::Indeterminate
    }
}

fn open_verified_candidate_entry(
    namespace: &Namespace,
    candidate_name: &OsStr,
    candidate_file: &File,
) -> io::Result<(ConditionalEntry, FileIdentity)> {
    let mut guard = PrivateEntryGuard::new(namespace, candidate_file)?;
    let candidate_identity = file_identity(candidate_file)?;
    let candidate_entry = open_verified_private_entry(
        namespace,
        LocalRecoveryFileRole::StagedCandidate,
        candidate_name,
        candidate_file,
        candidate_identity,
    )?;
    guard.disarm();
    Ok((candidate_entry, candidate_identity))
}

fn open_verified_private_entry(
    namespace: &Namespace,
    role: LocalRecoveryFileRole,
    name: &OsStr,
    file: &File,
    expected_identity: FileIdentity,
) -> io::Result<ConditionalEntry> {
    let observed = namespace.entry(role, name.to_os_string())?;
    let reader = observed.open_reader().map_err(io::Error::other)?;
    if file_identity(reader.as_file())? != expected_identity {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "conditional private entry name changed identity while reopening",
        ));
    }
    namespace.entry_bound_to(role, name.to_os_string(), file)
}

#[derive(Debug)]
struct PrivateEntryGuard<'a> {
    namespace: &'a Namespace,
    _file: File,
    identity: Option<FileIdentity>,
    armed: bool,
}

impl<'a> PrivateEntryGuard<'a> {
    fn new(namespace: &'a Namespace, file: &File) -> io::Result<Self> {
        let identity = match file_identity(file) {
            Ok(identity) => identity,
            Err(error) => {
                let _ = namespace.delete_created_private(file, None);
                return Err(error);
            }
        };
        let retained = match file.try_clone() {
            Ok(retained) => retained,
            Err(error) => {
                let _ = namespace.delete_created_private(file, Some(identity));
                return Err(error);
            }
        };
        Ok(Self {
            namespace,
            _file: retained,
            identity: Some(identity),
            armed: true,
        })
    }

    fn disarm(&mut self) {
        self.armed = false;
    }
}

impl Drop for PrivateEntryGuard<'_> {
    fn drop(&mut self) {
        if !self.armed {
            return;
        }
        let Some(identity) = self.identity else {
            return;
        };
        let _ = self.namespace.delete_created_private(&self._file, Some(identity));
    }
}

#[derive(Debug)]
pub(crate) struct WindowsConditionalBackend {
    namespace: Namespace,
    candidate_name: OsString,
    candidate_file: Option<File>,
    candidate_entry: ConditionalEntry,
    candidate_identity: FileIdentity,
    candidate_copy: Option<(OsString, FileIdentity)>,
    candidate_guard: bool,
    options: LocalWriteOptions,
    hooks: ConditionalHooks,
    recovery_entries: Vec<ConditionalEntry>,
    recovery_containers: Vec<PathBuf>,
}

impl WindowsConditionalBackend {
    pub(crate) fn open_host(path: &Path, options: &LocalWriteOptions) -> io::Result<Self> {
        Self::open(Namespace::open_host(path, options)?, options)
    }

    pub(crate) fn open_rooted(
        root: File,
        relative: &Path,
        diagnostic: &Path,
        options: &LocalWriteOptions,
    ) -> io::Result<Self> {
        Self::open(Namespace::open_rooted(root, relative, diagnostic, options)?, options)
    }

    fn open(namespace: Namespace, options: &LocalWriteOptions) -> io::Result<Self> {
        let (candidate_name, candidate_file) = namespace.create_private("candidate")?;
        let (candidate_entry, candidate_identity) =
            open_verified_candidate_entry(&namespace, &candidate_name, &candidate_file)?;
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
        })
    }

    fn entry(&self, role: LocalRecoveryFileRole, name: OsString) -> io::Result<ConditionalEntry> {
        self.namespace.entry(role, name)
    }

    fn entry_if_present(&self, role: LocalRecoveryFileRole, name: &OsStr) -> io::Result<Option<ConditionalEntry>> {
        observe_optional_entry(|| self.entry(role, name.to_os_string()))
    }

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

    fn candidate_receipt(&self, error: io::Error) -> NativeAttempt {
        let (candidate, observation_error) = observe_with_error(self.observe_candidate_entry());
        let observation_failed = observation_error.is_some();
        NativeAttempt {
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

    fn collect_native_receipt(
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
        let (installed_is_candidate, identity_error) = match installed.as_ref() {
            Some(entry) => match entry
                .open_reader()
                .map_err(io::Error::other)
                .and_then(|reader| file_identity(reader.as_file()))
            {
                Ok(identity) => (Some(identity == self.candidate_identity), None),
                Err(error) => (None, Some(error)),
            },
            None => (Some(false), None),
        };
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
        if let Some(candidate) = candidate {
            if installed_is_candidate != Some(true) {
                displaced.push(candidate);
            }
        }
        NativeAttempt {
            error: combine_observation_error(error, observation_error),
            publication,
            installed,
            displaced,
        }
    }

    fn collect_rollback_receipt(
        &self,
        error: Option<io::Error>,
        expected_restored: FileIdentity,
        backup_container: &Path,
        backup_path: &Path,
    ) -> NativeAttempt {
        let _failure_class = error
            .as_ref()
            .map(|source| classify_replace_failure(source.raw_os_error()));
        let (installed, mut observation_error) =
            observe_with_error(self.entry_if_present(LocalRecoveryFileRole::Displaced, self.namespace.target_name()));
        let (backup, backup_error) = observe_with_error(observe_optional_entry(|| {
            self.namespace
                .entry_in_container(LocalRecoveryFileRole::RollbackDisplaced, backup_container, backup_path)
        }));
        observation_error = observation_error.or(backup_error);
        let (installed_identity, identity_error) = match installed.as_ref() {
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
        observation_error = observation_error.or(identity_error);
        let publication = if observation_error.is_some() {
            LocalConditionalPublicationState::Indeterminate
        } else if installed_identity == Some(expected_restored) {
            LocalConditionalPublicationState::Restored
        } else if installed_identity == Some(self.candidate_identity) && backup.is_none() {
            LocalConditionalPublicationState::NotPublished
        } else {
            LocalConditionalPublicationState::Indeterminate
        };
        NativeAttempt {
            error: combine_observation_error(error, observation_error),
            publication,
            installed,
            displaced: backup.into_iter().collect(),
        }
    }

    fn make_backup(&mut self) -> io::Result<(PathBuf, PathBuf)> {
        let (container, backup) = self.namespace.backup_container()?;
        self.recovery_containers.push(container.clone());
        Ok((container, backup))
    }

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

    fn write_target_for_hook(&self, bytes: &[u8], in_place: bool) -> io::Result<()> {
        use std::io::Write as _;
        let path = self.namespace.target_path();
        if in_place {
            let mut file = std::fs::OpenOptions::new().write(true).truncate(true).open(path)?;
            file.write_all(bytes)
        } else {
            let mut file = std::fs::OpenOptions::new().write(true).create_new(true).open(path)?;
            file.write_all(bytes)
        }
    }

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
                return self.collect_native_receipt(Some(error), Some(&container), Some(&backup));
            }
        };
        match install::replace_with_backup(self.namespace.target_path(), &candidate_path, &backup) {
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
        }
    }

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
}

impl Output for WindowsConditionalBackend {
    type Item = u8;

    fn write_vectored(&mut self, slices: &[&[u8]]) -> io::Result<usize> {
        let file = self
            .candidate_file
            .as_mut()
            .ok_or_else(|| io::Error::new(io::ErrorKind::BrokenPipe, "conditional candidate is closed"))?;
        let slices: Vec<io::IoSlice<'_>> = slices.iter().map(|slice| io::IoSlice::new(slice)).collect();
        Write::write_vectored(file, &slices)
    }

    unsafe fn write_unchecked(&mut self, input: &[u8], index: usize, count: usize) -> io::Result<usize> {
        let file = self
            .candidate_file
            .as_mut()
            .ok_or_else(|| io::Error::new(io::ErrorKind::BrokenPipe, "conditional candidate is closed"))?;
        Write::write(file, &input[index..index + count])
    }

    fn flush(&mut self) -> io::Result<()> {
        Write::flush(
            self.candidate_file
                .as_mut()
                .ok_or_else(|| io::Error::new(io::ErrorKind::BrokenPipe, "conditional candidate is closed"))?,
        )
    }
}

impl ConditionalBackend for WindowsConditionalBackend {
    fn candidate(&self) -> &ConditionalEntry {
        &self.candidate_entry
    }

    fn create_candidate_copy(&mut self) -> io::Result<ConditionalEntry> {
        let (name, mut destination) = match self.namespace.create_private("candidate-copy") {
            Ok(created) => created,
            Err(error) => {
                self.retain_original_candidate();
                return Err(error);
            }
        };
        let mut guard = PrivateEntryGuard::new(&self.namespace, &destination)?;
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
                destination.write_all(&buffer[..read])?;
            }
            Write::flush(&mut destination)?;
            let identity = file_identity(&destination)?;
            let entry = open_verified_private_entry(
                &self.namespace,
                LocalRecoveryFileRole::StagedCandidate,
                &name,
                &destination,
                identity,
            )?;
            Ok((entry, identity))
        })();
        match result {
            Ok((entry, identity)) => {
                self.candidate_copy = Some((name, identity));
                guard.disarm();
                drop(guard);
                Ok(entry)
            }
            Err(error) => {
                drop(guard);
                self.retain_original_candidate();
                Err(error)
            }
        }
    }

    fn open_target(&self) -> io::Result<ConditionalEntry> {
        let entry = self.entry(
            LocalRecoveryFileRole::Displaced,
            self.namespace.target_name().to_os_string(),
        )?;
        Ok(entry)
    }

    fn preserve_metadata(&mut self, observed: &ConditionalEntry) -> io::Result<()> {
        self.namespace.verify_entry(observed)?;
        // ReplaceFileW carries the existing destination's attributes and ACL
        // onto the replacement. Applying read-only permissions to the staged
        // file here can itself prevent ReplaceFileW from replacing it.
        Ok(())
    }

    fn sync_candidate(&self) -> io::Result<()> {
        self.candidate_file
            .as_ref()
            .ok_or_else(|| io::Error::new(io::ErrorKind::BrokenPipe, "conditional candidate is closed"))?
            .sync_all()
    }

    fn sync_entry(&self, entry: &ConditionalEntry) -> io::Result<()> {
        self.namespace.sync_entry(entry)
    }

    fn sync_namespace(&self) -> io::Result<()> {
        if self.options.durability() == crate::policy::LocalDurabilityRequirement::NotRequired {
            return Ok(());
        }
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "Windows directory durability is not guaranteed by the conditional backend",
        ))
    }

    fn is_candidate(&self, entry: &ConditionalEntry) -> io::Result<bool> {
        self.verify_entry(entry)?;
        let reader = entry.open_reader().map_err(io::Error::other)?;
        Ok(file_identity(reader.as_file())? == self.candidate_identity)
    }

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

    fn begin_publication(&mut self) {
        self.candidate_guard = false;
        self.candidate_file.take();
    }

    fn publish_absent(&mut self) -> NativeAttempt {
        match install::install_absent(&self.namespace, &self.candidate_name) {
            Ok(()) => {
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
                    error: observation_error,
                    publication,
                    installed,
                    displaced: Vec::new(),
                }
            }
            Err(error) => self.collect_native_receipt(Some(error), None, None),
        }
    }

    fn publish_content(&mut self) -> NativeAttempt {
        if self.namespace.is_rooted() {
            return self.candidate_receipt(io::Error::new(
                io::ErrorKind::Unsupported,
                "Windows Rooted conditional Content is unsupported",
            ));
        }
        self.publish_host_content()
    }

    fn rollback(&mut self, displaced: &ConditionalEntry) -> NativeAttempt {
        if self.namespace.is_rooted() || self.options.metadata_policy() != LocalWriteMetadataPolicy::PreserveExisting {
            return NativeAttempt {
                error: Some(io::Error::new(
                    io::ErrorKind::Unsupported,
                    "Windows conditional rollback is unsupported for this namespace",
                )),
                publication: LocalConditionalPublicationState::NotPublished,
                installed: None,
                displaced: Vec::new(),
            };
        }
        if let Err(error) = self.namespace.verify_entry(displaced) {
            return NativeAttempt {
                error: Some(error),
                publication: LocalConditionalPublicationState::NotPublished,
                installed: None,
                displaced: Vec::new(),
            };
        }
        let expected_restored = match displaced
            .open_reader()
            .map_err(io::Error::other)
            .and_then(|reader| file_identity(reader.as_file()))
        {
            Ok(identity) => identity,
            Err(error) => {
                return NativeAttempt {
                    error: Some(error),
                    publication: LocalConditionalPublicationState::NotPublished,
                    installed: None,
                    displaced: Vec::new(),
                };
            }
        };
        let displaced_path = displaced.namespace_path();
        let (container, backup) = match self.make_backup() {
            Ok(paths) => paths,
            Err(error) => {
                return NativeAttempt {
                    error: Some(error),
                    publication: LocalConditionalPublicationState::NotPublished,
                    installed: None,
                    displaced: Vec::new(),
                };
            }
        };
        if let Err(error) = self.checkpoint_action(ConditionalPoint::RollbackExchange) {
            return self.collect_rollback_receipt(Some(error), expected_restored, &container, &backup);
        }
        match install::replace_with_backup(self.namespace.target_path(), displaced_path, &backup) {
            Ok(()) => {
                let hook_error = self.checkpoint_action(ConditionalPoint::AfterRollbackSyscall).err();
                let (installed, installed_error) = observe_with_error(
                    self.entry_if_present(LocalRecoveryFileRole::Displaced, self.namespace.target_name()),
                );
                let (replacement_displaced, displaced_error) = observe_with_error(observe_optional_entry(|| {
                    self.namespace
                        .entry_in_container(LocalRecoveryFileRole::RollbackDisplaced, &container, &backup)
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
            Err(error) => {
                let hook_error = self.checkpoint_action(ConditionalPoint::AfterRollbackSyscall).err();
                self.collect_rollback_receipt(Some(error).or(hook_error), expected_restored, &container, &backup)
            }
        }
    }

    fn remove_observed(&mut self, entry: &ConditionalEntry) -> io::Result<()> {
        self.namespace.remove(entry)?;
        if self
            .candidate_copy
            .as_ref()
            .is_some_and(|(name, _)| entry.namespace_path().file_name() == Some(name.as_os_str()))
        {
            self.candidate_copy = None;
        }
        if let Some(parent) = entry.namespace_path().parent() {
            if self.recovery_containers.iter().any(|container| container == parent) {
                self.namespace.remove_container(parent)?;
                self.recovery_containers.retain(|container| container != parent);
            }
        }
        Ok(())
    }

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

    fn take_recovery_entries(&mut self) -> Vec<ConditionalEntry> {
        std::mem::take(&mut self.recovery_entries)
    }
    fn set_hooks(&mut self, hooks: ConditionalHooks) {
        self.hooks = hooks;
    }
    fn checkpoint(&mut self, point: ConditionalPoint) -> io::Result<()> {
        self.checkpoint_action(point)
    }
}

impl Drop for WindowsConditionalBackend {
    fn drop(&mut self) {
        if self.candidate_guard {
            let _ = self.abort();
        }
    }
}
