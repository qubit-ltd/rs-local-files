// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::Path;

use digest::Digest;
use qubit_io::Output;
use qubit_io::Streams;
use sha2::Sha256;

use super::super::commit::commit_conditional;
use super::UnixConditionalBackend;
use super::namespace::Namespace;
use super::test_support;
use crate::local::internal::conditional::ConditionalBackend;
use crate::local::internal::conditional::ConditionalHookAction;
use crate::local::internal::conditional::ConditionalHooks;
use crate::local::internal::conditional::ConditionalPoint;
use crate::options::LocalWriteMode;
use crate::options::LocalWriteOptions;
use crate::options::LocalWritePrecondition;

fn private_entries(directory: &Path, marker: &str) -> Vec<std::path::PathBuf> {
    fs::read_dir(directory)
        .expect("fixture directory should be readable")
        .map(|entry| entry.expect("directory entry should be readable").path())
        .filter(|path| {
            path.file_name()
                .is_some_and(|name| name.to_string_lossy().contains(marker))
        })
        .collect()
}

fn exchange_backend() -> (tempfile::TempDir, UnixConditionalBackend) {
    let directory = tempfile::tempdir().expect("fixture directory should be created");
    let target = directory.path().join("target");
    fs::write(&target, b"original").expect("initial target should be written");
    let mut backend =
        UnixConditionalBackend::open_host(&target, &LocalWriteOptions::default()).expect("backend should open");
    backend
        .candidate
        .write_fully(b"candidate")
        .expect("candidate should be written");
    (directory, backend)
}

fn recovery_entry_bytes(entry: &super::super::ConditionalEntry) -> Vec<u8> {
    let mut reader = entry.open_reader().expect("recovery entry should remain readable");
    let mut bytes = Vec::new();
    Streams::copy_input_to_output(&mut reader, &mut bytes).expect("recovery entry bytes should be readable");
    bytes
}

#[test]
fn conditional_unix_candidate_setup_failures_cleanup_or_report_the_candidate() {
    for (point, expected_residue) in [
        (test_support::FailurePoint::GuardIdentityMetadata, true),
        (test_support::FailurePoint::CandidateMetadata, false),
        (test_support::FailurePoint::EntryIdentity, false),
        (test_support::FailurePoint::EntryParentClone, false),
    ] {
        let directory = tempfile::tempdir().expect("fixture directory should be created");
        let namespace = Namespace::open_host(&directory.path().join("target"), &LocalWriteOptions::default())
            .expect("namespace should open");
        test_support::fail_next(point);

        let error = UnixConditionalBackend::open(namespace).expect_err("injected setup step should fail");

        assert!(
            error.to_string().contains("injected"),
            "failure source should be retained: {error}"
        );
        let candidates = private_entries(directory.path(), ".qubit-candidate-");
        assert_eq!(
            !candidates.is_empty(),
            expected_residue,
            "unexpected residue for {point:?}: {candidates:?}"
        );
        if expected_residue {
            assert!(
                error.to_string().contains(&candidates[0].display().to_string()),
                "unremovable candidate path should be reported: {error}"
            );
        }
    }
}

#[test]
fn conditional_unix_candidate_copy_registration_failure_cleans_copy() {
    let directory = tempfile::tempdir().expect("fixture directory should be created");
    let target = directory.path().join("target");
    let mut backend =
        UnixConditionalBackend::open_host(&target, &LocalWriteOptions::default()).expect("backend should open");
    backend
        .candidate
        .write_fully(b"candidate bytes")
        .expect("candidate should be writable");
    test_support::fail_next(test_support::FailurePoint::CandidateCopyRegistration);

    let error = backend
        .create_candidate_copy()
        .expect_err("injected candidate-copy registration should fail");

    assert!(error.to_string().contains("injected CandidateCopyRegistration failure"));
    assert!(
        private_entries(directory.path(), ".qubit-candidate-copy-").is_empty(),
        "candidate copy should not remain unreported"
    );
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn conditional_unix_content_exchange_publishes_candidate_and_returns_old_bytes() {
    let directory = tempfile::tempdir().expect("fixture directory should be created");
    let target = directory.path().join("target");
    fs::write(&target, b"A").expect("initial target should be written");
    let mut backend =
        UnixConditionalBackend::open_host(&target, &LocalWriteOptions::default()).expect("backend should open");
    backend
        .candidate
        .write_fully(b"B")
        .expect("candidate should be written");
    let result = commit_conditional::<Sha256, _>(
        backend,
        LocalWritePrecondition::Content(Sha256::digest(b"A")),
        LocalWriteOptions::new(LocalWriteMode::CreateOrReplace),
        1,
    );
    assert!(result.is_ok(), "commit should succeed: {result:?}");
    assert_eq!(fs::read(&target).expect("published target should be readable"), b"B");
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn test_conditional_unknown_identity_remains_indeterminate() {
    let directory = tempfile::tempdir().expect("fixture directory should be created");
    let target = directory.path().join("target");
    fs::write(&target, b"original").expect("initial target should be written");
    let mut backend =
        UnixConditionalBackend::open_host(&target, &LocalWriteOptions::default()).expect("backend should open");
    backend
        .candidate
        .write_fully(b"candidate")
        .expect("candidate should be written");

    for point in [
        test_support::FailurePoint::ExchangeEioAfterSuccess,
        test_support::FailurePoint::CandidateIdentityAfterExchange,
        test_support::FailurePoint::InstalledIdentityAfterExchange,
        test_support::FailurePoint::StagedIdentityAfterExchange,
    ] {
        test_support::fail_next(point);
    }
    let error = commit_conditional::<Sha256, _>(
        backend,
        LocalWritePrecondition::Content(Sha256::digest(b"original")),
        LocalWriteOptions::new(LocalWriteMode::CreateOrReplace),
        1,
    )
    .expect_err("injected exchange error should be reported");

    assert_eq!(
        error.error().io_error().and_then(std::io::Error::raw_os_error),
        Some(libc::EIO)
    );
    assert_eq!(
        error.publication(),
        crate::outcome::LocalConditionalPublicationState::Indeterminate
    );
    assert_eq!(
        fs::read(&target).expect("installed target should remain readable"),
        b"candidate"
    );
    assert!(
        error
            .recovery_files()
            .iter()
            .any(|entry| { entry.role() == crate::recovery::LocalRecoveryFileRole::StagedCandidate }),
        "uncertain staged candidate should remain available for recovery"
    );
    test_support::assert_no_pending_failures();
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn test_conditional_unknown_candidate_does_not_authorize_not_published() {
    let directory = tempfile::tempdir().expect("fixture directory should be created");
    let target = directory.path().join("target");
    fs::write(&target, b"original").expect("initial target should be written");
    let mut backend =
        UnixConditionalBackend::open_host(&target, &LocalWriteOptions::default()).expect("backend should open");
    backend
        .candidate
        .write_fully(b"candidate")
        .expect("candidate should be written");
    test_support::fail_next(test_support::FailurePoint::ExchangeEioBeforeSuccess);
    test_support::fail_next(test_support::FailurePoint::CandidateIdentityAfterExchange);

    let error = commit_conditional::<Sha256, _>(
        backend,
        LocalWritePrecondition::Content(Sha256::digest(b"original")),
        LocalWriteOptions::new(LocalWriteMode::CreateOrReplace),
        1,
    )
    .expect_err("injected exchange error should be reported");

    assert_eq!(
        error.error().io_error().and_then(std::io::Error::raw_os_error),
        Some(libc::EIO)
    );
    assert_eq!(
        error.publication(),
        crate::outcome::LocalConditionalPublicationState::Indeterminate
    );
    assert_eq!(
        fs::read(&target).expect("original target should remain readable"),
        b"original"
    );
    assert!(
        error
            .recovery_files()
            .iter()
            .any(|entry| { entry.role() == crate::recovery::LocalRecoveryFileRole::StagedCandidate }),
        "unknown candidate identity must retain the staged recovery entry"
    );
    test_support::assert_no_pending_failures();
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn conditional_unix_exchange_identity_state_matrix_preserves_error_and_receipts() {
    use crate::outcome::LocalConditionalPublicationState;
    use crate::recovery::LocalRecoveryFileRole;

    let target = |directory: &tempfile::TempDir| directory.path().join("target");

    // Candidate identity equals the installed identity after a real exchange.
    let (directory, mut backend) = exchange_backend();
    backend.begin_publication().expect("publication should begin");
    test_support::fail_next(test_support::FailurePoint::ExchangeEioAfterSuccess);
    let attempt = backend.publish_content();
    assert_eq!(attempt.publication, LocalConditionalPublicationState::Published);
    assert_eq!(
        attempt.error.as_ref().and_then(std::io::Error::raw_os_error),
        Some(libc::EIO)
    );
    let installed = attempt
        .installed
        .as_ref()
        .expect("installed candidate receipt should be retained");
    assert_eq!(installed.role(), LocalRecoveryFileRole::StagedCandidate);
    assert_eq!(recovery_entry_bytes(installed), b"candidate");
    assert_eq!(
        attempt.displaced.len(),
        1,
        "displaced target receipt should be retained"
    );
    assert_eq!(attempt.displaced[0].role(), LocalRecoveryFileRole::Displaced);
    assert_eq!(recovery_entry_bytes(&attempt.displaced[0]), b"original");
    assert_eq!(
        fs::read(target(&directory)).expect("target should remain readable"),
        b"candidate"
    );
    test_support::assert_no_pending_failures();

    // The installed identity is known but different from the candidate; an
    // unknown staged identity cannot turn that mismatch into equality.
    let (directory, mut backend) = exchange_backend();
    backend.begin_publication().expect("publication should begin");
    test_support::fail_next(test_support::FailurePoint::ExchangeEioBeforeSuccess);
    test_support::fail_next(test_support::FailurePoint::StagedIdentityAfterExchange);
    let attempt = backend.publish_content();
    assert_eq!(attempt.publication, LocalConditionalPublicationState::Indeterminate);
    assert_eq!(
        attempt.error.as_ref().and_then(std::io::Error::raw_os_error),
        Some(libc::EIO)
    );
    let installed = attempt
        .installed
        .as_ref()
        .expect("observed target receipt should be retained");
    assert_eq!(recovery_entry_bytes(installed), b"original");
    assert_eq!(attempt.displaced.len(), 1, "candidate receipt should be retained");
    assert_eq!(recovery_entry_bytes(&attempt.displaced[0]), b"candidate");
    assert_eq!(
        fs::read(target(&directory)).expect("target should remain readable"),
        b"original"
    );
    test_support::assert_no_pending_failures();

    // A single unknown installed identity remains indeterminate when the
    // staged entry is known to contain the displaced original.
    let (directory, mut backend) = exchange_backend();
    backend.begin_publication().expect("publication should begin");
    test_support::fail_next(test_support::FailurePoint::ExchangeEioAfterSuccess);
    test_support::fail_next(test_support::FailurePoint::InstalledIdentityAfterExchange);
    let attempt = backend.publish_content();
    assert_eq!(attempt.publication, LocalConditionalPublicationState::Indeterminate);
    assert_eq!(
        attempt.error.as_ref().and_then(std::io::Error::raw_os_error),
        Some(libc::EIO)
    );
    let installed = attempt
        .installed
        .as_ref()
        .expect("installed receipt should be retained");
    assert_eq!(recovery_entry_bytes(installed), b"candidate");
    assert_eq!(attempt.displaced.len(), 1, "displaced receipt should be retained");
    assert_eq!(recovery_entry_bytes(&attempt.displaced[0]), b"original");
    assert_eq!(
        fs::read(target(&directory)).expect("target should remain readable"),
        b"candidate"
    );
    test_support::assert_no_pending_failures();

    // On EIO before the exchange, the staged candidate's known identity is a
    // positive NotPublished receipt; the backend still owns the candidate.
    let (directory, mut backend) = exchange_backend();
    backend.begin_publication().expect("publication should begin");
    test_support::fail_next(test_support::FailurePoint::ExchangeEioBeforeSuccess);
    let attempt = backend.publish_content();
    assert_eq!(attempt.publication, LocalConditionalPublicationState::NotPublished);
    assert_eq!(
        attempt.error.as_ref().and_then(std::io::Error::raw_os_error),
        Some(libc::EIO)
    );
    assert!(
        attempt.installed.is_none(),
        "NotPublished must not transfer an installed receipt"
    );
    assert!(
        attempt.displaced.is_empty(),
        "NotPublished must not transfer displaced receipts"
    );
    assert_eq!(backend.candidate_entry.role(), LocalRecoveryFileRole::StagedCandidate);
    assert_eq!(recovery_entry_bytes(&backend.candidate_entry), b"candidate");
    assert_eq!(
        fs::read(target(&directory)).expect("original target should remain readable"),
        b"original"
    );
    test_support::assert_no_pending_failures();
}

#[test]
fn conditional_unix_absent_install_does_not_overwrite_a_racing_creator() {
    let directory = tempfile::tempdir().expect("fixture directory should be created");
    let target = directory.path().join("target");
    let mut backend =
        UnixConditionalBackend::open_host(&target, &LocalWriteOptions::default()).expect("backend should open");
    backend
        .candidate
        .write_fully(b"B")
        .expect("candidate should be written");
    let result = commit_conditional::<Sha256, _>(
        backend,
        LocalWritePrecondition::Absent,
        LocalWriteOptions::new(LocalWriteMode::CreateNew),
        1,
    );
    assert!(result.is_ok(), "absent install should succeed: {result:?}");
    assert_eq!(fs::read(&target).expect("installed target should be readable"), b"B");
}

#[test]
fn conditional_unix_required_durability_publishes_with_nested_parents() {
    use crate::policy::LocalDurabilityRequirement;

    let directory = tempfile::tempdir().expect("fixture directory should be created");
    let target = directory.path().join("one").join("two").join("target");
    let options = LocalWriteOptions::new(LocalWriteMode::CreateNew)
        .with_create_parent()
        .with_durability(LocalDurabilityRequirement::Required);
    let mut backend = UnixConditionalBackend::open_host(&target, &options).expect("backend should create parents");
    backend
        .candidate
        .write_fully(b"B")
        .expect("candidate should be written");

    let result = commit_conditional::<Sha256, _>(backend, LocalWritePrecondition::Absent, options, 1);

    assert!(result.is_ok(), "required namespace sync should succeed: {result:?}");
    assert_eq!(fs::read(&target).expect("published target should be readable"), b"B");
}

#[test]
fn conditional_unix_absent_precheck_race_keeps_the_racing_file() {
    let directory = tempfile::tempdir().expect("fixture directory should be created");
    let target = directory.path().join("target");
    let mut backend =
        UnixConditionalBackend::open_host(&target, &LocalWriteOptions::default()).expect("backend should open");
    backend
        .candidate
        .write_fully(b"B")
        .expect("candidate should be written");
    backend.set_hooks(ConditionalHooks::sequence(vec![(
        ConditionalPoint::Precheck,
        ConditionalHookAction::ReplaceTarget(b"racer".to_vec()),
    )]));
    let error = commit_conditional::<Sha256, _>(
        backend,
        LocalWritePrecondition::Absent,
        LocalWriteOptions::new(LocalWriteMode::CreateNew),
        1,
    )
    .expect_err("racing target should reject absent publication");
    assert_eq!(
        error.publication(),
        crate::outcome::LocalConditionalPublicationState::NotPublished
    );
    assert_eq!(fs::read(&target).expect("racing target should remain"), b"racer");
    assert!(
        error
            .recovery_files()
            .iter()
            .any(|file| file.role() == crate::recovery::LocalRecoveryFileRole::CandidateCopy)
    );
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn conditional_unix_rooted_parent_rename_keeps_the_open_namespace() {
    let directory = tempfile::tempdir().expect("fixture directory should be created");
    let root_path = directory.path().join("root");
    let moved_path = directory.path().join("moved-root");
    fs::create_dir(&root_path).expect("root directory should be created");
    fs::write(root_path.join("target"), b"A").expect("target should be written");
    let root = std::fs::File::open(&root_path).expect("root capability should open");
    let diagnostic = root_path.join("target");
    let mut backend = UnixConditionalBackend::open_rooted(
        root,
        Path::new("target"),
        &diagnostic,
        Some(&diagnostic),
        &LocalWriteOptions::default(),
    )
    .expect("rooted backend should open");
    backend
        .candidate
        .write_fully(b"B")
        .expect("candidate should be written");
    fs::rename(&root_path, &moved_path).expect("root should be renamed");
    let result = commit_conditional::<Sha256, _>(
        backend,
        LocalWritePrecondition::Content(Sha256::digest(b"A")),
        LocalWriteOptions::new(LocalWriteMode::CreateOrReplace),
        1,
    );
    assert!(
        result.is_ok(),
        "rooted commit should use the retained namespace: {result:?}"
    );
    assert_eq!(
        fs::read(moved_path.join("target")).expect("renamed root target should be readable"),
        b"B"
    );
}

#[test]
fn conditional_unix_precheck_same_inode_rewrite_is_detected_and_retained() {
    let directory = tempfile::tempdir().expect("fixture directory should be created");
    let target = directory.path().join("target");
    fs::write(&target, b"A").expect("initial target should be written");
    let original_inode = fs::metadata(&target).expect("target metadata should be readable").ino();
    let mut backend =
        UnixConditionalBackend::open_host(&target, &LocalWriteOptions::default()).expect("backend should open");
    backend
        .candidate
        .write_fully(b"B")
        .expect("candidate should be written");
    backend.set_hooks(ConditionalHooks::sequence(vec![(
        ConditionalPoint::Precheck,
        ConditionalHookAction::OverwriteTargetInPlace(b"C".to_vec()),
    )]));
    let error = commit_conditional::<Sha256, _>(
        backend,
        LocalWritePrecondition::Content(Sha256::digest(b"A")),
        LocalWriteOptions::new(LocalWriteMode::CreateOrReplace),
        1,
    )
    .expect_err("in-place target rewrite must not pass the content precondition");
    assert_eq!(
        error.publication(),
        crate::outcome::LocalConditionalPublicationState::NotPublished
    );
    assert_eq!(fs::read(&target).expect("restored target should be readable"), b"C");
    assert_eq!(
        original_inode,
        fs::metadata(&target).expect("target metadata should be readable").ino()
    );
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn conditional_unix_precheck_replacement_inode_with_matching_content_can_publish() {
    let directory = tempfile::tempdir().expect("fixture directory should be created");
    let target = directory.path().join("target");
    fs::write(&target, b"A").expect("initial target should be written");
    let original_inode = fs::metadata(&target).expect("target metadata should be readable").ino();
    let mut backend =
        UnixConditionalBackend::open_host(&target, &LocalWriteOptions::default()).expect("backend should open");
    backend
        .candidate
        .write_fully(b"B")
        .expect("candidate should be written");
    backend.set_hooks(ConditionalHooks::sequence(vec![(
        ConditionalPoint::Precheck,
        ConditionalHookAction::ReplaceTarget(b"A".to_vec()),
    )]));
    let result = commit_conditional::<Sha256, _>(
        backend,
        LocalWritePrecondition::Content(Sha256::digest(b"A")),
        LocalWriteOptions::new(LocalWriteMode::CreateOrReplace),
        1,
    );
    assert!(
        result.is_ok(),
        "matching content on a replacement inode should satisfy Content: {result:?}"
    );
    assert_ne!(
        original_inode,
        fs::metadata(&target)
            .expect("published metadata should be readable")
            .ino()
    );
    assert_eq!(fs::read(&target).expect("published target should be readable"), b"B");
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn conditional_unix_preserves_read_only_mode_and_handles_empty_and_large_candidates() {
    use std::os::unix::fs::PermissionsExt;
    let directory = tempfile::tempdir().expect("fixture directory should be created");
    let target = directory.path().join("target");
    fs::write(&target, b"old").expect("initial target should be written");
    fs::set_permissions(&target, fs::Permissions::from_mode(0o444)).expect("target mode should be restricted");
    let payload = vec![0x5a; 2 * 1024 * 1024];
    let mut backend =
        UnixConditionalBackend::open_host(&target, &LocalWriteOptions::default()).expect("backend should open");
    backend
        .candidate
        .write_fully(&payload)
        .expect("large candidate should be written");
    let result = commit_conditional::<Sha256, _>(
        backend,
        LocalWritePrecondition::Content(Sha256::digest(b"old")),
        LocalWriteOptions::new(LocalWriteMode::CreateOrReplace),
        payload.len() as u64,
    );
    assert!(result.is_ok(), "large candidate should publish: {result:?}");
    assert_eq!(fs::read(&target).expect("large target should be readable"), payload);
    assert_eq!(
        0o444,
        fs::metadata(&target)
            .expect("target metadata should be readable")
            .permissions()
            .mode()
            & 0o777
    );

    fs::set_permissions(&target, fs::Permissions::from_mode(0o644))
        .expect("target mode should become writable for the next fixture");
    fs::write(&target, b"empty-source").expect("empty-source target should be written");
    let mut backend =
        UnixConditionalBackend::open_host(&target, &LocalWriteOptions::default()).expect("backend should reopen");
    backend
        .candidate
        .write_fully(b"")
        .expect("empty candidate should need no bytes");
    let result = commit_conditional::<Sha256, _>(
        backend,
        LocalWritePrecondition::Content(Sha256::digest(b"empty-source")),
        LocalWriteOptions::new(LocalWriteMode::CreateOrReplace),
        0,
    );
    assert!(result.is_ok(), "empty candidate should publish: {result:?}");
    assert_eq!(fs::read(&target).expect("empty target should be readable"), b"");
}

#[test]
fn conditional_unix_prepublication_error_retains_candidate_and_copy_after_error_drop() {
    let directory = tempfile::tempdir().expect("fixture directory should be created");
    let target = directory.path().join("target");
    fs::write(&target, b"A").expect("initial target should be written");
    let mut backend =
        UnixConditionalBackend::open_host(&target, &LocalWriteOptions::default()).expect("backend should open");
    backend
        .candidate
        .write_fully(b"B")
        .expect("candidate should be written");
    backend.set_hooks(ConditionalHooks::sequence(vec![(
        ConditionalPoint::Publish,
        ConditionalHookAction::Fail(std::io::ErrorKind::Unsupported),
    )]));
    let error = commit_conditional::<Sha256, _>(
        backend,
        LocalWritePrecondition::Content(Sha256::digest(b"A")),
        LocalWriteOptions::new(LocalWriteMode::CreateOrReplace),
        1,
    )
    .expect_err("unsupported publication should fail before the syscall");
    assert_eq!(
        error.publication(),
        crate::outcome::LocalConditionalPublicationState::NotPublished
    );
    for role in [
        crate::recovery::LocalRecoveryFileRole::CandidateCopy,
        crate::recovery::LocalRecoveryFileRole::StagedCandidate,
    ] {
        let recovery = error
            .recovery_files()
            .iter()
            .find(|file| file.role() == role)
            .expect("each B recovery object should be reported");
        let mut reader = recovery.open_reader().expect("recovery object should remain readable");
        let mut bytes = Vec::new();
        Streams::copy_input_to_output(&mut reader, &mut bytes).expect("recovery bytes should be readable");
        assert_eq!(bytes, b"B");
    }
    drop(error);
    assert_eq!(fs::read(&target).expect("original target should remain"), b"A");
    assert_eq!(
        3,
        fs::read_dir(directory.path())
            .expect("directory should be readable")
            .count()
    );
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn conditional_unix_rollback_window_replacement_is_not_exchanged_or_reported_restored() {
    let directory = tempfile::tempdir().expect("fixture directory should be created");
    let target = directory.path().join("target");
    fs::write(&target, b"A").expect("initial target should be written");
    let mut backend =
        UnixConditionalBackend::open_host(&target, &LocalWriteOptions::default()).expect("backend should open");
    backend
        .candidate
        .write_fully(b"B")
        .expect("candidate should be written");
    backend.set_hooks(ConditionalHooks::sequence(vec![
        (ConditionalPoint::Publish, ConditionalHookAction::PartialPublish),
        (
            ConditionalPoint::RollbackPrecheck,
            ConditionalHookAction::ReplaceTarget(b"C".to_vec()),
        ),
    ]));
    let error = commit_conditional::<Sha256, _>(
        backend,
        LocalWritePrecondition::Content(Sha256::digest(b"A")),
        LocalWriteOptions::new(LocalWriteMode::CreateOrReplace),
        1,
    )
    .expect_err("the competing rollback-window target must prevent restoration");
    assert_eq!(
        error.publication(),
        crate::outcome::LocalConditionalPublicationState::Indeterminate
    );
    assert_eq!(
        fs::read(&target).expect("competing target should remain in place"),
        b"C"
    );
    let recovered: Vec<_> = error
        .recovery_files()
        .iter()
        .map(|file| {
            let mut reader = file.open_reader().expect("every recovery entry must remain readable");
            let mut content = Vec::new();
            Streams::copy_input_to_output(&mut reader, &mut content).expect("every recovery entry should be readable");
            (file.role(), file.namespace_path().to_owned(), content)
        })
        .collect();
    assert_eq!(recovered.len(), 2, "unexpected recovery entries: {recovered:?}");
    assert!(
        recovered.iter().all(|(_, _, content)| content != b"C"),
        "the target that failed rollback precheck is not a rollback receipt: {recovered:?}"
    );
    let original_entries: Vec<_> = recovered
        .iter()
        .filter(|(role, _, content)| *role == crate::recovery::LocalRecoveryFileRole::Displaced && content == b"A")
        .collect();
    assert_eq!(
        original_entries.len(),
        1,
        "the original displaced entry should not be duplicated: {recovered:?}"
    );
    assert_ne!(original_entries[0].1, target);
    let candidate_copies: Vec<_> = recovered
        .iter()
        .filter(|(role, _, content)| *role == crate::recovery::LocalRecoveryFileRole::CandidateCopy && content == b"B")
        .collect();
    assert_eq!(candidate_copies.len(), 1, "unexpected candidate copy: {recovered:?}");
    assert_ne!(candidate_copies[0].1, target);
    assert!(recovered.iter().all(|(role, _, _)| {
        matches!(
            role,
            crate::recovery::LocalRecoveryFileRole::Displaced | crate::recovery::LocalRecoveryFileRole::CandidateCopy
        )
    }));
}

#[cfg(any(target_os = "linux", target_os = "macos"))]
#[test]
fn conditional_unix_rollback_exchange_window_reports_actual_namespace_receipts() {
    let directory = tempfile::tempdir().expect("fixture directory should be created");
    let target = directory.path().join("target");
    fs::write(&target, b"A").expect("initial target should be written");
    let mut backend =
        UnixConditionalBackend::open_host(&target, &LocalWriteOptions::default()).expect("backend should open");
    backend
        .candidate
        .write_fully(b"B")
        .expect("candidate should be written");
    backend.set_hooks(ConditionalHooks::sequence(vec![
        (ConditionalPoint::Publish, ConditionalHookAction::PartialPublish),
        (
            ConditionalPoint::RollbackExchange,
            ConditionalHookAction::ReplaceTarget(b"C".to_vec()),
        ),
    ]));
    let error = commit_conditional::<Sha256, _>(
        backend,
        LocalWritePrecondition::Content(Sha256::digest(b"A")),
        LocalWriteOptions::new(LocalWriteMode::CreateOrReplace),
        1,
    )
    .expect_err("exchange-window competition must prevent a restored outcome");
    assert_eq!(
        error.publication(),
        crate::outcome::LocalConditionalPublicationState::Indeterminate
    );
    assert_eq!(
        fs::read(&target).expect("the exchanged-back original should remain"),
        b"A"
    );
    let recovered: Vec<_> = error
        .recovery_files()
        .iter()
        .map(|file| {
            let mut reader = file.open_reader().expect("every recovery entry must remain readable");
            let mut content = Vec::new();
            Streams::copy_input_to_output(&mut reader, &mut content).expect("every recovery entry should be readable");
            (file.role(), file.namespace_path().to_owned(), content)
        })
        .collect();
    assert_eq!(recovered.len(), 3, "unexpected recovery entries: {recovered:?}");
    let original_entries: Vec<_> = recovered
        .iter()
        .filter(|(role, path, content)| {
            *role == crate::recovery::LocalRecoveryFileRole::Displaced && path == &target && content == b"A"
        })
        .collect();
    assert_eq!(original_entries.len(), 1, "unexpected original entry: {recovered:?}");
    let candidate_copies: Vec<_> = recovered
        .iter()
        .filter(|(role, path, content)| {
            *role == crate::recovery::LocalRecoveryFileRole::CandidateCopy && path != &target && content == b"B"
        })
        .collect();
    assert_eq!(candidate_copies.len(), 1, "unexpected candidate copy: {recovered:?}");
    let rollback_displaced: Vec<_> = recovered
        .iter()
        .filter(|(role, path, content)| {
            *role == crate::recovery::LocalRecoveryFileRole::RollbackDisplaced
                && path.file_name().is_some_and(|name| name != "target")
                && content == b"C"
        })
        .collect();
    assert_eq!(
        rollback_displaced.len(),
        1,
        "unexpected rollback-displaced entry: {recovered:?}"
    );
    assert!(recovered.iter().all(|(role, _, _)| {
        matches!(
            role,
            crate::recovery::LocalRecoveryFileRole::Displaced
                | crate::recovery::LocalRecoveryFileRole::CandidateCopy
                | crate::recovery::LocalRecoveryFileRole::RollbackDisplaced
        )
    }));
}
