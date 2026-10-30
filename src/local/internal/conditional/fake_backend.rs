// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

#[cfg(test)]
mod backend_error_contract_tests {
    use qubit_io::Output;

    #[test]
    fn recovery_reader_metadata_failure_keeps_candidate_and_allows_retry() {
        use crate::tests::native_io_fault_tests::NativeIoFaultGuard;
        use crate::tests::native_io_fault_tests::NativeIoPoint;

        let backend = FakeBackend::new(b"old", b"candidate");
        let path = backend.candidate().namespace_path().to_path_buf();
        let fault = NativeIoFaultGuard::fail_on(NativeIoPoint::ReaderMetadata, 1);
        let error = backend
            .candidate()
            .open_reader()
            .expect_err("opened recovery reader metadata fails");
        assert!(fault.fired());
        assert_eq!(error.operation(), crate::LocalFileOperation::OpenReader);
        assert_eq!(error.path(), Some(path.as_path()));
        assert_eq!(error.current_directory(), None);
        assert_eq!(error.io_error_kind(), io::ErrorKind::Other);
        assert!(error.to_string().contains("injected native I/O contract failure"));
        assert_eq!(fs::read(&path).expect("candidate retained"), b"candidate");
        assert_eq!(fs::read(&backend.target).expect("target preserved"), b"old");
        drop(fault);
        let mut reader = backend.candidate().open_reader().expect("retry retained candidate");
        let mut bytes = Vec::new();
        qubit_io::Streams::copy_input_to_output(&mut reader, &mut bytes).expect("read retained candidate");
        assert_eq!(bytes, b"candidate");
    }

    use std::fs;
    use std::io;

    use super::ConditionalBackend;
    use super::FakeBackend;
    use super::required_options;
    use crate::local::internal::conditional::ConditionalHooks;
    use crate::outcome::LocalConditionalPublicationState;

    #[test]
    fn fake_output_and_retained_access_report_missing_files_without_mutating_target() {
        let mut backend = FakeBackend::new(b"old", b"candidate");
        Output::write_fully(&mut backend, b"+").expect("candidate append");
        assert_eq!(fs::read(&backend.candidate).expect("candidate"), b"candidate+");
        backend.candidate().sync_file().expect("sync retained candidate");
        backend.abort().expect("fake abort is a successful no-op");
        assert_eq!(fs::read(&backend.target).expect("target unchanged"), b"old");
        fs::remove_file(&backend.candidate).expect("model external candidate removal");
        let error = Output::write_fully(&mut backend, b"x").expect_err("removed candidate cannot accept data");
        assert_eq!(error.kind(), io::ErrorKind::NotFound);
        assert_eq!(
            backend
                .candidate()
                .sync_file()
                .expect_err("removed candidate cannot sync")
                .kind(),
            io::ErrorKind::NotFound
        );
        assert_eq!(
            backend
                .candidate()
                .open_reader()
                .expect_err("removed candidate cannot read")
                .io_error_kind(),
            io::ErrorKind::NotFound
        );
    }

    #[test]
    fn failed_publication_receipts_report_only_entries_that_really_remain() {
        let mut before_first_rename = FakeBackend::new(b"old", b"candidate");
        fs::create_dir(before_first_rename.root.path().join("old")).expect("incompatible displacement destination");
        let receipt = before_first_rename.publish_content();
        assert!(receipt.error.is_some());
        assert_eq!(receipt.publication, LocalConditionalPublicationState::Indeterminate);
        let installed = receipt.installed.expect("old target still exists");
        assert_eq!(installed.namespace_path(), before_first_rename.target);
        assert!(receipt.displaced.is_empty());
        assert_eq!(
            fs::read(&before_first_rename.target).expect("old target intact"),
            b"old"
        );

        let mut after_first_rename = FakeBackend::new(b"old", b"candidate");
        fs::remove_file(&after_first_rename.candidate).expect("candidate disappeared before native publish");
        let receipt = after_first_rename.publish_content();
        assert_eq!(receipt.error.expect("rename failed").kind(), io::ErrorKind::NotFound);
        assert_eq!(receipt.publication, LocalConditionalPublicationState::Indeterminate);
        assert!(receipt.installed.is_none());
        assert_eq!(receipt.displaced.len(), 1);
        assert_eq!(
            fs::read(receipt.displaced[0].namespace_path()).expect("displaced old target retained"),
            b"old"
        );
    }

    #[cfg(feature = "digest")]
    #[test]
    fn commit_reports_candidate_digest_and_last_verification_failures_before_publication() {
        use sha2::Digest;
        use sha2::Sha256;

        use crate::error::LocalConditionalWriteStage;
        use crate::options::LocalWritePrecondition;

        for at_final_verification in [false, true] {
            let backend = FakeBackend::new(b"old", b"candidate");
            let (_lease, target) = backend.target_observer();
            let candidate = backend.candidate.clone();
            #[cfg(feature = "test-support")]
            let backend = if at_final_verification {
                backend.with_hooks(ConditionalHooks::from_test(
                    crate::test_support::TestConditionalHooks::once(
                        crate::test_support::TestConditionalPoint::AfterPrecheck,
                        move || fs::remove_file(candidate),
                    ),
                ))
            } else {
                fs::remove_file(candidate).expect("candidate disappeared before digest");
                backend
            };
            #[cfg(not(feature = "test-support"))]
            let backend = {
                if at_final_verification {
                    continue;
                }
                fs::remove_file(candidate).expect("candidate disappeared before digest");
                backend
            };
            let error = super::super::commit_conditional::<Sha256, _>(
                backend,
                LocalWritePrecondition::Content(Sha256::digest(b"old")),
                required_options(),
                9,
            )
            .expect_err("missing candidate must stop publication");
            assert_eq!(
                error.stage(),
                if at_final_verification {
                    LocalConditionalWriteStage::Precheck
                } else {
                    LocalConditionalWriteStage::VerifyCandidate
                }
            );
            assert_eq!(error.publication(), LocalConditionalPublicationState::NotPublished);
            assert_eq!(fs::read(target).expect("target intact"), b"old");
        }
    }
}

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
use crate::outcome::LocalWritePublicationMethod;
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

    fn sync_file(&self) -> io::Result<()> {
        let file = fs::OpenOptions::new().read(true).write(true).open(&self.path)?;
        file.sync_all()
    }
}

pub(super) struct FakeBackend {
    root: Arc<tempfile::TempDir>,
    target: PathBuf,
    candidate: PathBuf,
    candidate_entry: ConditionalEntry,
    events: Rc<RefCell<Vec<Event>>>,
    namespace_sync_calls: Rc<Cell<usize>>,
    hooks: ConditionalHooks,
    partial_publish: Cell<bool>,
    partial_rollback: Cell<bool>,
    fail_candidate_copy: Cell<Option<io::ErrorKind>>,
    fail_target_open: Cell<Option<io::ErrorKind>>,
    omit_displaced_receipt: Cell<bool>,
    omit_installed_receipt: Cell<bool>,
    fail_displaced_verify: Cell<bool>,
    fail_candidate_identity: Cell<bool>,
    remove_displaced_after_verify: Cell<bool>,
    remove_installed_after_identity: Cell<bool>,
    target_is_candidate: Cell<bool>,
    recovery_entries: RefCell<Vec<ConditionalEntry>>,
    absent_method: LocalWritePublicationMethod,
    content_method: LocalWritePublicationMethod,
    rollback_method: LocalWritePublicationMethod,
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
            namespace_sync_calls: Rc::new(Cell::new(0)),
            hooks: ConditionalHooks::default(),
            partial_publish: Cell::new(false),
            partial_rollback: Cell::new(false),
            fail_candidate_copy: Cell::new(None),
            fail_target_open: Cell::new(None),
            omit_displaced_receipt: Cell::new(false),
            omit_installed_receipt: Cell::new(false),
            fail_displaced_verify: Cell::new(false),
            fail_candidate_identity: Cell::new(false),
            remove_displaced_after_verify: Cell::new(false),
            remove_installed_after_identity: Cell::new(false),
            target_is_candidate: Cell::new(false),
            recovery_entries: RefCell::new(Vec::new()),
            absent_method: LocalWritePublicationMethod::AtomicRename,
            content_method: LocalWritePublicationMethod::AtomicRename,
            rollback_method: LocalWritePublicationMethod::AtomicRename,
        }
    }
    pub(super) fn events(&self) -> Rc<RefCell<Vec<Event>>> {
        Rc::clone(&self.events)
    }
    pub(super) fn namespace_sync_observer(&self) -> Rc<Cell<usize>> {
        Rc::clone(&self.namespace_sync_calls)
    }
    pub(super) fn with_hooks(mut self, hooks: ConditionalHooks) -> Self {
        self.set_hooks(hooks);
        self
    }
    pub(super) fn with_publication_methods(
        mut self,
        absent: LocalWritePublicationMethod,
        content: LocalWritePublicationMethod,
        rollback: LocalWritePublicationMethod,
    ) -> Self {
        self.absent_method = absent;
        self.content_method = content;
        self.rollback_method = rollback;
        self
    }
    pub(super) fn fail_after_first_rollback_rename_once(self) -> Self {
        self.partial_rollback.set(true);
        self
    }
    pub(super) fn fail_candidate_copy_once(self, kind: io::ErrorKind) -> Self {
        self.fail_candidate_copy.set(Some(kind));
        self
    }
    pub(super) fn fail_target_open_once(self, kind: io::ErrorKind) -> Self {
        self.fail_target_open.set(Some(kind));
        self
    }
    pub(super) fn omit_displaced_receipt_once(self) -> Self {
        self.omit_displaced_receipt.set(true);
        self
    }
    pub(super) fn omit_installed_receipt_once(self) -> Self {
        self.omit_installed_receipt.set(true);
        self
    }
    pub(super) fn fail_displaced_verify_once(self) -> Self {
        self.fail_displaced_verify.set(true);
        self
    }
    pub(super) fn fail_candidate_identity_once(self) -> Self {
        self.fail_candidate_identity.set(true);
        self
    }
    pub(super) fn remove_displaced_after_verify_once(self) -> Self {
        self.remove_displaced_after_verify.set(true);
        self
    }
    pub(super) fn remove_installed_after_identity_once(self) -> Self {
        self.remove_installed_after_identity.set(true);
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
                    let mut target = fs::OpenOptions::new().write(true).truncate(true).open(&self.target)?;
                    qubit_io::Output::write_fully(&mut target, &bytes)?;
                }
                ConditionalHookAction::CorruptCandidateCopy(bytes) => fs::write(self.root.path().join("copy"), bytes)?,
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

impl qubit_io::Output for FakeBackend {
    type Item = u8;

    unsafe fn write_unchecked(&mut self, bytes: &[u8], index: usize, count: usize) -> io::Result<usize> {
        qubit_io::Output::write(
            &mut fs::OpenOptions::new().append(true).open(&self.candidate)?,
            &bytes[index..index + count],
        )
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
        if let Some(kind) = self.fail_candidate_copy.take() {
            return Err(io::Error::new(kind, "injected candidate-copy creation failure"));
        }
        let copy = self.root.path().join("copy");
        fs::copy(&self.candidate, &copy)?;
        Ok(self.entry(copy, LocalRecoveryFileRole::StagedCandidate))
    }
    fn open_target(&self) -> io::Result<ConditionalEntry> {
        if let Some(kind) = self.fail_target_open.take() {
            return Err(io::Error::new(kind, "injected target-open failure"));
        }
        if !self.target.is_file() {
            return Err(io::Error::new(io::ErrorKind::NotFound, "fake target is absent"));
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
        self.namespace_sync_calls.set(self.namespace_sync_calls.get() + 1);
        self.inject(ConditionalPoint::SyncNamespace)
    }
    fn is_candidate(&self, entry: &ConditionalEntry) -> io::Result<bool> {
        if self.fail_candidate_identity.replace(false) {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "injected identity check failure",
            ));
        }
        let candidate = entry.namespace_path() == self.target && self.target_is_candidate.get();
        if candidate && self.remove_installed_after_identity.replace(false) {
            fs::remove_file(&self.target)?;
        }
        Ok(candidate)
    }
    fn verify_entry(&self, entry: &ConditionalEntry) -> io::Result<()> {
        if entry.role() == LocalRecoveryFileRole::Displaced && self.fail_displaced_verify.replace(false) {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "injected displaced verification failure",
            ));
        }
        if entry.namespace_path().is_file() {
            if entry.role() == LocalRecoveryFileRole::Displaced && self.remove_displaced_after_verify.replace(false) {
                fs::remove_file(entry.namespace_path())?;
            }
            Ok(())
        } else {
            Err(io::Error::new(io::ErrorKind::NotFound, "fake retained entry missing"))
        }
    }
    fn begin_publication(&mut self) -> io::Result<()> {
        Ok(())
    }
    fn publish_absent(&mut self) -> NativeAttempt {
        let publication_method = self.absent_method;
        if self.target.exists() {
            return NativeAttempt {
                publication_method,
                error: Some(io::Error::new(io::ErrorKind::AlreadyExists, "target exists")),
                publication: LocalConditionalPublicationState::NotPublished,
                installed: None,
                displaced: Vec::new(),
            };
        }
        if let Err(error) = fs::rename(&self.candidate, &self.target) {
            return NativeAttempt {
                publication_method,
                error: Some(error),
                publication: LocalConditionalPublicationState::NotPublished,
                installed: None,
                displaced: Vec::new(),
            };
        }
        self.target_is_candidate.set(true);
        NativeAttempt {
            publication_method,
            error: None,
            publication: LocalConditionalPublicationState::Published,
            installed: Some(self.entry(self.target.clone(), LocalRecoveryFileRole::StagedCandidate)),
            displaced: Vec::new(),
        }
    }
    fn publish_content(&mut self) -> NativeAttempt {
        let publication_method = self.content_method;
        let old = self.root.path().join("old");
        let operation = fs::rename(&self.target, &old).and_then(|()| fs::rename(&self.candidate, &self.target));
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
                publication_method,
                error: Some(error),
                publication: LocalConditionalPublicationState::Indeterminate,
                installed,
                displaced,
            };
        }
        self.target_is_candidate.set(true);
        if self.omit_displaced_receipt.replace(false) {
            return NativeAttempt {
                publication_method,
                error: None,
                publication: LocalConditionalPublicationState::Published,
                installed: Some(self.entry(self.target.clone(), LocalRecoveryFileRole::StagedCandidate)),
                displaced: Vec::new(),
            };
        }
        if self.omit_installed_receipt.replace(false) {
            return NativeAttempt {
                publication_method,
                error: None,
                publication: LocalConditionalPublicationState::Published,
                installed: None,
                displaced: vec![self.entry(old, LocalRecoveryFileRole::Displaced)],
            };
        }
        if self.partial_publish.replace(false) {
            return NativeAttempt {
                publication_method,
                error: Some(io::Error::other("injected partial publication")),
                publication: LocalConditionalPublicationState::Published,
                installed: Some(self.entry(self.target.clone(), LocalRecoveryFileRole::StagedCandidate)),
                displaced: vec![self.entry(old, LocalRecoveryFileRole::Displaced)],
            };
        }
        NativeAttempt {
            publication_method,
            error: None,
            publication: LocalConditionalPublicationState::Published,
            installed: Some(self.entry(self.target.clone(), LocalRecoveryFileRole::StagedCandidate)),
            displaced: vec![self.entry(old, LocalRecoveryFileRole::Displaced)],
        }
    }
    fn rollback(&mut self, displaced: &ConditionalEntry) -> NativeAttempt {
        let publication_method = self.rollback_method;
        self.events.borrow_mut().push(Event::Rollback);
        if let Err(error) = self.checkpoint(ConditionalPoint::RollbackExchange) {
            return NativeAttempt {
                publication_method,
                error: Some(error),
                publication: LocalConditionalPublicationState::Indeterminate,
                installed: self
                    .target
                    .is_file()
                    .then(|| self.entry(self.target.clone(), LocalRecoveryFileRole::Displaced)),
                displaced: Vec::new(),
            };
        }
        let discarded = self.root.path().join("rollback-displaced");
        let operation = fs::rename(&self.target, &discarded).and_then(|()| {
            if self.partial_rollback.replace(false) {
                Err(io::Error::other(
                    "injected fake rollback syscall failure after first rename",
                ))
            } else {
                fs::rename(displaced.namespace_path(), &self.target)
            }
        });
        let hook_error = self.checkpoint(ConditionalPoint::AfterRollbackSyscall).err();
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
                publication_method,
                error: Some(error),
                publication: LocalConditionalPublicationState::Indeterminate,
                installed,
                displaced,
            };
        }
        self.target_is_candidate.set(false);
        let publication = if hook_error.is_some() {
            LocalConditionalPublicationState::Indeterminate
        } else {
            LocalConditionalPublicationState::Restored
        };
        NativeAttempt {
            publication_method,
            error: hook_error,
            publication,
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
    #[cfg(any(test, all(feature = "digest", feature = "test-support")))]
    fn set_hooks(&mut self, hooks: ConditionalHooks) {
        self.hooks = hooks;
    }
    fn checkpoint(&mut self, point: ConditionalPoint) -> io::Result<()> {
        #[cfg(all(feature = "digest", feature = "test-support"))]
        self.hooks.run_callback(point)?;
        self.inject(point)?;
        match point {
            ConditionalPoint::VerifyCandidateCopy => self.events.borrow_mut().push(Event::VerifyCopy),
            ConditionalPoint::Publish => self.events.borrow_mut().push(Event::Publish),
            ConditionalPoint::VerifyInstalled => self.events.borrow_mut().push(Event::VerifyInstalled),
            ConditionalPoint::SaveComplete => self.events.borrow_mut().push(Event::SaveComplete),
            ConditionalPoint::CleanupCopy => self.events.borrow_mut().push(Event::CleanupCopy),
            _ => {}
        }
        Ok(())
    }
}

#[cfg(feature = "digest")]
#[cfg(test)]
mod publication_receipt_tests {
    use sha2::Digest;
    use sha2::Sha256;

    use super::FakeBackend;
    use super::required_options;
    use crate::local::internal::conditional::commit_conditional;
    use crate::options::LocalWritePrecondition;
    use crate::outcome::LocalWritePublicationMethod;

    #[test]
    fn selected_fake_publication_method_is_reported_verbatim() {
        let backend = FakeBackend::new(b"A", b"B").with_publication_methods(
            LocalWritePublicationMethod::AtomicExchange,
            LocalWritePublicationMethod::AtomicReplaceWithBackup,
            LocalWritePublicationMethod::AtomicRename,
        );
        let outcome = commit_conditional::<Sha256, _>(
            backend,
            LocalWritePrecondition::Content(Sha256::digest(b"A")),
            required_options(),
            1,
        )
        .expect("fake content publication should succeed");

        assert_eq!(
            LocalWritePublicationMethod::AtomicReplaceWithBackup,
            outcome.write_outcome().publication_method(),
        );
    }
}
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
