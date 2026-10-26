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
    use qubit_io::Streams;
    use sha2::Digest;
    use sha2::Sha256;

    use crate::options::LocalWritePrecondition;

    #[test]
    fn conditional_hooks_debug_output_hides_actions() {
        let hooks = super::super::hooks::ConditionalHooks::default();
        assert_eq!("ConditionalHooks { .. }", format!("{hooks:?}"));
    }

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
        let mut reader = file.open_reader().expect("recovery entry opens");
        let mut bytes = Vec::new();
        Streams::copy_input_to_output(&mut reader, &mut bytes).expect("recovery entry reads");
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

        let backend = super::super::fake_backend::FakeBackend::new(b"A", b"B").with_hooks(ConditionalHooks::fail_once(
            ConditionalPoint::Flush,
            std::io::ErrorKind::PermissionDenied,
        ));
        let (_lease, target_path) = backend.target_observer();
        let error = super::super::commit::commit_conditional::<Sha256, _>(
            backend,
            LocalWritePrecondition::Content(Sha256::digest(b"A")),
            super::super::fake_backend::required_options(),
            1,
        )
        .expect_err("flush failure must stop before candidate verification");
        assert_eq!(error.stage(), crate::error::LocalConditionalWriteStage::Prepare);
        assert_eq!(error.reason(), crate::error::LocalConditionalFailureReason::Io);
        assert_eq!(
            error.publication(),
            crate::outcome::LocalConditionalPublicationState::NotPublished
        );
        assert_eq!(error.error().kind(), crate::LocalFileErrorKind::PermissionDenied);
        assert!(error.candidate().is_none());
        assert_eq!(std::fs::read(target_path).expect("target unchanged"), b"A");
    }

    #[test]
    fn conditional_state_prepublication_checkpoints_fail_closed_with_recovery_copy() {
        use super::super::ConditionalHooks;
        use super::super::ConditionalPoint;

        for (point, expected_stage) in [
            (
                ConditionalPoint::VerifyCandidateCopy,
                crate::error::LocalConditionalWriteStage::VerifyCandidateCopy,
            ),
            (
                ConditionalPoint::Precheck,
                crate::error::LocalConditionalWriteStage::Precheck,
            ),
            (
                ConditionalPoint::AfterPrecheck,
                crate::error::LocalConditionalWriteStage::Precheck,
            ),
            (
                ConditionalPoint::Publish,
                crate::error::LocalConditionalWriteStage::Publish,
            ),
        ] {
            let backend = super::super::fake_backend::FakeBackend::new(b"A", b"B")
                .with_hooks(ConditionalHooks::fail_once(point, std::io::ErrorKind::PermissionDenied));
            let (_lease, target_path) = backend.target_observer();
            let error = super::super::commit::commit_conditional::<Sha256, _>(
                backend,
                LocalWritePrecondition::Content(Sha256::digest(b"A")),
                super::super::fake_backend::required_options(),
                1,
            )
            .expect_err("a failed prepublication checkpoint must prevent publishing");

            assert_eq!(error.stage(), expected_stage, "checkpoint {point:?}");
            assert_eq!(error.reason(), crate::error::LocalConditionalFailureReason::Io);
            assert_eq!(
                error.publication(),
                crate::outcome::LocalConditionalPublicationState::NotPublished
            );
            assert_eq!(
                recovery_bytes(error.candidate().expect("verified B copy retained")),
                b"B"
            );
            assert_eq!(std::fs::read(target_path).expect("old target remains"), b"A");
        }
    }

    #[test]
    fn conditional_state_postpublication_checkpoint_error_transfers_native_receipts() {
        use super::super::ConditionalHooks;
        use super::super::ConditionalPoint;

        let backend = super::super::fake_backend::FakeBackend::new(b"A", b"B").with_hooks(ConditionalHooks::fail_once(
            ConditionalPoint::AfterPublish,
            std::io::ErrorKind::Other,
        ));
        let (_lease, target_path) = backend.target_observer();
        let error = super::super::commit::commit_conditional::<Sha256, _>(
            backend,
            LocalWritePrecondition::Content(Sha256::digest(b"A")),
            super::super::fake_backend::required_options(),
            1,
        )
        .expect_err("failure after native publication must preserve its receipts");

        assert_eq!(error.stage(), crate::error::LocalConditionalWriteStage::Publish);
        assert_eq!(error.reason(), crate::error::LocalConditionalFailureReason::Io);
        assert_eq!(
            error.publication(),
            crate::outcome::LocalConditionalPublicationState::Published
        );
        assert_eq!(std::fs::read(target_path).expect("published target remains"), b"B");
        assert_eq!(recovery_bytes(error.candidate().expect("verified copy retained")), b"B");
        assert!(error.recovery_files().iter().any(|file| {
            file.role() == crate::recovery::LocalRecoveryFileRole::StagedCandidate && recovery_bytes(file) == b"B"
        }));
        assert!(error.recovery_files().iter().any(|file| {
            file.role() == crate::recovery::LocalRecoveryFileRole::Displaced && recovery_bytes(file) == b"A"
        }));
    }

    #[test]
    fn conditional_state_displaced_and_rollback_checkpoints_preserve_recovery() {
        use super::super::ConditionalHookAction;
        use super::super::ConditionalHooks;
        use super::super::ConditionalPoint;

        let displaced_backend = super::super::fake_backend::FakeBackend::new(b"A", b"B").with_hooks(
            ConditionalHooks::fail_once(ConditionalPoint::VerifyDisplaced, std::io::ErrorKind::Other),
        );
        let (_lease, target_path) = displaced_backend.target_observer();
        let error = super::super::commit::commit_conditional::<Sha256, _>(
            displaced_backend,
            LocalWritePrecondition::Content(Sha256::digest(b"A")),
            super::super::fake_backend::required_options(),
            1,
        )
        .expect_err("displaced-entry verification failure must retain both entries");
        assert_eq!(error.stage(), crate::error::LocalConditionalWriteStage::VerifyDisplaced);
        assert_eq!(
            error.publication(),
            crate::outcome::LocalConditionalPublicationState::Indeterminate
        );
        assert_eq!(std::fs::read(target_path).expect("published candidate remains"), b"B");
        assert!(error.candidate().is_some());
        assert!(
            error
                .recovery_files()
                .iter()
                .any(|file| { file.role() == crate::recovery::LocalRecoveryFileRole::Displaced })
        );

        for point in [
            ConditionalPoint::RollbackPrecheck,
            ConditionalPoint::RollbackValidated,
            ConditionalPoint::RollbackCompleted,
        ] {
            let hooks = ConditionalHooks::sequence(vec![
                (
                    ConditionalPoint::Publish,
                    ConditionalHookAction::ReplaceTarget(b"R".to_vec()),
                ),
                (point, ConditionalHookAction::Fail(std::io::ErrorKind::PermissionDenied)),
            ]);
            let backend = super::super::fake_backend::FakeBackend::new(b"A", b"B").with_hooks(hooks);
            let (_lease, target_path) = backend.target_observer();
            let error = super::super::commit::commit_conditional::<Sha256, _>(
                backend,
                LocalWritePrecondition::Content(Sha256::digest(b"A")),
                super::super::fake_backend::required_options(),
                1,
            )
            .expect_err("rollback checkpoint failure must retain recovery entries");

            assert_eq!(
                error.reason(),
                crate::error::LocalConditionalFailureReason::PostPublicationConflict
            );
            assert_eq!(
                error.publication(),
                crate::outcome::LocalConditionalPublicationState::Indeterminate
            );
            let expected_target = if point == ConditionalPoint::RollbackCompleted {
                b"R".as_slice()
            } else {
                b"B".as_slice()
            };
            assert_eq!(
                std::fs::read(target_path).expect("current target retained"),
                expected_target
            );
            assert!(error.candidate().is_some());
            assert!(error.recovery_files().iter().any(|file| {
                matches!(
                    file.role(),
                    crate::recovery::LocalRecoveryFileRole::Displaced
                        | crate::recovery::LocalRecoveryFileRole::RollbackDisplaced
                )
            }));
        }
    }

    #[test]
    fn conditional_state_candidate_copy_backend_error_has_no_copy() {
        let backend = super::super::fake_backend::FakeBackend::new(b"A", b"B")
            .fail_candidate_copy_once(std::io::ErrorKind::PermissionDenied);
        let (_lease, target_path) = backend.target_observer();
        let error = super::super::commit::commit_conditional::<Sha256, _>(
            backend,
            LocalWritePrecondition::Content(Sha256::digest(b"A")),
            super::super::fake_backend::required_options(),
            1,
        )
        .expect_err("candidate-copy creation error must fail before publication");

        assert_eq!(
            error.stage(),
            crate::error::LocalConditionalWriteStage::PrepareCandidateCopy
        );
        assert_eq!(error.reason(), crate::error::LocalConditionalFailureReason::Io);
        assert_eq!(
            error.publication(),
            crate::outcome::LocalConditionalPublicationState::NotPublished
        );
        assert!(error.candidate().is_none());
        assert!(error.recovery_files().is_empty());
        assert_eq!(std::fs::read(target_path).expect("old target remains"), b"A");
    }

    #[test]
    fn conditional_state_absent_precheck_io_error_is_not_target_missing() {
        let backend = super::super::fake_backend::FakeBackend::new(b"A", b"B")
            .fail_target_open_once(std::io::ErrorKind::PermissionDenied);
        let (_lease, target_path) = backend.target_observer();
        let error = super::super::commit::commit_conditional::<Sha256, _>(
            backend,
            LocalWritePrecondition::Absent,
            super::super::fake_backend::required_options(),
            1,
        )
        .expect_err("non-NotFound target-open failure must be classified as I/O");

        assert_eq!(error.stage(), crate::error::LocalConditionalWriteStage::Precheck);
        assert_eq!(error.reason(), crate::error::LocalConditionalFailureReason::Io);
        assert_eq!(
            error.publication(),
            crate::outcome::LocalConditionalPublicationState::NotPublished
        );
        assert_eq!(recovery_bytes(error.candidate().expect("verified copy retained")), b"B");
        assert_eq!(std::fs::read(target_path).expect("old target remains"), b"A");
    }

    #[test]
    fn conditional_state_content_publish_without_displaced_receipt_is_indeterminate() {
        let backend = super::super::fake_backend::FakeBackend::new(b"A", b"B").omit_displaced_receipt_once();
        let (_lease, target_path) = backend.target_observer();
        let error = super::super::commit::commit_conditional::<Sha256, _>(
            backend,
            LocalWritePrecondition::Content(Sha256::digest(b"A")),
            super::super::fake_backend::required_options(),
            1,
        )
        .expect_err("content publication requires a receipt for the displaced target");

        assert_eq!(error.stage(), crate::error::LocalConditionalWriteStage::Publish);
        assert_eq!(error.reason(), crate::error::LocalConditionalFailureReason::Io);
        assert_eq!(
            error.publication(),
            crate::outcome::LocalConditionalPublicationState::Indeterminate
        );
        assert_eq!(error.error().kind(), crate::LocalFileErrorKind::DataCorruption);
        assert_eq!(std::fs::read(target_path).expect("installed candidate remains"), b"B");
        assert_eq!(
            recovery_bytes(error.candidate().expect("independent copy retained")),
            b"B"
        );
        assert!(error.recovery_files().iter().any(|file| {
            file.role() == crate::recovery::LocalRecoveryFileRole::StagedCandidate && recovery_bytes(file) == b"B"
        }));
    }

    #[test]
    fn conditional_state_preferred_metadata_sync_failure_only_downgrades_durability() {
        use super::super::ConditionalHookAction;
        use super::super::ConditionalHooks;
        use super::super::ConditionalPoint;
        use crate::options::LocalWriteMode;
        use crate::options::LocalWriteOptions;
        use crate::policy::LocalAtomicityRequirement;
        use crate::policy::LocalDurabilityRequirement;

        let options = LocalWriteOptions::new(LocalWriteMode::CreateOrReplace)
            .with_atomicity(LocalAtomicityRequirement::Required)
            .with_durability(LocalDurabilityRequirement::Preferred);
        let hooks = ConditionalHooks::sequence(vec![
            (ConditionalPoint::SyncCandidate, ConditionalHookAction::Noop),
            (
                ConditionalPoint::SyncCandidate,
                ConditionalHookAction::Fail(std::io::ErrorKind::PermissionDenied),
            ),
        ]);
        let backend = super::super::fake_backend::FakeBackend::new(b"A", b"B").with_hooks(hooks);
        let (_lease, target_path) = backend.target_observer();
        let outcome = super::super::commit::commit_conditional::<Sha256, _>(
            backend,
            LocalWritePrecondition::Content(Sha256::digest(b"A")),
            options,
            1,
        )
        .expect("Preferred durability must permit publication after metadata sync failure");

        assert!(!outcome.write_outcome().durable());
        assert_eq!(outcome.digest(), &Sha256::digest(b"B"));
        assert_eq!(std::fs::read(target_path).expect("candidate published"), b"B");
    }

    #[test]
    fn conditional_state_native_receipt_and_identity_failures_keep_all_recovery_entries() {
        let backend = super::super::fake_backend::FakeBackend::new(b"A", b"B").omit_installed_receipt_once();
        let (_lease, target_path) = backend.target_observer();
        let error = super::super::commit::commit_conditional::<Sha256, _>(
            backend,
            LocalWritePrecondition::Content(Sha256::digest(b"A")),
            super::super::fake_backend::required_options(),
            1,
        )
        .expect_err("publication without an installed receipt is indeterminate");
        assert_eq!(error.stage(), crate::error::LocalConditionalWriteStage::Publish);
        assert_eq!(
            error.publication(),
            crate::outcome::LocalConditionalPublicationState::Indeterminate
        );
        assert_eq!(std::fs::read(target_path).expect("published target remains"), b"B");
        assert!(error.candidate().is_some());
        assert!(error.recovery_files().iter().any(|file| {
            file.role() == crate::recovery::LocalRecoveryFileRole::Displaced && recovery_bytes(file) == b"A"
        }));

        for backend in [
            super::super::fake_backend::FakeBackend::new(b"A", b"B").fail_displaced_verify_once(),
            super::super::fake_backend::FakeBackend::new(b"A", b"B").fail_candidate_identity_once(),
        ] {
            let (_lease, target_path) = backend.target_observer();
            let error = super::super::commit::commit_conditional::<Sha256, _>(
                backend,
                LocalWritePrecondition::Content(Sha256::digest(b"A")),
                super::super::fake_backend::required_options(),
                1,
            )
            .expect_err("native receipt verification errors must preserve recovery entries");
            assert_eq!(
                error.publication(),
                crate::outcome::LocalConditionalPublicationState::Indeterminate
            );
            assert_eq!(std::fs::read(target_path).expect("published target remains"), b"B");
            assert!(error.candidate().is_some());
            assert!(error.recovery_files().iter().any(|file| {
                file.role() == crate::recovery::LocalRecoveryFileRole::Displaced && recovery_bytes(file) == b"A"
            }));
        }
    }

    #[test]
    fn conditional_state_digest_readback_errors_keep_published_receipts() {
        let backend = super::super::fake_backend::FakeBackend::new(b"A", b"B").remove_displaced_after_verify_once();
        let (_lease, target_path) = backend.target_observer();
        let error = super::super::commit::commit_conditional::<Sha256, _>(
            backend,
            LocalWritePrecondition::Content(Sha256::digest(b"A")),
            super::super::fake_backend::required_options(),
            1,
        )
        .expect_err("displaced digest readback failure must be indeterminate");
        assert_eq!(error.stage(), crate::error::LocalConditionalWriteStage::VerifyDisplaced);
        assert_eq!(
            error.publication(),
            crate::outcome::LocalConditionalPublicationState::Indeterminate
        );
        assert_eq!(std::fs::read(target_path).expect("published target remains"), b"B");
        assert_eq!(
            recovery_bytes(error.candidate().expect("independent copy retained")),
            b"B"
        );
        assert!(
            error
                .recovery_files()
                .iter()
                .any(|file| { file.role() == crate::recovery::LocalRecoveryFileRole::Displaced })
        );

        let backend = super::super::fake_backend::FakeBackend::new(b"A", b"B").remove_installed_after_identity_once();
        let (_lease, target_path) = backend.target_observer();
        let error = super::super::commit::commit_conditional::<Sha256, _>(
            backend,
            LocalWritePrecondition::Content(Sha256::digest(b"A")),
            super::super::fake_backend::required_options(),
            1,
        )
        .expect_err("installed digest readback failure must be indeterminate");
        assert_eq!(error.stage(), crate::error::LocalConditionalWriteStage::VerifyInstalled);
        assert_eq!(
            error.publication(),
            crate::outcome::LocalConditionalPublicationState::Indeterminate
        );
        assert!(!target_path.exists());
        assert_eq!(
            recovery_bytes(error.candidate().expect("independent copy retained")),
            b"B"
        );
        assert!(
            error
                .recovery_files()
                .iter()
                .any(|file| { file.role() == crate::recovery::LocalRecoveryFileRole::StagedCandidate })
        );
    }

    #[test]
    fn conditional_state_preferred_published_sync_failure_returns_durable_false() {
        use super::super::ConditionalHooks;
        use super::super::ConditionalPoint;
        use crate::options::LocalWriteMode;
        use crate::options::LocalWriteOptions;
        use crate::policy::LocalAtomicityRequirement;
        use crate::policy::LocalDurabilityRequirement;

        let options = LocalWriteOptions::new(LocalWriteMode::CreateOrReplace)
            .with_atomicity(LocalAtomicityRequirement::Required)
            .with_durability(LocalDurabilityRequirement::Preferred);
        let backend = super::super::fake_backend::FakeBackend::new(b"A", b"B").with_hooks(ConditionalHooks::fail_once(
            ConditionalPoint::SyncInstalled,
            std::io::ErrorKind::PermissionDenied,
        ));
        let (_lease, target_path) = backend.target_observer();
        let outcome = super::super::commit::commit_conditional::<Sha256, _>(
            backend,
            LocalWritePrecondition::Content(Sha256::digest(b"A")),
            options,
            1,
        )
        .expect("Preferred post-publication sync failure cannot undo the save");

        assert!(!outcome.write_outcome().durable());
        assert_eq!(outcome.digest(), &Sha256::digest(b"B"));
        assert_eq!(std::fs::read(target_path).expect("published candidate remains"), b"B");
        assert!(outcome.cleanup_errors().is_empty());
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
        assert_eq!(error.stage(), crate::error::LocalConditionalWriteStage::Precheck);
        assert_eq!(
            error.publication(),
            crate::outcome::LocalConditionalPublicationState::NotPublished
        );
        let copy = error.candidate().expect("verified copy must be retained");
        assert_eq!(copy.role(), crate::recovery::LocalRecoveryFileRole::CandidateCopy);
        let mut reader = copy.open_reader().expect("retained copy remains readable");
        let mut bytes = Vec::new();
        Streams::copy_input_to_output(&mut reader, &mut bytes).expect("read recovery content");
        assert_eq!(bytes, b"B");
        assert_eq!(std::fs::read(target_path).expect("precondition target remains"), b"A");
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

        let backend =
            super::super::fake_backend::FakeBackend::new(b"A", b"B").with_hooks(ConditionalHooks::sequence(vec![(
                ConditionalPoint::CandidateCopyCreated,
                ConditionalHookAction::CorruptCandidateCopy(b"broken".to_vec()),
            )]));
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
        assert_eq!(error.reason(), crate::error::LocalConditionalFailureReason::Io);
        assert_eq!(error.error().kind(), crate::LocalFileErrorKind::DataCorruption);
    }

    #[test]
    fn conditional_state_candidate_copy_digest_mismatch_retains_corrupt_copy() {
        use super::super::ConditionalHookAction;
        use super::super::ConditionalHooks;
        use super::super::ConditionalPoint;

        let backend =
            super::super::fake_backend::FakeBackend::new(b"A", b"B").with_hooks(ConditionalHooks::sequence(vec![(
                ConditionalPoint::CandidateCopyCreated,
                ConditionalHookAction::CorruptCandidateCopy(b"damaged".to_vec()),
            )]));
        let (_lease, target_path) = backend.target_observer();
        let error = super::super::commit::commit_conditional::<Sha256, _>(
            backend,
            LocalWritePrecondition::Content(Sha256::digest(b"A")),
            super::super::fake_backend::required_options(),
            1,
        )
        .expect_err("digest-mismatched candidate copy must not publish");

        assert_eq!(
            error.stage(),
            crate::error::LocalConditionalWriteStage::VerifyCandidateCopy
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
        assert_eq!(recovery_bytes(&error.recovery_files()[0]), b"damaged");
        assert_eq!(std::fs::read(target_path).expect("target remains unchanged"), b"A");
    }

    #[test]
    fn conditional_state_prepublication_sync_failure_preserves_verified_copy() {
        use super::super::ConditionalHooks;
        use super::super::ConditionalPoint;

        let backend = super::super::fake_backend::FakeBackend::new(b"A", b"B").with_hooks(ConditionalHooks::fail_once(
            ConditionalPoint::SyncCandidate,
            std::io::ErrorKind::PermissionDenied,
        ));
        let error = super::super::commit::commit_conditional::<Sha256, _>(
            backend,
            LocalWritePrecondition::Content(Sha256::digest(b"A")),
            super::super::fake_backend::required_options(),
            1,
        )
        .expect_err("required candidate sync failure must stop publication");
        assert_eq!(error.stage(), crate::error::LocalConditionalWriteStage::Sync);
        assert_eq!(error.reason(), crate::error::LocalConditionalFailureReason::Io);
        assert_eq!(error.error().kind(), crate::error::LocalFileErrorKind::PermissionDenied);
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
        assert_eq!(error.reason(), crate::error::LocalConditionalFailureReason::Io);
        assert_eq!(error.stage(), crate::error::LocalConditionalWriteStage::Sync);
        assert_eq!(
            error.publication(),
            crate::outcome::LocalConditionalPublicationState::Published
        );
        assert_eq!(std::fs::read(target_path).expect("published B remains"), b"B");
        assert_eq!(recovery_bytes(error.candidate().expect("verified B copy")), b"B");
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
        assert_eq!(error.error().kind(), crate::LocalFileErrorKind::PermissionDenied);
    }

    #[test]
    fn conditional_state_installed_file_sync_failure_retains_published_entries() {
        use super::super::ConditionalHooks;
        use super::super::ConditionalPoint;

        let backend = super::super::fake_backend::FakeBackend::new(b"A", b"B").with_hooks(ConditionalHooks::fail_once(
            ConditionalPoint::SyncInstalled,
            std::io::ErrorKind::PermissionDenied,
        ));
        let (_lease, target_path) = backend.target_observer();
        let error = super::super::commit::commit_conditional::<Sha256, _>(
            backend,
            LocalWritePrecondition::Content(Sha256::digest(b"A")),
            super::super::fake_backend::required_options(),
            1,
        )
        .expect_err("required installed-file sync must fail");
        assert_eq!(error.reason(), crate::error::LocalConditionalFailureReason::Io);
        assert_eq!(error.stage(), crate::error::LocalConditionalWriteStage::Sync);
        assert_eq!(
            error.publication(),
            crate::outcome::LocalConditionalPublicationState::Published
        );
        assert_eq!(std::fs::read(target_path).expect("published target"), b"B");
        assert_eq!(recovery_bytes(error.candidate().expect("verified B copy")), b"B");
        assert!(error.recovery_files().iter().any(|file| {
            file.role() == crate::recovery::LocalRecoveryFileRole::StagedCandidate && recovery_bytes(file) == b"B"
        }));
        assert!(error.recovery_files().iter().any(|file| {
            file.role() == crate::recovery::LocalRecoveryFileRole::Displaced && recovery_bytes(file) == b"A"
        }));
        assert_eq!(error.error().kind(), crate::LocalFileErrorKind::PermissionDenied);
    }

    #[test]
    fn conditional_state_save_complete_cleanup_failure_returns_success_and_retains_files() {
        use super::super::ConditionalHooks;
        use super::super::ConditionalPoint;

        let backend = super::super::fake_backend::FakeBackend::new(b"A", b"B").with_hooks(ConditionalHooks::fail_once(
            ConditionalPoint::SaveComplete,
            std::io::ErrorKind::PermissionDenied,
        ));
        let (_lease, target_path) = backend.target_observer();
        let outcome = super::super::commit::commit_conditional::<Sha256, _>(
            backend,
            LocalWritePrecondition::Content(Sha256::digest(b"A")),
            super::super::fake_backend::required_options(),
            1,
        )
        .expect("post-save cleanup failure must remain successful");
        assert_eq!(std::fs::read(target_path).expect("installed target"), b"B");
        assert_eq!(outcome.digest(), &Sha256::digest(b"B"));
        assert_eq!(outcome.cleanup_errors().len(), 1);
        assert!(outcome.retained_files().iter().any(|file| {
            file.role() == crate::recovery::LocalRecoveryFileRole::CandidateCopy && recovery_bytes(file) == b"B"
        }));
        assert!(outcome.retained_files().iter().any(|file| {
            file.role() == crate::recovery::LocalRecoveryFileRole::Displaced && recovery_bytes(file) == b"A"
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
        assert_eq!(error.stage(), crate::error::LocalConditionalWriteStage::Sync);
        assert_eq!(
            error.publication(),
            crate::outcome::LocalConditionalPublicationState::Restored
        );
        assert!(error.candidate().is_some());
        let roles: Vec<_> = error.recovery_files().iter().map(|file| file.role()).collect();
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
            (ConditionalPoint::Publish, ConditionalHookAction::PartialPublish),
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
        assert_eq!(std::fs::read(target_path).expect("in-place competitor remains"), b"C");
        assert_eq!(recovery_bytes(error.candidate().expect("independent B copy")), b"B");
        assert!(error.recovery_files().iter().any(|file| {
            file.role() == crate::recovery::LocalRecoveryFileRole::StagedCandidate && recovery_bytes(file) == b"C"
        }));
        assert!(error.recovery_files().iter().any(|file| {
            file.role() == crate::recovery::LocalRecoveryFileRole::Displaced && recovery_bytes(file) == b"A"
        }));
        assert!(!events.borrow().contains(&super::super::fake_backend::Event::Rollback));
        assert_eq!(error.error().kind(), crate::LocalFileErrorKind::DataCorruption);
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
        assert_eq!(std::fs::read(target_path).expect("competitor remains"), b"C");
        assert_eq!(recovery_bytes(error.candidate().expect("independent B copy")), b"B");
        assert!(error.recovery_files().iter().any(|file| {
            file.role() == crate::recovery::LocalRecoveryFileRole::StagedCandidate && recovery_bytes(file) == b"C"
        }));
        assert!(error.recovery_files().iter().any(|file| {
            file.role() == crate::recovery::LocalRecoveryFileRole::Displaced && recovery_bytes(file) == b"A"
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
        assert_eq!(std::fs::read(target_path).expect("prior target restored"), b"A");
        let source = std::error::Error::source(&error).expect("structured error source");
        let source = std::error::Error::source(source).expect("native I/O source");
        assert_eq!(
            source.downcast_ref::<std::io::Error>().map(std::io::Error::kind),
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
        assert_eq!(error.stage(), crate::error::LocalConditionalWriteStage::Rollback);
        assert_eq!(
            error.publication(),
            crate::outcome::LocalConditionalPublicationState::Published
        );
        assert_eq!(std::fs::read(target_path).expect("installed candidate"), b"B");
        assert!(error.candidate().is_some());
        assert_eq!(recovery_bytes(error.candidate().expect("candidate copy")), b"B");
        assert_eq!(error.error().kind(), crate::LocalFileErrorKind::PermissionDenied);
        assert!(
            error
                .recovery_files()
                .iter()
                .any(|file| file.role() == crate::recovery::LocalRecoveryFileRole::RollbackDisplaced)
        );
        assert!(
            error
                .recovery_files()
                .iter()
                .any(|file| file.role() == crate::recovery::LocalRecoveryFileRole::StagedCandidate)
        );
    }

    #[cfg(feature = "test-support")]
    #[test]
    fn conditional_state_partial_rollback_failure_runs_hook_before_receipts() {
        use super::super::ConditionalHookAction;
        use super::super::ConditionalHooks;
        use super::super::ConditionalPoint;
        use crate::test_support::TestConditionalHooks;
        use crate::test_support::TestConditionalPoint;

        let backend = super::super::fake_backend::FakeBackend::new(b"A", b"B").fail_after_first_rollback_rename_once();
        let (_lease, target_path) = backend.target_observer();
        let hook_target = target_path.clone();
        let hooks = ConditionalHooks::sequence(vec![(
            ConditionalPoint::Publish,
            ConditionalHookAction::ReplaceTarget(b"R".to_vec()),
        )])
        .with_test(TestConditionalHooks::once(
            TestConditionalPoint::AfterRollback,
            move || {
                std::fs::write(hook_target, b"D")?;
                Err(std::io::Error::new(
                    std::io::ErrorKind::PermissionDenied,
                    "injected post-rollback hook error",
                ))
            },
        ));
        let backend = backend.with_hooks(hooks);
        let error = super::super::commit::commit_conditional::<Sha256, _>(
            backend,
            LocalWritePrecondition::Content(Sha256::digest(b"A")),
            super::super::fake_backend::required_options(),
            1,
        )
        .expect_err("partial rollback failure must preserve recovery state");

        assert_eq!(error.stage(), crate::error::LocalConditionalWriteStage::Rollback);
        assert_eq!(
            error.publication(),
            crate::outcome::LocalConditionalPublicationState::Indeterminate
        );
        assert_eq!(std::fs::read(&target_path).expect("hook target side effect"), b"D");
        let displaced = error
            .recovery_files()
            .iter()
            .find(|file| file.role() == crate::recovery::LocalRecoveryFileRole::Displaced)
            .expect("receipt for hook replacement");
        assert_eq!(recovery_bytes(displaced), b"D");
        assert_eq!(recovery_bytes(error.candidate().expect("candidate copy")), b"B");

        let source = std::error::Error::source(&error).expect("structured error source");
        let source = std::error::Error::source(source).expect("native rollback error");
        let native_error = source
            .downcast_ref::<std::io::Error>()
            .expect("rollback source is an I/O error");
        assert_eq!(native_error.kind(), std::io::ErrorKind::Other);
        assert_eq!(
            native_error.to_string(),
            "injected fake rollback syscall failure after first rename"
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
        assert_eq!(error.stage(), crate::error::LocalConditionalWriteStage::Sync);
        assert_eq!(
            error.publication(),
            crate::outcome::LocalConditionalPublicationState::Restored
        );
        assert!(error.candidate().is_some());
        assert_eq!(std::fs::read(target_path).expect("restored target"), b"R");
        assert_eq!(recovery_bytes(error.candidate().expect("candidate copy")), b"B");
        assert!(error.recovery_files().iter().any(|file| {
            file.role() == crate::recovery::LocalRecoveryFileRole::RollbackDisplaced && recovery_bytes(file) == b"B"
        }));
        assert!(error.recovery_files().iter().any(|file| {
            file.role() == crate::recovery::LocalRecoveryFileRole::Displaced && recovery_bytes(file) == b"R"
        }));
        assert_eq!(error.error().kind(), crate::LocalFileErrorKind::PermissionDenied);
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
        assert_eq!(error.stage(), crate::error::LocalConditionalWriteStage::VerifyRollback);
        assert_eq!(
            error.publication(),
            crate::outcome::LocalConditionalPublicationState::Indeterminate
        );
        assert_eq!(std::fs::read(target_path).expect("restored target"), b"R");
        assert_eq!(recovery_bytes(error.candidate().expect("verified B copy")), b"B");
        assert!(error.recovery_files().iter().any(|file| {
            file.role() == crate::recovery::LocalRecoveryFileRole::Displaced && recovery_bytes(file) == b"R"
        }));
        assert!(error.recovery_files().iter().any(|file| {
            file.role() == crate::recovery::LocalRecoveryFileRole::RollbackDisplaced && recovery_bytes(file) == b"B"
        }));
        assert_eq!(error.error().kind(), crate::LocalFileErrorKind::PermissionDenied);
    }

    #[test]
    fn conditional_state_postpublication_readback_failure_retains_all_entries() {
        use super::super::ConditionalHooks;
        use super::super::ConditionalPoint;

        let backend = super::super::fake_backend::FakeBackend::new(b"A", b"B").with_hooks(ConditionalHooks::fail_once(
            ConditionalPoint::VerifyInstalled,
            std::io::ErrorKind::PermissionDenied,
        ));
        let (_lease, target_path) = backend.target_observer();
        let error = super::super::commit::commit_conditional::<Sha256, _>(
            backend,
            LocalWritePrecondition::Content(Sha256::digest(b"A")),
            super::super::fake_backend::required_options(),
            1,
        )
        .expect_err("installed readback failure must fail closed");
        assert_eq!(error.stage(), crate::error::LocalConditionalWriteStage::VerifyInstalled);
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
            file.role() == crate::recovery::LocalRecoveryFileRole::StagedCandidate && recovery_bytes(file) == b"B"
        }));
        assert!(error.recovery_files().iter().any(|file| {
            file.role() == crate::recovery::LocalRecoveryFileRole::Displaced && recovery_bytes(file) == b"A"
        }));
        assert_eq!(error.error().kind(), crate::LocalFileErrorKind::PermissionDenied);
    }

    #[test]
    fn conditional_state_cleanup_failure_is_reported_on_success() {
        use super::super::ConditionalHooks;
        use super::super::ConditionalPoint;

        let backend = super::super::fake_backend::FakeBackend::new(b"A", b"B").with_hooks(ConditionalHooks::fail_once(
            ConditionalPoint::CleanupOld,
            std::io::ErrorKind::PermissionDenied,
        ));
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
    fn conditional_state_not_required_skips_cleanup_sync_checkpoint_and_namespace_sync() {
        use super::super::ConditionalHooks;
        use super::super::ConditionalPoint;
        use crate::options::LocalWriteMode;
        use crate::options::LocalWriteOptions;

        let backend = super::super::fake_backend::FakeBackend::new(b"A", b"B").with_hooks(ConditionalHooks::fail_once(
            ConditionalPoint::CleanupSync,
            std::io::ErrorKind::PermissionDenied,
        ));
        let sync_calls = backend.namespace_sync_observer();
        let (_lease, target_path) = backend.target_observer();
        let outcome = super::super::commit::commit_conditional::<Sha256, _>(
            backend,
            LocalWritePrecondition::Content(Sha256::digest(b"A")),
            LocalWriteOptions::new(LocalWriteMode::CreateOrReplace),
            1,
        )
        .expect("NotRequired cleanup does not run its sync checkpoint");

        assert_eq!(sync_calls.get(), 0);
        assert!(outcome.cleanup_errors().is_empty());
        assert!(outcome.retained_files().is_empty());
        assert!(!outcome.write_outcome().durable());
        assert_eq!(std::fs::read(target_path).expect("saved target"), b"B");
    }

    #[test]
    fn conditional_state_preferred_and_required_cleanup_sync_remains_best_effort() {
        use super::super::ConditionalHookAction;
        use super::super::ConditionalHooks;
        use super::super::ConditionalPoint;
        use crate::options::LocalWriteMode;
        use crate::options::LocalWriteOptions;
        use crate::policy::LocalAtomicityRequirement;
        use crate::policy::LocalDurabilityRequirement;

        for durability in [
            LocalDurabilityRequirement::Preferred,
            LocalDurabilityRequirement::Required,
        ] {
            let options = LocalWriteOptions::new(LocalWriteMode::CreateOrReplace)
                .with_atomicity(LocalAtomicityRequirement::Required)
                .with_durability(durability);
            let backend = super::super::fake_backend::FakeBackend::new(b"A", b"B");
            let baseline_sync_calls = backend.namespace_sync_observer();
            let baseline = super::super::commit::commit_conditional::<Sha256, _>(
                backend,
                LocalWritePrecondition::Content(Sha256::digest(b"A")),
                options,
                1,
            )
            .expect("successful cleanup sync should complete");
            assert_eq!(baseline_sync_calls.get(), 3, "{durability:?} sync baseline");
            assert!(baseline.cleanup_errors().is_empty());

            let checkpoint_backend = super::super::fake_backend::FakeBackend::new(b"A", b"B").with_hooks(
                ConditionalHooks::fail_once(ConditionalPoint::CleanupSync, std::io::ErrorKind::PermissionDenied),
            );
            let checkpoint_sync_calls = checkpoint_backend.namespace_sync_observer();
            let (_lease, checkpoint_target) = checkpoint_backend.target_observer();
            let checkpoint_outcome = super::super::commit::commit_conditional::<Sha256, _>(
                checkpoint_backend,
                LocalWritePrecondition::Content(Sha256::digest(b"A")),
                options,
                1,
            )
            .expect("cleanup checkpoint failure must preserve published success");
            assert_eq!(checkpoint_sync_calls.get(), 2, "{durability:?} checkpoint failure");
            assert!(!checkpoint_outcome.cleanup_errors().is_empty());
            assert!(checkpoint_outcome.retained_files().is_empty());
            assert_eq!(std::fs::read(checkpoint_target).expect("published target"), b"B");

            let sync_hooks = ConditionalHooks::sequence(vec![
                (ConditionalPoint::SyncNamespace, ConditionalHookAction::Noop),
                (ConditionalPoint::SyncNamespace, ConditionalHookAction::Noop),
                (
                    ConditionalPoint::SyncNamespace,
                    ConditionalHookAction::Fail(std::io::ErrorKind::PermissionDenied),
                ),
            ]);
            let sync_backend = super::super::fake_backend::FakeBackend::new(b"A", b"B").with_hooks(sync_hooks);
            let sync_calls = sync_backend.namespace_sync_observer();
            let (_lease, sync_target) = sync_backend.target_observer();
            let sync_outcome = super::super::commit::commit_conditional::<Sha256, _>(
                sync_backend,
                LocalWritePrecondition::Content(Sha256::digest(b"A")),
                options,
                1,
            )
            .expect("cleanup namespace sync failure must preserve published success");
            assert_eq!(sync_calls.get(), 3, "{durability:?} namespace sync count");
            assert!(!sync_outcome.cleanup_errors().is_empty());
            assert!(sync_outcome.retained_files().is_empty());
            assert_eq!(std::fs::read(sync_target).expect("published target"), b"B");
        }
    }

    #[test]
    fn conditional_state_not_required_still_reports_real_cleanup_failure() {
        use super::super::ConditionalHooks;
        use super::super::ConditionalPoint;
        use crate::options::LocalWriteMode;
        use crate::options::LocalWriteOptions;

        let backend = super::super::fake_backend::FakeBackend::new(b"A", b"B").with_hooks(ConditionalHooks::fail_once(
            ConditionalPoint::CleanupOld,
            std::io::ErrorKind::PermissionDenied,
        ));
        let (_lease, target_path) = backend.target_observer();
        let outcome = super::super::commit::commit_conditional::<Sha256, _>(
            backend,
            LocalWritePrecondition::Content(Sha256::digest(b"A")),
            LocalWriteOptions::new(LocalWriteMode::CreateOrReplace),
            1,
        )
        .expect("real deletion failure remains a successful commit outcome");

        assert_eq!(outcome.cleanup_errors().len(), 1);
        assert_eq!(outcome.retained_files().len(), 1);
        assert_eq!(
            outcome.retained_files()[0].role(),
            crate::recovery::LocalRecoveryFileRole::Displaced
        );
        assert_eq!(std::fs::read(target_path).expect("published target"), b"B");
    }

    #[test]
    fn conditional_state_cleanup_sync_failure_after_copy_delete_is_reported_as_ok() {
        use super::super::ConditionalHooks;
        use super::super::ConditionalPoint;

        let backend = super::super::fake_backend::FakeBackend::new(b"A", b"B").with_hooks(ConditionalHooks::fail_once(
            ConditionalPoint::CleanupSync,
            std::io::ErrorKind::PermissionDenied,
        ));
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
        assert_eq!(error.stage(), crate::error::LocalConditionalWriteStage::Precheck);
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
        assert_eq!(std::fs::read(target_path).expect("racing target stays"), b"racer");
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
        assert_eq!(error.stage(), crate::error::LocalConditionalWriteStage::Precheck);
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

        let backend = super::super::fake_backend::FakeBackend::new(b"A", b"B").with_hooks(ConditionalHooks::fail_once(
            ConditionalPoint::PrepareCandidateCopy,
            std::io::ErrorKind::PermissionDenied,
        ));
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
            let backend = super::super::fake_backend::FakeBackend::new(b"A", b"B")
                .with_hooks(ConditionalHooks::fail_once(point, std::io::ErrorKind::PermissionDenied));
            let (_lease, target_path) = backend.target_observer();
            let error = super::super::commit::commit_conditional::<Sha256, _>(
                backend,
                LocalWritePrecondition::Content(Sha256::digest(b"A")),
                super::super::fake_backend::required_options(),
                1,
            )
            .expect_err("every required prepublication sync must be enforced");
            assert_eq!(error.stage(), crate::error::LocalConditionalWriteStage::Sync);
            assert_eq!(
                error.publication(),
                crate::outcome::LocalConditionalPublicationState::NotPublished
            );
            assert_eq!(error.error().kind(), crate::LocalFileErrorKind::PermissionDenied);
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
        let backend = super::super::fake_backend::FakeBackend::new(b"A", b"B").with_hooks(ConditionalHooks::fail_once(
            ConditionalPoint::SyncCandidate,
            std::io::ErrorKind::PermissionDenied,
        ));
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

        let backend = super::super::fake_backend::FakeBackend::new(b"A", b"B").with_hooks(ConditionalHooks::fail_once(
            ConditionalPoint::CleanupCopy,
            std::io::ErrorKind::PermissionDenied,
        ));
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
