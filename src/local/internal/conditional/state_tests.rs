// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

#[cfg(feature = "digest")]
mod conditional_state_tests {
    use digest::FixedOutput;
    use digest::HashMarker;
    use digest::OutputSizeUser;
    use digest::Update;
    use digest::consts::U32;
    use qubit_io::Input;
    use sha2::Digest;
    use sha2::Sha256;

    use crate::options::LocalWritePrecondition;

    struct NoCloneSha256(Sha256);
    impl Default for NoCloneSha256 {
        fn default() -> Self {
            Self(Sha256::new())
        }
    }
    impl HashMarker for NoCloneSha256 {}
    impl OutputSizeUser for NoCloneSha256 {
        type OutputSize = U32;
    }
    impl Update for NoCloneSha256 {
        fn update(&mut self, data: &[u8]) {
            Update::update(&mut self.0, data);
        }
    }
    impl FixedOutput for NoCloneSha256 {
        fn finalize_into(self, out: &mut digest::Output<Self>) {
            let result = self.0.finalize();
            out.copy_from_slice(&result);
        }
    }

    fn recovery_bytes(file: &crate::recovery::LocalRecoveryFile) -> Vec<u8> {
        use std::io::Read;
        let reader = file.open_reader().expect("recovery entry opens");
        let mut bytes = Vec::new();
        reader
            .into_std_read()
            .read_to_end(&mut bytes)
            .expect("recovery entry reads");
        bytes
    }

    #[test]
    fn conditional_state_copy_precedes_publication() {
        let backend = super::super::fake_backend::FakeBackend::new(b"A", b"B");
        let events = backend.events();
        let result = super::super::commit::commit_conditional::<Sha256, _>(
            backend,
            LocalWritePrecondition::Content(Sha256::digest(b"A")),
            super::super::fake_backend::required_options(),
            1,
        )
        .unwrap();
        let events = events.borrow();
        let copy = events
            .iter()
            .position(|event| *event == super::super::fake_backend::Event::VerifyCopy)
            .unwrap();
        let publish = events
            .iter()
            .position(|event| *event == super::super::fake_backend::Event::Publish)
            .unwrap();
        assert!(copy < publish);
        assert_eq!(result.digest(), &Sha256::digest(b"B"));
    }

    #[test]
    fn conditional_state_flush_failure_reports_prepare_stage() {
        use super::super::ConditionalHooks;
        use super::super::ConditionalPoint;

        let backend = super::super::fake_backend::FakeBackend::new(b"A", b"B").with_hooks(
            ConditionalHooks::fail_once(
                ConditionalPoint::Flush,
                std::io::ErrorKind::PermissionDenied,
            ),
        );
        let (_lease, target_path) = backend.target_observer();
        let error = super::super::commit::commit_conditional::<Sha256, _>(
            backend,
            LocalWritePrecondition::Content(Sha256::digest(b"A")),
            super::super::fake_backend::required_options(),
            1,
        )
        .expect_err("flush failure must stop before candidate verification");
        assert_eq!(
            error.stage(),
            crate::error::LocalConditionalWriteStage::Prepare
        );
        assert_eq!(
            error.reason(),
            crate::error::LocalConditionalFailureReason::Io
        );
        assert_eq!(
            error.publication(),
            crate::outcome::LocalConditionalPublicationState::NotPublished
        );
        assert_eq!(
            error.error().kind(),
            crate::LocalFileErrorKind::PermissionDenied
        );
        assert!(error.candidate().is_none());
        assert_eq!(std::fs::read(target_path).expect("target unchanged"), b"A");
    }

    #[test]
    fn conditional_state_precondition_failure_retains_candidate_copy() {
        let backend = super::super::fake_backend::FakeBackend::new(b"A", b"B");
        let (_lease, target_path) = backend.target_observer();
        let error = super::super::commit::commit_conditional::<Sha256, _>(
            backend,
            LocalWritePrecondition::Content(Sha256::digest(b"other")),
            super::super::fake_backend::required_options(),
            1,
        )
        .expect_err("mismatched content precondition must fail");
        assert_eq!(
            error.reason(),
            crate::error::LocalConditionalFailureReason::PreconditionMismatch
        );
        assert_eq!(
            error.stage(),
            crate::error::LocalConditionalWriteStage::Precheck
        );
        assert_eq!(
            error.publication(),
            crate::outcome::LocalConditionalPublicationState::NotPublished
        );
        let copy = error.candidate().expect("verified copy must be retained");
        assert_eq!(
            copy.role(),
            crate::recovery::LocalRecoveryFileRole::CandidateCopy
        );
        let reader = copy.open_reader().expect("retained copy remains readable");
        use std::io::Read;
        let mut bytes = Vec::new();
        reader
            .into_std_read()
            .read_to_end(&mut bytes)
            .expect("read recovery content");
        assert_eq!(bytes, b"B");
        assert_eq!(
            std::fs::read(target_path).expect("precondition target remains"),
            b"A"
        );
    }

    #[test]
    fn conditional_state_supports_digest_without_clone() {
        let backend = super::super::fake_backend::FakeBackend::new(b"A", b"B");
        let result = super::super::commit::commit_conditional::<NoCloneSha256, _>(
            backend,
            LocalWritePrecondition::Content(NoCloneSha256::digest(b"A")),
            super::super::fake_backend::required_options(),
            1,
        )
        .expect("non-Clone digest commits");
        assert_eq!(result.digest().as_slice(), Sha256::digest(b"B").as_slice());
    }

    #[test]
    fn conditional_state_copy_verification_failure_retains_partial_copy() {
        use super::super::ConditionalHookAction;
        use super::super::ConditionalHooks;
        use super::super::ConditionalPoint;

        let backend = super::super::fake_backend::FakeBackend::new(b"A", b"B").with_hooks(
            ConditionalHooks::sequence(vec![(
                ConditionalPoint::CandidateCopyCreated,
                ConditionalHookAction::CorruptCandidateCopy(b"broken".to_vec()),
            )]),
        );
        let (_lease, target_path) = backend.target_observer();
        let error = super::super::commit::commit_conditional::<Sha256, _>(
            backend,
            LocalWritePrecondition::Content(Sha256::digest(b"A")),
            super::super::fake_backend::required_options(),
            1,
        )
        .expect_err("damaged copy must not publish");
        assert_eq!(
            error.stage(),
            crate::error::LocalConditionalWriteStage::VerifyCandidateCopy
        );
        assert_eq!(
            error.publication(),
            crate::outcome::LocalConditionalPublicationState::NotPublished
        );
        assert!(error.candidate().is_none());
        assert_eq!(
            error.recovery_files()[0].role(),
            crate::recovery::LocalRecoveryFileRole::StagedCandidate
        );
        assert_eq!(recovery_bytes(&error.recovery_files()[0]), b"broken");
        assert_eq!(std::fs::read(target_path).expect("target unchanged"), b"A");
        assert_eq!(
            error.reason(),
            crate::error::LocalConditionalFailureReason::Io
        );
        assert_eq!(
            error.error().kind(),
            crate::LocalFileErrorKind::DataCorruption
        );
    }

    #[test]
    fn conditional_state_prepublication_sync_failure_preserves_verified_copy() {
        use super::super::ConditionalHooks;
        use super::super::ConditionalPoint;

        let backend = super::super::fake_backend::FakeBackend::new(b"A", b"B").with_hooks(
            ConditionalHooks::fail_once(
                ConditionalPoint::SyncCandidate,
                std::io::ErrorKind::PermissionDenied,
            ),
        );
        let error = super::super::commit::commit_conditional::<Sha256, _>(
            backend,
            LocalWritePrecondition::Content(Sha256::digest(b"A")),
            super::super::fake_backend::required_options(),
            1,
        )
        .expect_err("required candidate sync failure must stop publication");
        assert_eq!(
            error.stage(),
            crate::error::LocalConditionalWriteStage::Sync
        );
        assert_eq!(
            error.reason(),
            crate::error::LocalConditionalFailureReason::Io
        );
        assert_eq!(
            error.error().kind(),
            crate::error::LocalFileErrorKind::PermissionDenied
        );
        assert!(error.candidate().is_some());
    }

    #[test]
    fn conditional_state_postpublication_sync_failure_transfers_every_entry() {
        use super::super::ConditionalHookAction;
        use super::super::ConditionalHooks;
        use super::super::ConditionalPoint;

        let hooks = ConditionalHooks::sequence(vec![
            (ConditionalPoint::SyncNamespace, ConditionalHookAction::Noop),
            (
                ConditionalPoint::SyncNamespace,
                ConditionalHookAction::Fail(std::io::ErrorKind::PermissionDenied),
            ),
        ]);
        let backend = super::super::fake_backend::FakeBackend::new(b"A", b"B").with_hooks(hooks);
        let (_lease, target_path) = backend.target_observer();
        let error = super::super::commit::commit_conditional::<Sha256, _>(
            backend,
            LocalWritePrecondition::Content(Sha256::digest(b"A")),
            super::super::fake_backend::required_options(),
            1,
        )
        .expect_err("required postpublication namespace sync must fail");
        assert_eq!(
            error.reason(),
            crate::error::LocalConditionalFailureReason::Io
        );
        assert_eq!(
            error.stage(),
            crate::error::LocalConditionalWriteStage::Sync
        );
        assert_eq!(
            error.publication(),
            crate::outcome::LocalConditionalPublicationState::Published
        );
        assert_eq!(
            std::fs::read(target_path).expect("published B remains"),
            b"B"
        );
        assert_eq!(
            recovery_bytes(error.candidate().expect("verified B copy")),
            b"B"
        );
        let installed = error
            .recovery_files()
            .iter()
            .find(|file| file.role() == crate::recovery::LocalRecoveryFileRole::StagedCandidate)
            .expect("installed target transferred");
        assert_eq!(recovery_bytes(installed), b"B");
        let displaced = error
            .recovery_files()
            .iter()
            .find(|file| file.role() == crate::recovery::LocalRecoveryFileRole::Displaced)
            .expect("old target transferred");
        assert_eq!(recovery_bytes(displaced), b"A");
        assert_eq!(
            error.error().kind(),
            crate::LocalFileErrorKind::PermissionDenied
        );
    }

    #[test]
    fn conditional_state_installed_file_sync_failure_retains_published_entries() {
        use super::super::ConditionalHooks;
        use super::super::ConditionalPoint;

        let backend = super::super::fake_backend::FakeBackend::new(b"A", b"B").with_hooks(
            ConditionalHooks::fail_once(
                ConditionalPoint::SyncInstalled,
                std::io::ErrorKind::PermissionDenied,
            ),
        );
        let (_lease, target_path) = backend.target_observer();
        let error = super::super::commit::commit_conditional::<Sha256, _>(
            backend,
            LocalWritePrecondition::Content(Sha256::digest(b"A")),
            super::super::fake_backend::required_options(),
            1,
        )
        .expect_err("required installed-file sync must fail");
        assert_eq!(
            error.reason(),
            crate::error::LocalConditionalFailureReason::Io
        );
        assert_eq!(
            error.stage(),
            crate::error::LocalConditionalWriteStage::Sync
        );
        assert_eq!(
            error.publication(),
            crate::outcome::LocalConditionalPublicationState::Published
        );
        assert_eq!(std::fs::read(target_path).expect("published target"), b"B");
        assert_eq!(
            recovery_bytes(error.candidate().expect("verified B copy")),
            b"B"
        );
        assert!(error.recovery_files().iter().any(|file| {
            file.role() == crate::recovery::LocalRecoveryFileRole::StagedCandidate
                && recovery_bytes(file) == b"B"
        }));
        assert!(error.recovery_files().iter().any(|file| {
            file.role() == crate::recovery::LocalRecoveryFileRole::Displaced
                && recovery_bytes(file) == b"A"
        }));
        assert_eq!(
            error.error().kind(),
            crate::LocalFileErrorKind::PermissionDenied
        );
    }

    #[test]
    fn conditional_state_save_complete_failure_keeps_installed_and_copy() {
        use super::super::ConditionalHooks;
        use super::super::ConditionalPoint;

        let backend = super::super::fake_backend::FakeBackend::new(b"A", b"B").with_hooks(
            ConditionalHooks::fail_once(
                ConditionalPoint::SaveComplete,
                std::io::ErrorKind::PermissionDenied,
            ),
        );
        let (_lease, target_path) = backend.target_observer();
        let error = super::super::commit::commit_conditional::<Sha256, _>(
            backend,
            LocalWritePrecondition::Content(Sha256::digest(b"A")),
            super::super::fake_backend::required_options(),
            1,
        )
        .expect_err("save-complete boundary failure must remain an error");
        assert_eq!(
            error.stage(),
            crate::error::LocalConditionalWriteStage::Sync
        );
        assert_eq!(
            error.publication(),
            crate::outcome::LocalConditionalPublicationState::Published
        );
        assert_eq!(std::fs::read(target_path).expect("installed target"), b"B");
        assert_eq!(
            recovery_bytes(error.candidate().expect("verified B copy")),
            b"B"
        );
        assert!(error.recovery_files().iter().any(|file| {
            file.role() == crate::recovery::LocalRecoveryFileRole::StagedCandidate
                && recovery_bytes(file) == b"B"
        }));
        assert!(error.recovery_files().iter().any(|file| {
            file.role() == crate::recovery::LocalRecoveryFileRole::Displaced
                && recovery_bytes(file) == b"A"
        }));
    }

    #[test]
    fn conditional_state_postpublication_race_rolls_back_once_and_retains_files() {
        use super::super::ConditionalHookAction;
        use super::super::ConditionalHooks;
        use super::super::ConditionalPoint;

        let hooks = ConditionalHooks::sequence(vec![(
            ConditionalPoint::Publish,
            ConditionalHookAction::ReplaceTarget(b"R".to_vec()),
        )]);
        let backend = super::super::fake_backend::FakeBackend::new(b"A", b"B").with_hooks(hooks);
        let error = super::super::commit::commit_conditional::<Sha256, _>(
            backend,
            LocalWritePrecondition::Content(Sha256::digest(b"A")),
            super::super::fake_backend::required_options(),
            1,
        )
        .expect_err("post-publication conflict must be reported");
        assert_eq!(
            error.reason(),
            crate::error::LocalConditionalFailureReason::PostPublicationConflict
        );
        assert_eq!(
            error.stage(),
            crate::error::LocalConditionalWriteStage::Sync
        );
        assert_eq!(
            error.publication(),
            crate::outcome::LocalConditionalPublicationState::Restored
        );
        assert!(error.candidate().is_some());
        let roles: Vec<_> = error
            .recovery_files()
            .iter()
            .map(|file| file.role())
            .collect();
        assert!(roles.contains(&crate::recovery::LocalRecoveryFileRole::RollbackDisplaced));
        assert!(roles.contains(&crate::recovery::LocalRecoveryFileRole::Displaced));
        let restored = error
            .recovery_files()
            .iter()
            .find(|file| file.role() == crate::recovery::LocalRecoveryFileRole::Displaced)
            .expect("restored target retained");
        assert_eq!(recovery_bytes(restored), b"R");
        let displaced_candidate = error
            .recovery_files()
            .iter()
            .find(|file| file.role() == crate::recovery::LocalRecoveryFileRole::RollbackDisplaced)
            .expect("published candidate retained");
        assert_eq!(recovery_bytes(displaced_candidate), b"B");
    }

    #[test]
    fn conditional_state_in_place_candidate_change_blocks_rollback() {
        use super::super::ConditionalHookAction;
        use super::super::ConditionalHooks;
        use super::super::ConditionalPoint;

        let hooks = ConditionalHooks::sequence(vec![
            (
                ConditionalPoint::Publish,
                ConditionalHookAction::PartialPublish,
            ),
            (
                ConditionalPoint::Rollback,
                ConditionalHookAction::OverwriteTargetInPlace(b"C".to_vec()),
            ),
        ]);
        let backend = super::super::fake_backend::FakeBackend::new(b"A", b"B").with_hooks(hooks);
        let (_lease, target_path) = backend.target_observer();
        let events = backend.events();
        let error = super::super::commit::commit_conditional::<Sha256, _>(
            backend,
            LocalWritePrecondition::Content(Sha256::digest(b"A")),
            super::super::fake_backend::required_options(),
            1,
        )
        .expect_err("changed installed bytes must fail rollback precheck");
        assert_eq!(
            error.reason(),
            crate::error::LocalConditionalFailureReason::PostPublicationConflict
        );
        assert_eq!(
            error.stage(),
            crate::error::LocalConditionalWriteStage::RollbackPrecheck
        );
        assert_eq!(
            error.publication(),
            crate::outcome::LocalConditionalPublicationState::Indeterminate
        );
        assert_eq!(
            std::fs::read(target_path).expect("in-place competitor remains"),
            b"C"
        );
        assert_eq!(
            recovery_bytes(error.candidate().expect("independent B copy")),
            b"B"
        );
        assert!(error.recovery_files().iter().any(|file| {
            file.role() == crate::recovery::LocalRecoveryFileRole::StagedCandidate
                && recovery_bytes(file) == b"C"
        }));
        assert!(error.recovery_files().iter().any(|file| {
            file.role() == crate::recovery::LocalRecoveryFileRole::Displaced
                && recovery_bytes(file) == b"A"
        }));
        assert!(
            !events
                .borrow()
                .iter()
                .any(|event| *event == super::super::fake_backend::Event::Rollback)
        );
        assert_eq!(
            error.error().kind(),
            crate::LocalFileErrorKind::DataCorruption
        );
    }

    #[test]
    fn conditional_state_late_target_change_is_preserved_without_rollback() {
        use super::super::ConditionalHookAction;
        use super::super::ConditionalHooks;
        use super::super::ConditionalPoint;

        let hooks = ConditionalHooks::sequence(vec![(
            ConditionalPoint::VerifyInstalled,
            ConditionalHookAction::ReplaceTarget(b"C".to_vec()),
        )]);
        let backend = super::super::fake_backend::FakeBackend::new(b"A", b"B").with_hooks(hooks);
        let (_lease, target_path) = backend.target_observer();
        let error = super::super::commit::commit_conditional::<Sha256, _>(
            backend,
            LocalWritePrecondition::Content(Sha256::digest(b"A")),
            super::super::fake_backend::required_options(),
            1,
        )
        .expect_err("late competing target must not be overwritten by rollback");
        assert_eq!(
            error.reason(),
            crate::error::LocalConditionalFailureReason::PostPublicationConflict
        );
        assert_eq!(
            error.stage(),
            crate::error::LocalConditionalWriteStage::RollbackPrecheck
        );
        assert_eq!(
            error.publication(),
            crate::outcome::LocalConditionalPublicationState::Indeterminate
        );
        assert_eq!(
            std::fs::read(target_path).expect("competitor remains"),
            b"C"
        );
        assert_eq!(
            recovery_bytes(error.candidate().expect("independent B copy")),
            b"B"
        );
        assert!(error.recovery_files().iter().any(|file| {
            file.role() == crate::recovery::LocalRecoveryFileRole::StagedCandidate
                && recovery_bytes(file) == b"C"
        }));
        assert!(error.recovery_files().iter().any(|file| {
            file.role() == crate::recovery::LocalRecoveryFileRole::Displaced
                && recovery_bytes(file) == b"A"
        }));
    }

    #[test]
    fn conditional_state_partial_native_failure_restores_and_preserves_source() {
        use super::super::ConditionalHooks;

        let backend = super::super::fake_backend::FakeBackend::new(b"A", b"B")
            .with_hooks(ConditionalHooks::partial_publish_once());
        let (_lease, target_path) = backend.target_observer();
        let error = super::super::commit::commit_conditional::<Sha256, _>(
            backend,
            LocalWritePrecondition::Content(Sha256::digest(b"A")),
            super::super::fake_backend::required_options(),
            1,
        )
        .expect_err("native error after publication remains an error");
        assert_eq!(
            error.reason(),
            crate::error::LocalConditionalFailureReason::PostPublicationConflict
        );
        assert_eq!(
            error.publication(),
            crate::outcome::LocalConditionalPublicationState::Restored
        );
        assert_eq!(
            std::fs::read(target_path).expect("prior target restored"),
            b"A"
        );
        let source = std::error::Error::source(&error).expect("structured error source");
        let source = std::error::Error::source(source).expect("native I/O source");
        assert_eq!(
            source
                .downcast_ref::<std::io::Error>()
                .map(std::io::Error::kind),
            Some(std::io::ErrorKind::Other)
        );
    }

    #[test]
    fn conditional_state_rollback_failure_keeps_both_namespaces_recoverable() {
        use super::super::ConditionalHookAction;
        use super::super::ConditionalHooks;
        use super::super::ConditionalPoint;

        let hooks = ConditionalHooks::sequence(vec![
            (
                ConditionalPoint::Publish,
                ConditionalHookAction::ReplaceTarget(b"R".to_vec()),
            ),
            (
                ConditionalPoint::Rollback,
                ConditionalHookAction::Fail(std::io::ErrorKind::PermissionDenied),
            ),
        ]);
        let backend = super::super::fake_backend::FakeBackend::new(b"A", b"B").with_hooks(hooks);
        let (_lease, target_path) = backend.target_observer();
        let error = super::super::commit::commit_conditional::<Sha256, _>(
            backend,
            LocalWritePrecondition::Content(Sha256::digest(b"A")),
            super::super::fake_backend::required_options(),
            1,
        )
        .expect_err("failed rollback must keep recovery state");
        assert_eq!(
            error.stage(),
            crate::error::LocalConditionalWriteStage::Rollback
        );
        assert_eq!(
            error.publication(),
            crate::outcome::LocalConditionalPublicationState::Published
        );
        assert_eq!(
            std::fs::read(target_path).expect("installed candidate"),
            b"B"
        );
        assert!(error.candidate().is_some());
        assert_eq!(
            recovery_bytes(error.candidate().expect("candidate copy")),
            b"B"
        );
        assert_eq!(
            error.error().kind(),
            crate::LocalFileErrorKind::PermissionDenied
        );
        assert!(
            error.recovery_files().iter().any(
                |file| file.role() == crate::recovery::LocalRecoveryFileRole::RollbackDisplaced
            )
        );
        assert!(
            error
                .recovery_files()
                .iter()
                .any(|file| file.role() == crate::recovery::LocalRecoveryFileRole::StagedCandidate)
        );
    }

    #[test]
    fn conditional_state_rollback_sync_failure_is_err_and_restored() {
        use super::super::ConditionalHookAction;
        use super::super::ConditionalHooks;
        use super::super::ConditionalPoint;

        let hooks = ConditionalHooks::sequence(vec![
            (
                ConditionalPoint::Publish,
                ConditionalHookAction::ReplaceTarget(b"R".to_vec()),
            ),
            (
                ConditionalPoint::SyncRollback,
                ConditionalHookAction::Fail(std::io::ErrorKind::PermissionDenied),
            ),
        ]);
        let backend = super::super::fake_backend::FakeBackend::new(b"A", b"B").with_hooks(hooks);
        let (_lease, target_path) = backend.target_observer();
        let error = super::super::commit::commit_conditional::<Sha256, _>(
            backend,
            LocalWritePrecondition::Content(Sha256::digest(b"A")),
            super::super::fake_backend::required_options(),
            1,
        )
        .expect_err("required rollback sync failure must be an error");
        assert_eq!(
            error.stage(),
            crate::error::LocalConditionalWriteStage::Sync
        );
        assert_eq!(
            error.publication(),
            crate::outcome::LocalConditionalPublicationState::Restored
        );
        assert!(error.candidate().is_some());
        assert_eq!(std::fs::read(target_path).expect("restored target"), b"R");
        assert_eq!(
            recovery_bytes(error.candidate().expect("candidate copy")),
            b"B"
        );
        assert!(error.recovery_files().iter().any(|file| {
            file.role() == crate::recovery::LocalRecoveryFileRole::RollbackDisplaced
                && recovery_bytes(file) == b"B"
        }));
        assert!(error.recovery_files().iter().any(|file| {
            file.role() == crate::recovery::LocalRecoveryFileRole::Displaced
                && recovery_bytes(file) == b"R"
        }));
        assert_eq!(
            error.error().kind(),
            crate::LocalFileErrorKind::PermissionDenied
        );
    }

    #[test]
    fn conditional_state_rollback_verification_failure_transfers_actual_names() {
        use super::super::ConditionalHookAction;
        use super::super::ConditionalHooks;
        use super::super::ConditionalPoint;

        let hooks = ConditionalHooks::sequence(vec![
            (
                ConditionalPoint::Publish,
                ConditionalHookAction::ReplaceTarget(b"R".to_vec()),
            ),
            (
                ConditionalPoint::VerifyRollback,
                ConditionalHookAction::Fail(std::io::ErrorKind::PermissionDenied),
            ),
        ]);
        let backend = super::super::fake_backend::FakeBackend::new(b"A", b"B").with_hooks(hooks);
        let (_lease, target_path) = backend.target_observer();
        let error = super::super::commit::commit_conditional::<Sha256, _>(
            backend,
            LocalWritePrecondition::Content(Sha256::digest(b"A")),
            super::super::fake_backend::required_options(),
            1,
        )
        .expect_err("rollback verification failure remains an error");
        assert_eq!(
            error.reason(),
            crate::error::LocalConditionalFailureReason::PostPublicationConflict
        );
        assert_eq!(
            error.stage(),
            crate::error::LocalConditionalWriteStage::VerifyRollback
        );
        assert_eq!(
            error.publication(),
            crate::outcome::LocalConditionalPublicationState::Indeterminate
        );
        assert_eq!(std::fs::read(target_path).expect("restored target"), b"R");
        assert_eq!(
            recovery_bytes(error.candidate().expect("verified B copy")),
            b"B"
        );
        assert!(error.recovery_files().iter().any(|file| {
            file.role() == crate::recovery::LocalRecoveryFileRole::Displaced
                && recovery_bytes(file) == b"R"
        }));
        assert!(error.recovery_files().iter().any(|file| {
            file.role() == crate::recovery::LocalRecoveryFileRole::RollbackDisplaced
                && recovery_bytes(file) == b"B"
        }));
        assert_eq!(
            error.error().kind(),
            crate::LocalFileErrorKind::PermissionDenied
        );
    }

    #[test]
    fn conditional_state_postpublication_readback_failure_retains_all_entries() {
        use super::super::ConditionalHooks;
        use super::super::ConditionalPoint;

        let backend = super::super::fake_backend::FakeBackend::new(b"A", b"B").with_hooks(
            ConditionalHooks::fail_once(
                ConditionalPoint::VerifyInstalled,
                std::io::ErrorKind::PermissionDenied,
            ),
        );
        let (_lease, target_path) = backend.target_observer();
        let error = super::super::commit::commit_conditional::<Sha256, _>(
            backend,
            LocalWritePrecondition::Content(Sha256::digest(b"A")),
            super::super::fake_backend::required_options(),
            1,
        )
        .expect_err("installed readback failure must fail closed");
        assert_eq!(
            error.stage(),
            crate::error::LocalConditionalWriteStage::VerifyInstalled
        );
        assert_eq!(
            error.publication(),
            crate::outcome::LocalConditionalPublicationState::Indeterminate
        );
        assert!(error.candidate().is_some());
        assert!(
            error
                .recovery_files()
                .iter()
                .any(|file| file.role() == crate::recovery::LocalRecoveryFileRole::StagedCandidate)
        );
        assert!(
            error
                .recovery_files()
                .iter()
                .any(|file| file.role() == crate::recovery::LocalRecoveryFileRole::Displaced)
        );
        assert_eq!(std::fs::read(target_path).expect("published target"), b"B");
        assert_eq!(recovery_bytes(error.candidate().expect("B copy")), b"B");
        assert!(error.recovery_files().iter().any(|file| {
            file.role() == crate::recovery::LocalRecoveryFileRole::StagedCandidate
                && recovery_bytes(file) == b"B"
        }));
        assert!(error.recovery_files().iter().any(|file| {
            file.role() == crate::recovery::LocalRecoveryFileRole::Displaced
                && recovery_bytes(file) == b"A"
        }));
        assert_eq!(
            error.error().kind(),
            crate::LocalFileErrorKind::PermissionDenied
        );
    }

    #[test]
    fn conditional_state_cleanup_failure_is_reported_on_success() {
        use super::super::ConditionalHooks;
        use super::super::ConditionalPoint;

        let backend = super::super::fake_backend::FakeBackend::new(b"A", b"B").with_hooks(
            ConditionalHooks::fail_once(
                ConditionalPoint::CleanupOld,
                std::io::ErrorKind::PermissionDenied,
            ),
        );
        let (_lease, target_path) = backend.target_observer();
        let outcome = super::super::commit::commit_conditional::<Sha256, _>(
            backend,
            LocalWritePrecondition::Content(Sha256::digest(b"A")),
            super::super::fake_backend::required_options(),
            1,
        )
        .expect("post-save cleanup failure must remain success");
        assert_eq!(outcome.digest(), &Sha256::digest(b"B"));
        assert_eq!(outcome.cleanup_errors().len(), 1);
        assert_eq!(outcome.retained_files().len(), 1);
        assert_eq!(std::fs::read(target_path).expect("installed target"), b"B");
        assert_eq!(
            outcome.retained_files()[0].role(),
            crate::recovery::LocalRecoveryFileRole::Displaced
        );
        assert_eq!(recovery_bytes(&outcome.retained_files()[0]), b"A");
        assert_eq!(
            outcome.cleanup_errors()[0].kind(),
            crate::LocalFileErrorKind::PermissionDenied
        );
    }

    #[test]
    fn conditional_state_cleanup_sync_failure_after_copy_delete_is_reported_as_ok() {
        use super::super::ConditionalHooks;
        use super::super::ConditionalPoint;

        let backend = super::super::fake_backend::FakeBackend::new(b"A", b"B").with_hooks(
            ConditionalHooks::fail_once(
                ConditionalPoint::CleanupSync,
                std::io::ErrorKind::PermissionDenied,
            ),
        );
        let (_lease, target_path) = backend.target_observer();
        let outcome = super::super::commit::commit_conditional::<Sha256, _>(
            backend,
            LocalWritePrecondition::Content(Sha256::digest(b"A")),
            super::super::fake_backend::required_options(),
            1,
        )
        .expect("cleanup directory sync failure cannot undo save");
        assert_eq!(outcome.cleanup_errors().len(), 1);
        assert!(outcome.retained_files().is_empty());
        assert_eq!(std::fs::read(target_path).expect("saved target"), b"B");
        assert_eq!(
            outcome.cleanup_errors()[0].kind(),
            crate::LocalFileErrorKind::PermissionDenied
        );
    }

    #[test]
    fn conditional_state_absent_precondition_rejects_existing_target() {
        let backend = super::super::fake_backend::FakeBackend::new(b"A", b"B");
        let (_lease, target_path) = backend.target_observer();
        let error = super::super::commit::commit_conditional::<Sha256, _>(
            backend,
            LocalWritePrecondition::Absent,
            super::super::fake_backend::required_options(),
            1,
        )
        .expect_err("existing target violates absence precondition");
        assert_eq!(
            error.reason(),
            crate::error::LocalConditionalFailureReason::TargetExists
        );
        assert_eq!(
            error.stage(),
            crate::error::LocalConditionalWriteStage::Precheck
        );
        assert_eq!(
            error.publication(),
            crate::outcome::LocalConditionalPublicationState::NotPublished
        );
        assert_eq!(std::fs::read(target_path).expect("target unchanged"), b"A");
        assert!(error.candidate().is_some());
    }

    #[test]
    fn conditional_state_absent_publish_uses_atomic_rename_and_detects_race() {
        use super::super::ConditionalHookAction;
        use super::super::ConditionalHooks;
        use super::super::ConditionalPoint;

        let backend = super::super::fake_backend::FakeBackend::new(b"A", b"B");
        backend.remove_target();
        let (_lease, target_path) = backend.target_observer();
        let hooks = ConditionalHooks::sequence(vec![(
            ConditionalPoint::Publish,
            ConditionalHookAction::ReplaceTarget(b"racer".to_vec()),
        )]);
        let error = super::super::commit::commit_conditional::<Sha256, _>(
            backend.with_hooks(hooks),
            LocalWritePrecondition::Absent,
            super::super::fake_backend::required_options(),
            1,
        )
        .expect_err("publish must reject a target created after precheck");
        assert_eq!(
            error.reason(),
            crate::error::LocalConditionalFailureReason::TargetExists
        );
        assert_eq!(
            error.publication(),
            crate::outcome::LocalConditionalPublicationState::NotPublished
        );
        assert_eq!(
            std::fs::read(target_path).expect("racing target stays"),
            b"racer"
        );
        assert!(error.candidate().is_some());
    }

    #[test]
    fn conditional_state_absent_success_reports_atomic_rename() {
        let backend = super::super::fake_backend::FakeBackend::new(b"A", b"B");
        backend.remove_target();
        let (_lease, target_path) = backend.target_observer();
        let outcome = super::super::commit::commit_conditional::<Sha256, _>(
            backend,
            LocalWritePrecondition::Absent,
            super::super::fake_backend::required_options(),
            1,
        )
        .expect("absent target is installed");
        assert_eq!(
            outcome.write_outcome().publication_method(),
            crate::outcome::LocalWritePublicationMethod::AtomicRename
        );
        assert_eq!(std::fs::read(target_path).expect("installed target"), b"B");
        assert!(outcome.retained_files().is_empty());
    }

    #[test]
    fn conditional_state_missing_content_target_is_classified_and_copy_retained() {
        let backend = super::super::fake_backend::FakeBackend::new(b"A", b"B");
        backend.remove_target();
        let error = super::super::commit::commit_conditional::<Sha256, _>(
            backend,
            LocalWritePrecondition::Content(Sha256::digest(b"A")),
            super::super::fake_backend::required_options(),
            1,
        )
        .expect_err("missing content precondition target must fail");
        assert_eq!(
            error.reason(),
            crate::error::LocalConditionalFailureReason::TargetMissing
        );
        assert_eq!(
            error.stage(),
            crate::error::LocalConditionalWriteStage::Precheck
        );
        assert_eq!(
            error.publication(),
            crate::outcome::LocalConditionalPublicationState::NotPublished
        );
        assert_eq!(
            recovery_bytes(error.candidate().expect("verified B copy retained")),
            b"B"
        );
    }

    #[test]
    fn conditional_state_copy_creation_failure_has_no_false_candidate_copy() {
        use super::super::ConditionalHooks;
        use super::super::ConditionalPoint;

        let backend = super::super::fake_backend::FakeBackend::new(b"A", b"B").with_hooks(
            ConditionalHooks::fail_once(
                ConditionalPoint::PrepareCandidateCopy,
                std::io::ErrorKind::PermissionDenied,
            ),
        );
        let (_lease, target_path) = backend.target_observer();
        let error = super::super::commit::commit_conditional::<Sha256, _>(
            backend,
            LocalWritePrecondition::Content(Sha256::digest(b"A")),
            super::super::fake_backend::required_options(),
            1,
        )
        .expect_err("candidate-copy setup failure must abort");
        assert_eq!(
            error.stage(),
            crate::error::LocalConditionalWriteStage::PrepareCandidateCopy
        );
        assert_eq!(
            error.publication(),
            crate::outcome::LocalConditionalPublicationState::NotPublished
        );
        assert!(error.candidate().is_none());
        assert_eq!(std::fs::read(target_path).expect("target unchanged"), b"A");
    }

    #[test]
    fn conditional_state_partial_copy_failure_transfers_staged_entry() {
        use super::super::ConditionalHookAction;
        use super::super::ConditionalHooks;
        use super::super::ConditionalPoint;

        let hooks = ConditionalHooks::sequence(vec![(
            ConditionalPoint::PrepareCandidateCopy,
            ConditionalHookAction::PartialCandidateCopy(b"partial".to_vec()),
        )]);
        let backend = super::super::fake_backend::FakeBackend::new(b"A", b"B").with_hooks(hooks);
        let (_lease, target_path) = backend.target_observer();
        let error = super::super::commit::commit_conditional::<Sha256, _>(
            backend,
            LocalWritePrecondition::Content(Sha256::digest(b"A")),
            super::super::fake_backend::required_options(),
            1,
        )
        .expect_err("partial copy failure must not publish");
        assert_eq!(
            error.stage(),
            crate::error::LocalConditionalWriteStage::PrepareCandidateCopy
        );
        assert_eq!(
            error.publication(),
            crate::outcome::LocalConditionalPublicationState::NotPublished
        );
        assert!(error.candidate().is_none());
        assert_eq!(error.recovery_files().len(), 1);
        assert_eq!(
            error.recovery_files()[0].role(),
            crate::recovery::LocalRecoveryFileRole::StagedCandidate
        );
        assert_eq!(recovery_bytes(&error.recovery_files()[0]), b"partial");
        assert_eq!(std::fs::read(target_path).expect("target unchanged"), b"A");
    }

    #[test]
    fn conditional_state_required_prepublication_sync_failures_are_fail_closed() {
        use super::super::ConditionalHooks;
        use super::super::ConditionalPoint;

        for point in [
            ConditionalPoint::SyncCandidate,
            ConditionalPoint::SyncEntry,
            ConditionalPoint::SyncNamespace,
        ] {
            let backend = super::super::fake_backend::FakeBackend::new(b"A", b"B").with_hooks(
                ConditionalHooks::fail_once(point, std::io::ErrorKind::PermissionDenied),
            );
            let (_lease, target_path) = backend.target_observer();
            let error = super::super::commit::commit_conditional::<Sha256, _>(
                backend,
                LocalWritePrecondition::Content(Sha256::digest(b"A")),
                super::super::fake_backend::required_options(),
                1,
            )
            .expect_err("every required prepublication sync must be enforced");
            assert_eq!(
                error.stage(),
                crate::error::LocalConditionalWriteStage::Sync
            );
            assert_eq!(
                error.publication(),
                crate::outcome::LocalConditionalPublicationState::NotPublished
            );
            assert_eq!(
                error.error().kind(),
                crate::LocalFileErrorKind::PermissionDenied
            );
            assert!(error.candidate().is_some());
            assert_eq!(std::fs::read(target_path).expect("target unchanged"), b"A");
        }
    }

    #[test]
    fn conditional_state_preferred_sync_failure_downgrades_durability_only() {
        use super::super::ConditionalHooks;
        use super::super::ConditionalPoint;
        use crate::options::LocalWriteMode;
        use crate::options::LocalWriteOptions;
        use crate::policy::LocalAtomicityRequirement;
        use crate::policy::LocalDurabilityRequirement;

        let options = LocalWriteOptions::new(LocalWriteMode::CreateOrReplace)
            .with_atomicity(LocalAtomicityRequirement::Required)
            .with_durability(LocalDurabilityRequirement::Preferred);
        let backend = super::super::fake_backend::FakeBackend::new(b"A", b"B").with_hooks(
            ConditionalHooks::fail_once(
                ConditionalPoint::SyncCandidate,
                std::io::ErrorKind::PermissionDenied,
            ),
        );
        let outcome = super::super::commit::commit_conditional::<Sha256, _>(
            backend,
            LocalWritePrecondition::Content(Sha256::digest(b"A")),
            options,
            1,
        )
        .expect("preferred synchronization may downgrade");
        assert_eq!(outcome.digest(), &Sha256::digest(b"B"));
        assert!(!outcome.write_outcome().durable());
        assert!(outcome.cleanup_errors().is_empty());
    }

    #[test]
    fn conditional_state_copy_cleanup_failure_returns_ok_with_candidate_copy_retained() {
        use super::super::ConditionalHooks;
        use super::super::ConditionalPoint;

        let backend = super::super::fake_backend::FakeBackend::new(b"A", b"B").with_hooks(
            ConditionalHooks::fail_once(
                ConditionalPoint::CleanupCopy,
                std::io::ErrorKind::PermissionDenied,
            ),
        );
        let (_lease, target_path) = backend.target_observer();
        let outcome = super::super::commit::commit_conditional::<Sha256, _>(
            backend,
            LocalWritePrecondition::Content(Sha256::digest(b"A")),
            super::super::fake_backend::required_options(),
            1,
        )
        .expect("post-save candidate-copy cleanup failure remains success");
        assert_eq!(outcome.cleanup_errors().len(), 1);
        assert_eq!(std::fs::read(target_path).expect("saved target"), b"B");
        assert_eq!(outcome.retained_files().len(), 1);
        assert_eq!(
            outcome.retained_files()[0].role(),
            crate::recovery::LocalRecoveryFileRole::CandidateCopy
        );
        assert_eq!(recovery_bytes(&outcome.retained_files()[0]), b"B");
        assert_eq!(
            outcome.cleanup_errors()[0].kind(),
            crate::LocalFileErrorKind::PermissionDenied
        );
    }
}
