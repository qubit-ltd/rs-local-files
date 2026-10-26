// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::fs;
use std::io;
use std::os::windows::ffi::OsStrExt;
use std::os::windows::fs::OpenOptionsExt;
use std::path::Path;

use digest::Digest;
use qubit_io::Streams;
use sha2::Sha256;
use sha2::digest::Output;
use windows_sys::Win32::Foundation::ERROR_SHARING_VIOLATION;

use super::PrivateEntryGuard;
use super::WindowsConditionalBackend;
use super::combine_observation_error;
use super::namespace::Namespace;
use super::open_verified_candidate_entry;
use super::receipt_publication;
use super::recovery::ReplaceFailure;
use super::recovery::classify_replace_failure;
use crate::local::internal::conditional::ConditionalBackend;
use crate::local::internal::conditional::ConditionalHookAction;
use crate::local::internal::conditional::ConditionalHooks;
use crate::local::internal::conditional::ConditionalPoint;
use crate::local::internal::conditional::commit::commit_conditional;
use crate::options::LocalWriteMode;
use crate::options::LocalWriteOptions;
use crate::options::LocalWritePrecondition;
use crate::outcome::LocalConditionalPublicationState;
use crate::policy::LocalAtomicityRequirement;
use crate::recovery::LocalRecoveryFileRole;

fn options(mode: LocalWriteMode) -> LocalWriteOptions {
    LocalWriteOptions::new(mode)
        .with_atomicity(LocalAtomicityRequirement::Required)
        .with_durability(crate::policy::LocalDurabilityRequirement::NotRequired)
}

fn put(backend: &mut WindowsConditionalBackend, bytes: &[u8]) {
    qubit_io::Output::write_fully(backend, bytes).expect("candidate bytes should be accepted");
}

fn digest(bytes: &[u8]) -> Output<Sha256> {
    Sha256::digest(bytes)
}

#[test]
fn conditional_windows_candidate_reopen_rejects_replaced_name() {
    let directory = tempfile::tempdir().expect("fixture directory should be created");
    let target = directory.path().join("target");
    let namespace =
        Namespace::open_host(&target, &options(LocalWriteMode::CreateOrReplace)).expect("Host namespace should open");
    let (name, original) = namespace
        .create_private("candidate")
        .expect("candidate should be created exclusively");
    let candidate_path = namespace
        .private_file_path(&name)
        .expect("candidate path should resolve");
    fs::remove_file(&candidate_path).expect("original candidate name should be removed");
    fs::write(&candidate_path, b"other user's entry").expect("replacement entry should be created");

    let error = open_verified_candidate_entry(&namespace, &name, &original)
        .expect_err("reopening a replaced candidate name must fail identity verification");
    assert_eq!(error.kind(), io::ErrorKind::InvalidData);
    assert_eq!(
        fs::read(&candidate_path).expect("replacement entry must remain untouched"),
        b"other user's entry"
    );
}

#[test]
fn conditional_windows_combined_receipt_error_keeps_native_error_source() {
    use std::error::Error;

    let native = io::Error::from_raw_os_error(1177);
    let expected_kind = native.kind();
    let combined = combine_observation_error(
        Some(native),
        Some(io::Error::new(io::ErrorKind::PermissionDenied, "receipt lookup denied")),
    )
    .expect("combined errors should be returned");
    assert_eq!(combined.kind(), expected_kind);
    let source = combined
        .get_ref()
        .and_then(|source| source.downcast_ref::<super::NativeReceiptError>())
        .expect("native error wrapper should remain available as source");
    assert_eq!(source.native.raw_os_error(), Some(1177));
    assert_eq!(source.observation.kind(), io::ErrorKind::PermissionDenied);
    assert!(Error::source(source).is_some());
}

#[test]
fn conditional_windows_receipt_errors_are_indeterminate() {
    assert_eq!(
        receipt_publication(None, true, false, true),
        LocalConditionalPublicationState::Indeterminate,
        "a failed observation must not be treated as an absent name"
    );
}

#[test]
fn conditional_windows_optional_entry_observation_propagates_non_not_found() {
    let error = super::observe_optional_entry::<()>(|| {
        Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "injected native observation failure",
        ))
    })
    .expect_err("only NotFound may become an absent receipt");
    assert_eq!(error.kind(), io::ErrorKind::PermissionDenied);
}

#[test]
fn conditional_windows_private_entry_guard_removes_only_original_identity() {
    let directory = tempfile::tempdir().expect("fixture directory should be created");
    let target = directory.path().join("target");
    let namespace =
        Namespace::open_host(&target, &options(LocalWriteMode::CreateOrReplace)).expect("Host namespace should open");

    let (owned_name, owned_file) = namespace
        .create_private("guard-owned")
        .expect("private entry should be created");
    let owned_path = namespace
        .private_file_path(&owned_name)
        .expect("private path should resolve");
    drop(PrivateEntryGuard::new(&namespace, &owned_file).expect("guard should retain its original handle"));
    drop(owned_file);
    assert!(
        !owned_path.exists(),
        "guard should remove the entry matching its original handle identity"
    );

    let (replaced_name, replaced_file) = namespace
        .create_private("guard-replaced")
        .expect("private entry should be created");
    let replaced_path = namespace
        .private_file_path(&replaced_name)
        .expect("private path should resolve");
    fs::remove_file(&replaced_path).expect("original entry should be removable");
    fs::write(&replaced_path, b"foreign replacement").expect("foreign replacement should be created");
    drop(PrivateEntryGuard::new(&namespace, &replaced_file).expect("guard should retain its original handle"));
    assert_eq!(
        fs::read(&replaced_path).expect("foreign replacement should remain"),
        b"foreign replacement"
    );
}

#[test]
fn conditional_windows_candidate_copy_keeps_original_handle_identity() {
    let directory = tempfile::tempdir().expect("fixture directory should be created");
    let target = directory.path().join("target");
    let mut backend = WindowsConditionalBackend::open_host(&target, &options(LocalWriteMode::CreateOrReplace))
        .expect("Host candidate should open");
    put(&mut backend, b"complete B copy");
    let copy = backend
        .create_candidate_copy()
        .expect("complete candidate copy should be returned");
    let copy_path = copy.namespace_path().to_path_buf();
    fs::remove_file(&copy_path).expect("copy name should be replaceable in test");
    fs::write(&copy_path, b"foreign replacement").expect("foreign replacement should be created");

    assert!(
        copy.open_reader().is_err(),
        "reader should reject a replaced namespace entry"
    );
    assert_eq!(
        fs::read(copy_path).expect("foreign replacement should remain"),
        b"foreign replacement"
    );
    assert!(
        backend.verify_entry(&copy).is_err(),
        "replaced copy name must reject publication"
    );
}

fn dacl_bytes(path: &Path) -> Vec<u8> {
    use windows_sys::Win32::Security::DACL_SECURITY_INFORMATION;
    use windows_sys::Win32::Security::GetFileSecurityW;

    let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    let mut required = 0_u32;
    // SAFETY: the path is NUL-terminated and the zero-sized query requests the
    // required descriptor length without writing through the null buffer.
    let first = unsafe {
        GetFileSecurityW(
            wide.as_ptr(),
            DACL_SECURITY_INFORMATION,
            std::ptr::null_mut(),
            0,
            &mut required,
        )
    };
    assert_eq!(first, 0, "descriptor size query should request a buffer");
    assert!(required > 0, "descriptor size should be nonzero");
    let mut descriptor = vec![0_u8; required as usize];
    // SAFETY: descriptor has the exact capacity reported by the first call.
    let success = unsafe {
        GetFileSecurityW(
            wide.as_ptr(),
            DACL_SECURITY_INFORMATION,
            descriptor.as_mut_ptr().cast(),
            required,
            &mut required,
        )
    };
    assert_ne!(success, 0, "security descriptor should be readable");
    descriptor.truncate(required as usize);
    let dacl_offset = u32::from_le_bytes(descriptor[16..20].try_into().expect("DACL offset is present")) as usize;
    assert_ne!(dacl_offset, 0, "DACL should be present");
    let dacl_size = u16::from_le_bytes(
        descriptor[dacl_offset + 2..dacl_offset + 4]
            .try_into()
            .expect("ACL size is present"),
    ) as usize;
    descriptor[dacl_offset..dacl_offset + dacl_size].to_vec()
}

#[test]
fn conditional_windows_host_content_replacement_retains_then_cleans_backup() {
    let directory = tempfile::tempdir().expect("fixture directory should be created");
    let target = directory.path().join("target");
    fs::write(&target, b"A").expect("old target should be written");
    let backend = WindowsConditionalBackend::open_host(&target, &options(LocalWriteMode::CreateOrReplace))
        .expect("Host conditional candidate should open");
    let mut backend = backend;
    put(&mut backend, b"B");
    let result = commit_conditional::<Sha256, _>(
        backend,
        LocalWritePrecondition::Content(digest(b"A")),
        options(LocalWriteMode::CreateOrReplace),
        1,
    )
    .expect("matching Content should publish");

    assert_eq!(fs::read(&target).expect("installed content should be readable"), b"B");
    assert_eq!(result.digest(), &digest(b"B"));
    assert!(
        result.retained_files().is_empty(),
        "successful cleanup should remove the displaced backup"
    );
    assert!(
        result.cleanup_errors().is_empty(),
        "successful cleanup should not report errors"
    );
}

#[test]
fn conditional_windows_host_content_preserves_destination_acl() {
    let directory = tempfile::tempdir().expect("fixture directory should be created");
    let target = directory.path().join("target");
    fs::write(&target, b"A").expect("old target should be written");
    let dacl_before = dacl_bytes(&target);
    let mut backend = WindowsConditionalBackend::open_host(&target, &options(LocalWriteMode::CreateOrReplace))
        .expect("Host candidate should open");
    put(&mut backend, b"B");

    commit_conditional::<Sha256, _>(
        backend,
        LocalWritePrecondition::Content(digest(b"A")),
        options(LocalWriteMode::CreateOrReplace),
        1,
    )
    .expect("matching content should publish");
    let dacl_after = dacl_bytes(&target);
    assert!(
        dacl_after[8..]
            .windows(dacl_before.len() - 8)
            .any(|entries| entries == &dacl_before[8..]),
        "ReplaceFileW should retain the target DACL while merging inherited ACEs",
    );
}

#[test]
fn conditional_windows_required_durability_fails_before_publication() {
    let directory = tempfile::tempdir().expect("fixture directory should be created");
    let target = directory.path().join("target");
    fs::write(&target, b"A").expect("old target should be written");
    let required =
        options(LocalWriteMode::CreateOrReplace).with_durability(crate::policy::LocalDurabilityRequirement::Required);
    let mut backend =
        WindowsConditionalBackend::open_host(&target, &required).expect("Host conditional candidate should open");
    put(&mut backend, b"B");

    let failure = commit_conditional::<Sha256, _>(backend, LocalWritePrecondition::Content(digest(b"A")), required, 1)
        .expect_err("unsupported Required directory durability must fail closed");
    assert_eq!(fs::read(&target).expect("old target should remain"), b"A");
    assert_eq!(failure.publication(), LocalConditionalPublicationState::NotPublished);
    assert!(failure.candidate().is_some(), "B copy remains recoverable");
}

#[test]
fn conditional_windows_absent_race_never_overwrites_late_target() {
    let directory = tempfile::tempdir().expect("fixture directory should be created");
    let target = directory.path().join("target");
    let mut backend = WindowsConditionalBackend::open_host(&target, &options(LocalWriteMode::CreateNew))
        .expect("Host conditional candidate should open");
    put(&mut backend, b"B");
    backend.set_hooks(ConditionalHooks::sequence(vec![(
        ConditionalPoint::Publish,
        ConditionalHookAction::ReplaceTarget(b"C".to_vec()),
    )]));

    let failure = commit_conditional::<Sha256, _>(
        backend,
        LocalWritePrecondition::Absent,
        options(LocalWriteMode::CreateNew),
        1,
    )
    .expect_err("the racing target must reject no-replace installation");
    assert_eq!(fs::read(&target).expect("racing target should remain"), b"C");
    let candidate = failure.candidate().expect("complete B copy must remain recoverable");
    let mut reader = candidate.open_reader().expect("candidate copy identity should verify");
    let mut bytes = Vec::new();
    Streams::copy_input_to_output(&mut reader, &mut bytes).expect("candidate copy should read");
    assert_eq!(bytes, b"B");
}

#[test]
fn conditional_windows_publish_checkpoint_failure_keeps_copy_and_cleans_candidate() {
    let directory = tempfile::tempdir().expect("fixture directory should be created");
    let target = directory.path().join("target");
    fs::write(&target, b"A").expect("old target should be written");
    let mut backend = WindowsConditionalBackend::open_host(&target, &options(LocalWriteMode::CreateOrReplace))
        .expect("Host conditional candidate should open");
    put(&mut backend, b"B");
    backend.set_hooks(ConditionalHooks::fail_once(
        ConditionalPoint::Publish,
        io::ErrorKind::PermissionDenied,
    ));

    let failure = commit_conditional::<Sha256, _>(
        backend,
        LocalWritePrecondition::Content(digest(b"A")),
        options(LocalWriteMode::CreateOrReplace),
        1,
    )
    .expect_err("pre-publication checkpoint should fail");
    assert_eq!(fs::read(&target).expect("old target should remain"), b"A");
    assert_eq!(failure.stage(), crate::error::LocalConditionalWriteStage::Publish);
    assert!(failure.candidate().is_some(), "independent B copy remains recoverable");
    assert!(
        failure
            .recovery_files()
            .iter()
            .all(|file| file.role() != LocalRecoveryFileRole::StagedCandidate)
    );
    let names: Vec<_> = fs::read_dir(directory.path())
        .expect("fixture directory should be readable")
        .map(|entry| entry.expect("directory entry should be readable").file_name())
        .collect();
    assert_eq!(
        names
            .iter()
            .filter(|name| name.to_string_lossy().contains("candidate"))
            .count(),
        1
    );
}

#[test]
fn conditional_windows_rooted_content_is_explicitly_unsupported() {
    let directory = tempfile::tempdir().expect("fixture directory should be created");
    let target = directory.path().join("target");
    fs::write(&target, b"A").expect("old target should be written");
    let root = crate::local::internal::open_root_directory(directory.path()).expect("root should open");
    let mut backend = WindowsConditionalBackend::open_rooted(
        root,
        Path::new("target"),
        &target,
        Some(&target),
        &options(LocalWriteMode::CreateOrReplace),
    )
    .expect("Rooted candidate should open");
    put(&mut backend, b"B");

    let failure = commit_conditional::<Sha256, _>(
        backend,
        LocalWritePrecondition::Content(digest(b"A")),
        options(LocalWriteMode::CreateOrReplace),
        1,
    )
    .expect_err("Windows Rooted Content must fail closed");
    assert_eq!(fs::read(&target).expect("old target should remain"), b"A");
    assert_eq!(failure.publication(), LocalConditionalPublicationState::NotPublished);
    assert!(failure.candidate().is_some(), "complete B copy should remain available");
}

#[test]
fn conditional_windows_rooted_absent_uses_handle_relative_no_replace() {
    let directory = tempfile::tempdir().expect("fixture directory should be created");
    let target = directory.path().join("target");
    let root = crate::local::internal::open_root_directory(directory.path()).expect("root should open");
    let mut backend = WindowsConditionalBackend::open_rooted(
        root,
        Path::new("target"),
        &target,
        Some(&target),
        &options(LocalWriteMode::CreateNew),
    )
    .expect("Rooted candidate should open");
    put(&mut backend, b"B");
    commit_conditional::<Sha256, _>(
        backend,
        LocalWritePrecondition::Absent,
        options(LocalWriteMode::CreateNew),
        1,
    )
    .expect("Rooted absent target should install");
    assert_eq!(fs::read(target).expect("Rooted target should be readable"), b"B");
}

#[test]
fn conditional_windows_reparse_target_is_rejected() {
    use std::os::windows::fs::symlink_file;

    let directory = tempfile::tempdir().expect("fixture directory should be created");
    let actual = directory.path().join("actual");
    let target = directory.path().join("target");
    fs::write(&actual, b"A").expect("referent should be written");
    if symlink_file(&actual, &target).is_err() {
        eprintln!("skipped: creating a Windows symbolic link is not permitted");
        return;
    }
    let result = WindowsConditionalBackend::open_host(&target, &options(LocalWriteMode::CreateOrReplace));
    assert!(
        result.is_ok(),
        "opening a candidate does not follow or open the target entry"
    );
    let mut backend = result.expect("candidate should open");
    put(&mut backend, b"B");
    let failure = commit_conditional::<Sha256, _>(
        backend,
        LocalWritePrecondition::Content(digest(b"A")),
        options(LocalWriteMode::CreateOrReplace),
        1,
    )
    .expect_err("final reparse points must be rejected");
    assert_eq!(failure.publication(), LocalConditionalPublicationState::NotPublished);
    assert_eq!(fs::read(actual).expect("symlink referent must remain"), b"A");
}

#[test]
fn conditional_windows_sharing_conflict_preserves_candidate_and_target() {
    let directory = tempfile::tempdir().expect("fixture directory should be created");
    let target = directory.path().join("target");
    fs::write(&target, b"A").expect("old target should be written");
    let blocker = fs::OpenOptions::new()
        .read(true)
        .share_mode(
            windows_sys::Win32::Storage::FileSystem::FILE_SHARE_READ
                | windows_sys::Win32::Storage::FileSystem::FILE_SHARE_WRITE,
        )
        .open(&target)
        .expect("sharing blocker should open");
    let mut backend = WindowsConditionalBackend::open_host(&target, &options(LocalWriteMode::CreateOrReplace))
        .expect("Host candidate should open alongside shared reader");
    put(&mut backend, b"B");
    let failure = commit_conditional::<Sha256, _>(
        backend,
        LocalWritePrecondition::Content(digest(b"A")),
        options(LocalWriteMode::CreateOrReplace),
        1,
    )
    .expect_err("a handle denying delete sharing must block ReplaceFileW");
    drop(blocker);
    assert_eq!(fs::read(&target).expect("old target should remain"), b"A");
    assert_eq!(failure.publication(), LocalConditionalPublicationState::NotPublished);
    assert!(
        failure.candidate().is_some(),
        "candidate copy must survive sharing failure"
    );
    assert!(
        fs::read_dir(directory.path())
            .expect("fixture parent should remain readable")
            .filter_map(Result::ok)
            .all(|entry| {
                entry.file_type().is_ok_and(|kind| !kind.is_dir())
                    || !entry
                        .file_name()
                        .to_string_lossy()
                        .starts_with(".qubit-conditional-recovery-")
            }),
        "empty recovery container should be removed when ReplaceFileW makes no backup"
    );
}

#[test]
fn conditional_windows_read_only_target_conflict_preserves_candidate_and_target() {
    let directory = tempfile::tempdir().expect("fixture directory should be created");
    let target = directory.path().join("target");
    fs::write(&target, b"A").expect("old target should be written");
    let mut permissions = fs::metadata(&target)
        .expect("target metadata should be available")
        .permissions();
    permissions.set_readonly(true);
    fs::set_permissions(&target, permissions).expect("target should become read-only");
    let mut backend = WindowsConditionalBackend::open_host(&target, &options(LocalWriteMode::CreateOrReplace))
        .expect("Host candidate should open");
    put(&mut backend, b"B");

    let failure = commit_conditional::<Sha256, _>(
        backend,
        LocalWritePrecondition::Content(digest(b"A")),
        options(LocalWriteMode::CreateOrReplace),
        1,
    )
    .expect_err("ReplaceFileW must report a read-only destination conflict");
    assert_eq!(fs::read(&target).expect("old target should remain"), b"A");
    assert!(failure.candidate().is_some(), "B copy remains recoverable");

    let mut permissions = fs::metadata(&target)
        .expect("target metadata should remain available")
        .permissions();
    permissions.set_readonly(false);
    fs::set_permissions(&target, permissions).expect("fixture target should become writable");
    drop(failure);
}

#[test]
fn conditional_windows_replacefile_partial_codes_are_classified() {
    assert_eq!(
        classify_replace_failure(Some(1175)),
        ReplaceFailure::UnableToRemoveReplaced
    );
    assert_eq!(
        classify_replace_failure(Some(1176)),
        ReplaceFailure::UnableToMoveReplacement
    );
    assert_eq!(
        classify_replace_failure(Some(1177)),
        ReplaceFailure::UnableToMoveReplacementAfterBackup
    );
    assert_eq!(classify_replace_failure(Some(5)), ReplaceFailure::Other);
}

#[test]
fn conditional_windows_partial_1177_receipt_reports_installed_and_displaced_entries() {
    let directory = tempfile::tempdir().expect("fixture directory should be created");
    let target = directory.path().join("target");
    fs::write(&target, b"A").expect("old target should be written");
    let mut backend = WindowsConditionalBackend::open_host(&target, &options(LocalWriteMode::CreateOrReplace))
        .expect("Host candidate should open");
    put(&mut backend, b"B");
    let (container, backup) = backend.make_backup().expect("recovery container should be exclusive");
    backend.begin_publication().expect("candidate access should be rebound");
    fs::rename(&target, &backup).expect("simulate ReplaceFileW moving the replaced entry");
    fs::rename(directory.path().join(&backend.candidate_name), &target)
        .expect("simulate replacement name becoming the destination");

    let attempt = backend.collect_native_receipt(
        Some(io::Error::from_raw_os_error(1177)),
        Some(&container),
        Some(&backup),
    );
    assert_eq!(attempt.publication, LocalConditionalPublicationState::Published);
    assert!(
        attempt.installed.is_some(),
        "final destination must be reopened after native error"
    );
    assert_eq!(attempt.displaced.len(), 1, "actual backup must be reported");
    assert_eq!(attempt.displaced[0].role(), LocalRecoveryFileRole::Displaced);
    let mut backup_reader = attempt.displaced[0]
        .open_reader()
        .expect("backup should retain the displaced file");
    let mut backup_bytes = Vec::new();
    Streams::copy_input_to_output(&mut backup_reader, &mut backup_bytes)
        .expect("displaced file should remain readable");
    assert_eq!(backup_bytes, b"A");
}

#[test]
fn conditional_windows_partial_1177_receipt_retains_every_visible_object() {
    let directory = tempfile::tempdir().expect("fixture directory should be created");
    let target = directory.path().join("target");
    fs::write(&target, b"A").expect("old target should be written");
    let mut backend = WindowsConditionalBackend::open_host(&target, &options(LocalWriteMode::CreateOrReplace))
        .expect("Host candidate should open");
    put(&mut backend, b"B");
    let (container, backup) = backend.make_backup().expect("recovery container should be exclusive");
    backend.begin_publication().expect("candidate access should be rebound");

    // Model the documented partial state where the old destination reached
    // the backup but the replacement remains under its candidate name.
    fs::rename(&target, &backup).expect("simulate moving the replaced entry");
    let attempt = backend.collect_native_receipt(
        Some(io::Error::from_raw_os_error(1177)),
        Some(&container),
        Some(&backup),
    );
    assert_eq!(attempt.publication, LocalConditionalPublicationState::Indeterminate);
    assert!(attempt.installed.is_none(), "target name is currently absent");
    assert_eq!(attempt.displaced.len(), 2, "backup and candidate are retained");
    let roles: Vec<_> = attempt.displaced.iter().map(|entry| entry.role()).collect();
    assert!(roles.contains(&LocalRecoveryFileRole::Displaced));
    assert!(roles.contains(&LocalRecoveryFileRole::StagedCandidate));
    let mut candidate = attempt
        .displaced
        .iter()
        .find(|entry| entry.role() == LocalRecoveryFileRole::StagedCandidate)
        .expect("candidate path must be reported")
        .open_reader()
        .expect("candidate should remain readable");
    let mut bytes = Vec::new();
    Streams::copy_input_to_output(&mut candidate, &mut bytes).expect("candidate should read");
    assert_eq!(bytes, b"B");
}

#[test]
fn conditional_windows_recovery_container_collision_does_not_reuse_existing_directory() {
    let directory = tempfile::tempdir().expect("fixture directory should be created");
    let target = directory.path().join("target");
    let namespace =
        Namespace::open_host(&target, &options(LocalWriteMode::CreateOrReplace)).expect("Host namespace should open");
    let collision = directory.path().join(".conditional-recovery-collision");
    fs::create_dir(&collision).expect("collision directory should be created");
    let marker = collision.join("user-data");
    fs::write(&marker, b"keep").expect("collision marker should be written");
    let result = namespace.backup_container_named(Path::new(".conditional-recovery-collision"));
    assert_eq!(
        result
            .expect_err("existing recovery directory must not be reused")
            .kind(),
        io::ErrorKind::AlreadyExists
    );
    assert_eq!(fs::read(marker).expect("existing container data must remain"), b"keep");
}

#[test]
fn conditional_windows_failed_container_verification_cleans_bound_directory() {
    let directory = tempfile::tempdir().expect("fixture directory should be created");
    let target = directory.path().join("target");
    let namespace =
        Namespace::open_host(&target, &options(LocalWriteMode::CreateOrReplace)).expect("Host namespace should open");
    namespace.fail_next_container_verification();

    let error = namespace
        .backup_container()
        .expect_err("injected verification failure should fail creation");

    assert!(
        error
            .to_string()
            .contains("injected recovery container verification failure")
    );
    let leftovers: Vec<_> = fs::read_dir(directory.path())
        .expect("fixture directory should be readable")
        .map(|entry| entry.expect("directory entry should be readable").path())
        .filter(|path| {
            path.file_name()
                .is_some_and(|name| name.to_string_lossy().starts_with(".qubit-conditional-recovery-"))
        })
        .collect();
    assert!(
        leftovers.is_empty(),
        "bound failed container should be cleaned: {leftovers:?}"
    );
}

#[test]
fn conditional_windows_recovery_container_name_cannot_be_replaced_while_owned() {
    let directory = tempfile::tempdir().expect("fixture directory should be created");
    let target = directory.path().join("target");
    let namespace =
        Namespace::open_host(&target, &options(LocalWriteMode::CreateOrReplace)).expect("Host namespace should open");
    let (container, _) = namespace
        .backup_container()
        .expect("recovery container should be created");
    let moved = directory.path().join("replacement-container");
    let error = fs::rename(&container, &moved).expect_err("owned directory name must reject replacement");
    assert_eq!(
        error.raw_os_error(),
        Some(ERROR_SHARING_VIOLATION as i32),
        "directory name should be locked by the recovery handle"
    );

    namespace
        .remove_container(&container)
        .expect("owned directory should be removable by its handle");
    assert!(!container.exists(), "owned directory name should be removed");
    assert!(!moved.exists(), "replacement path should remain absent");
}

#[test]
fn conditional_windows_recovery_container_cleanup_refuses_non_empty_directory() {
    let directory = tempfile::tempdir().expect("fixture directory should be created");
    let target = directory.path().join("target");
    let namespace =
        Namespace::open_host(&target, &options(LocalWriteMode::CreateOrReplace)).expect("Host namespace should open");
    let (container, _) = namespace
        .backup_container()
        .expect("recovery container should be created");
    let marker = container.join("user-data");
    fs::write(&marker, b"keep").expect("container data should be written");

    let error = namespace
        .remove_container(&container)
        .expect_err("non-empty recovery directory must not be removed");

    assert_ne!(error.kind(), io::ErrorKind::NotFound);
    assert_eq!(fs::read(marker).expect("container data should remain"), b"keep");
}

#[test]
fn conditional_windows_failed_empty_container_cleanup_reports_its_path() {
    let directory = tempfile::tempdir().expect("fixture directory should be created");
    let target = directory.path().join("target");
    fs::write(&target, b"A").expect("target should be written");
    let mut backend = WindowsConditionalBackend::open_host(&target, &options(LocalWriteMode::CreateOrReplace))
        .expect("Host candidate should open");
    put(&mut backend, b"B");
    let (container, backup) = backend.make_backup().expect("recovery container should be created");
    let marker = container.join("unexpected");
    fs::write(&marker, b"keep").expect("unexpected content should be written");
    let attempt = backend.candidate_receipt(io::Error::other("injected ReplaceFileW failure"));

    let attempt = backend.cleanup_empty_container(container.clone(), backup, attempt);

    let error = attempt.error.expect("cleanup failure should be reported");
    assert!(error.to_string().contains(&container.display().to_string()));
    assert_eq!(fs::read(marker).expect("unexpected content should remain"), b"keep");
}
