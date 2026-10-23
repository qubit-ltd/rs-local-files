// =============================================================================

use std::cell::Cell;
use std::cell::RefCell;
use std::fs;
use std::io;
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;

use crate::LocalFileReader;
use crate::LocalResult;
use crate::local::internal::conditional::ConditionalBackend;
use crate::local::internal::conditional::ConditionalEntry;
use crate::local::internal::conditional::ConditionalHookAction;
use crate::local::internal::conditional::ConditionalHooks;
use crate::local::internal::conditional::ConditionalPoint;
use crate::local::internal::conditional::NativeAttempt;
use crate::options::LocalWriteMode;
use crate::options::LocalWriteOptions;
use crate::outcome::LocalConditionalPublicationState;
use crate::policy::LocalAtomicityRequirement;
use crate::policy::LocalDurabilityRequirement;
use crate::recovery::LocalRecoveryFileRole;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum Event {
    Rollback,
    VerifyCopy,
    Publish,
    VerifyInstalled,
    RequiredSync,
    SaveComplete,
    CleanupCopy,
}

#[derive(Debug)]
struct Access {
    path: PathBuf,
    _lease: Arc<tempfile::TempDir>,
}

impl super::RecoveryAccess for Access {
    fn open_reader(&self) -> LocalResult<LocalFileReader> {
        let file = fs::File::open(&self.path).map_err(|error| {
            crate::LocalFileError::from_io(
                crate::LocalFileOperation::OpenReader,
                Some(self.path.clone()),
                None,
                error,
            )
        })?;
        LocalFileReader::from_file(file).map_err(|error| {
            crate::LocalFileError::from_io(
                crate::LocalFileOperation::OpenReader,
                Some(self.path.clone()),
                None,
                error,
            )
        })
    }
}

pub(super) struct FakeBackend {
    root: Arc<tempfile::TempDir>,
    target: PathBuf,
    candidate: PathBuf,
    candidate_entry: ConditionalEntry,
    events: Rc<RefCell<Vec<Event>>>,
    hooks: ConditionalHooks,
    partial_publish: Cell<bool>,
    target_is_candidate: Cell<bool>,
    recovery_entries: RefCell<Vec<ConditionalEntry>>,
}

impl FakeBackend {
    pub(super) fn new(old: &[u8], candidate: &[u8]) -> Self {
        let root = tempfile::tempdir().expect("tempdir");
        let root = Arc::new(root);
        let target = root.path().join("target");
        let candidate_path = root.path().join("candidate");
        fs::write(&target, old).expect("write old");
        fs::write(&candidate_path, candidate).expect("write candidate");
        let candidate_entry = ConditionalEntry::new(
            LocalRecoveryFileRole::StagedCandidate,
            candidate_path.clone(),
            Some(candidate_path.clone()),
            Box::new(Access {
                path: candidate_path.clone(),
                _lease: Arc::clone(&root),
            }),
        );
        Self {
            root,
            target,
            candidate: candidate_path,
            candidate_entry,
            events: Rc::new(RefCell::new(Vec::new())),
            hooks: ConditionalHooks::default(),
            partial_publish: Cell::new(false),
            target_is_candidate: Cell::new(false),
            recovery_entries: RefCell::new(Vec::new()),
        }
    }
    pub(super) fn events(&self) -> Rc<RefCell<Vec<Event>>> {
        Rc::clone(&self.events)
    }
    pub(super) fn with_hooks(mut self, hooks: ConditionalHooks) -> Self {
        self.set_hooks(hooks);
        self
    }
    pub(super) fn target_observer(&self) -> (Arc<tempfile::TempDir>, PathBuf) {
        (Arc::clone(&self.root), self.target.clone())
    }
    pub(super) fn remove_target(&self) {
        fs::remove_file(&self.target).expect("remove fake target");
    }
    fn entry(&self, path: PathBuf, role: LocalRecoveryFileRole) -> ConditionalEntry {
        ConditionalEntry::new(
            role,
            path.clone(),
            Some(path.clone()),
            Box::new(Access {
                path,
                _lease: Arc::clone(&self.root),
            }),
        )
    }

    fn inject(&self, point: ConditionalPoint) -> io::Result<()> {
        if let Some(action) = self.hooks.take(point) {
            match action {
                ConditionalHookAction::Noop => {}
                ConditionalHookAction::Fail(kind) => {
                    return Err(io::Error::new(kind, "injected checkpoint failure"));
                }
                ConditionalHookAction::ReplaceTarget(bytes) => {
                    let replacement = self.root.path().join("replacement");
                    fs::write(&replacement, bytes)?;
                    if self.target.exists() {
                        fs::remove_file(&self.target)?;
                    }
                    fs::rename(replacement, &self.target)?;
                    self.target_is_candidate.set(false);
                }
                ConditionalHookAction::OverwriteTargetInPlace(bytes) => {
                    let mut target = fs::OpenOptions::new()
                        .write(true)
                        .truncate(true)
                        .open(&self.target)?;
                    std::io::Write::write_all(&mut target, &bytes)?;
                }
                ConditionalHookAction::CorruptCandidateCopy(bytes) => {
                    fs::write(self.root.path().join("copy"), bytes)?
                }
                ConditionalHookAction::PartialCandidateCopy(bytes) => {
                    let path = self.root.path().join("copy");
                    fs::write(&path, bytes)?;
                    self.recovery_entries
                        .borrow_mut()
                        .push(self.entry(path, LocalRecoveryFileRole::StagedCandidate));
                    return Err(io::Error::new(
                        io::ErrorKind::PermissionDenied,
                        "injected partial copy failure",
                    ));
                }
                ConditionalHookAction::PartialPublish => self.partial_publish.set(true),
            }
        }
        Ok(())
    }
}

pub(super) fn required_options() -> LocalWriteOptions {
    LocalWriteOptions::new(LocalWriteMode::CreateOrReplace)
        .with_atomicity(LocalAtomicityRequirement::Required)
        .with_durability(LocalDurabilityRequirement::Required)
}

impl std::io::Write for FakeBackend {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        fs::OpenOptions::new()
            .append(true)
            .open(&self.candidate)?
            .write(bytes)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.inject(ConditionalPoint::Flush)
    }
}
impl ConditionalBackend for FakeBackend {
    fn candidate(&self) -> &ConditionalEntry {
        &self.candidate_entry
    }
    fn create_candidate_copy(&mut self) -> io::Result<ConditionalEntry> {
        let copy = self.root.path().join("copy");
        fs::copy(&self.candidate, &copy)?;
        Ok(self.entry(copy, LocalRecoveryFileRole::StagedCandidate))
    }
    fn open_target(&self) -> io::Result<ConditionalEntry> {
        if !self.target.is_file() {
            return Err(io::Error::new(
                io::ErrorKind::NotFound,
                "fake target is absent",
            ));
        }
        Ok(self.entry(self.target.clone(), LocalRecoveryFileRole::Displaced))
    }
    fn preserve_metadata(&mut self, _observed: &ConditionalEntry) -> io::Result<()> {
        Ok(())
    }
    fn sync_candidate(&self) -> io::Result<()> {
        self.events.borrow_mut().push(Event::RequiredSync);
        self.inject(ConditionalPoint::SyncCandidate)
    }
    fn sync_entry(&self, entry: &ConditionalEntry) -> io::Result<()> {
        self.events.borrow_mut().push(Event::RequiredSync);
        let point = match entry.role() {
            LocalRecoveryFileRole::CandidateCopy => ConditionalPoint::SyncEntry,
            LocalRecoveryFileRole::StagedCandidate => ConditionalPoint::SyncInstalled,
            LocalRecoveryFileRole::Displaced | LocalRecoveryFileRole::RollbackDisplaced => {
                ConditionalPoint::SyncRollback
            }
        };
        self.inject(point)
    }
    fn sync_namespace(&self) -> io::Result<()> {
        self.inject(ConditionalPoint::SyncNamespace)
    }
    fn is_candidate(&self, entry: &ConditionalEntry) -> io::Result<bool> {
        Ok(entry.namespace_path() == self.target && self.target_is_candidate.get())
    }
    fn verify_entry(&self, entry: &ConditionalEntry) -> io::Result<()> {
        if entry.namespace_path().is_file() {
            Ok(())
        } else {
            Err(io::Error::new(
                io::ErrorKind::NotFound,
                "fake retained entry missing",
            ))
        }
    }
    fn begin_publication(&mut self) {}
    fn publish_absent(&mut self) -> NativeAttempt {
        if self.target.exists() {
            return NativeAttempt {
                error: Some(io::Error::new(
                    io::ErrorKind::AlreadyExists,
                    "target exists",
                )),
                publication: LocalConditionalPublicationState::NotPublished,
                installed: None,
                displaced: Vec::new(),
            };
        }
        if let Err(error) = fs::rename(&self.candidate, &self.target) {
            return NativeAttempt {
                error: Some(error),
                publication: LocalConditionalPublicationState::NotPublished,
                installed: None,
                displaced: Vec::new(),
            };
        }
        self.target_is_candidate.set(true);
        NativeAttempt {
            error: None,
            publication: LocalConditionalPublicationState::Published,
            installed: Some(
                self.entry(self.target.clone(), LocalRecoveryFileRole::StagedCandidate),
            ),
            displaced: Vec::new(),
        }
    }
    fn publish_content(&mut self) -> NativeAttempt {
        let old = self.root.path().join("old");
        let operation =
            fs::rename(&self.target, &old).and_then(|()| fs::rename(&self.candidate, &self.target));
        if let Err(error) = operation {
            self.target_is_candidate.set(false);
            let installed = self
                .target
                .is_file()
                .then(|| self.entry(self.target.clone(), LocalRecoveryFileRole::StagedCandidate));
            let displaced = old
                .is_file()
                .then(|| self.entry(old, LocalRecoveryFileRole::Displaced))
                .into_iter()
                .collect();
            return NativeAttempt {
                error: Some(error),
                publication: LocalConditionalPublicationState::Indeterminate,
                installed,
                displaced,
            };
        }
        self.target_is_candidate.set(true);
        if self.partial_publish.replace(false) {
            return NativeAttempt {
                error: Some(io::Error::other("injected partial publication")),
                publication: LocalConditionalPublicationState::Published,
                installed: Some(
                    self.entry(self.target.clone(), LocalRecoveryFileRole::StagedCandidate),
                ),
                displaced: vec![self.entry(old, LocalRecoveryFileRole::Displaced)],
            };
        }
        NativeAttempt {
            error: None,
            publication: LocalConditionalPublicationState::Published,
            installed: Some(
                self.entry(self.target.clone(), LocalRecoveryFileRole::StagedCandidate),
            ),
            displaced: vec![self.entry(old, LocalRecoveryFileRole::Displaced)],
        }
    }
    fn rollback(&mut self, displaced: &ConditionalEntry) -> NativeAttempt {
        self.events.borrow_mut().push(Event::Rollback);
        let discarded = self.root.path().join("rollback-displaced");
        let operation = fs::rename(&self.target, &discarded)
            .and_then(|()| fs::rename(displaced.namespace_path(), &self.target));
        if let Err(error) = operation {
            self.target_is_candidate.set(false);
            let installed = self
                .target
                .is_file()
                .then(|| self.entry(self.target.clone(), LocalRecoveryFileRole::Displaced));
            let displaced = discarded
                .is_file()
                .then(|| self.entry(discarded, LocalRecoveryFileRole::RollbackDisplaced))
                .into_iter()
                .collect();
            return NativeAttempt {
                error: Some(error),
                publication: LocalConditionalPublicationState::Indeterminate,
                installed,
                displaced,
            };
        }
        self.target_is_candidate.set(false);
        NativeAttempt {
            error: None,
            publication: LocalConditionalPublicationState::Restored,
            installed: Some(self.entry(self.target.clone(), LocalRecoveryFileRole::Displaced)),
            displaced: vec![self.entry(discarded, LocalRecoveryFileRole::RollbackDisplaced)],
        }
    }
    fn remove_observed(&mut self, entry: &ConditionalEntry) -> io::Result<()> {
        fs::remove_file(entry.namespace_path())
    }
    fn abort(&mut self) -> io::Result<()> {
        Ok(())
    }
    fn take_recovery_entries(&mut self) -> Vec<ConditionalEntry> {
        std::mem::take(self.recovery_entries.get_mut())
    }
    fn set_hooks(&mut self, hooks: ConditionalHooks) {
        self.hooks = hooks;
    }
    fn checkpoint(&mut self, point: ConditionalPoint) -> io::Result<()> {
        self.inject(point)?;
        match point {
            ConditionalPoint::VerifyCandidateCopy => {
                self.events.borrow_mut().push(Event::VerifyCopy)
            }
            ConditionalPoint::Publish => self.events.borrow_mut().push(Event::Publish),
            ConditionalPoint::VerifyInstalled => {
                self.events.borrow_mut().push(Event::VerifyInstalled)
            }
            ConditionalPoint::SaveComplete => self.events.borrow_mut().push(Event::SaveComplete),
            ConditionalPoint::CleanupCopy => self.events.borrow_mut().push(Event::CleanupCopy),
            _ => {}
        }
        Ok(())
    }
}
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
