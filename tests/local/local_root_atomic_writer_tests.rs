// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

#[cfg(target_os = "linux")]
use std::env;
#[cfg(unix)]
use std::io::{
    IoSlice,
    Write,
};
#[cfg(target_os = "linux")]
use std::path::PathBuf;
#[cfg(target_os = "linux")]
use std::process::Command;
#[cfg(unix)]
use std::time::Duration;

#[cfg(unix)]
use qubit_local_files::{
    LocalAtomicDestinationState,
    LocalAtomicWriteOptions,
    LocalAtomicWriteStage,
    LocalRelativePath,
    LocalRoot,
};

#[cfg(target_os = "linux")]
use super::test_support::SourceReadLease;
#[cfg(all(coverage, target_os = "linux"))]
use super::test_support::run_in_coverage_fault_process;
#[cfg(unix)]
use super::test_support::{
    count_atomic_temp_files,
    fs,
    temp_dir,
};
#[cfg(any(
    target_os = "linux",
    target_os = "android",
    target_os = "macos",
    target_os = "freebsd",
))]
use super::test_support::{
    get_user_xattr,
    set_user_xattr,
};

#[cfg(target_os = "linux")]
const ROOTED_ATOMIC_SYNC_CHILD_ENV: &str =
    "QUBIT_LOCAL_FILES_ROOTED_ATOMIC_SYNC_CHILD";
#[cfg(target_os = "linux")]
const ROOTED_ATOMIC_SYNC_ROOT_ENV: &str =
    "QUBIT_LOCAL_FILES_ROOTED_ATOMIC_SYNC_ROOT";

#[cfg(unix)]
#[test]
fn test_begin_atomic_write_with_options_rejects_missing_parent() {
    let root_path = temp_dir("rooted-atomic-parent-disabled");
    let root = LocalRoot::open(&root_path).expect("root should open");
    let destination = LocalRelativePath::new("nested/result.txt")
        .expect("destination should validate");

    let error = root
        .begin_atomic_write_with_options(
            &destination,
            LocalAtomicWriteOptions::new(),
        )
        .expect_err("missing parent should be rejected");

    assert_eq!(LocalAtomicWriteStage::PrepareParent, error.stage());
    assert!(!root_path.join("nested").exists());
    fs::remove_dir_all(root_path).unwrap();
}

#[cfg(target_os = "linux")]
#[test]
fn test_local_root_atomic_writer_zero_open_retry_timeout_reports_timed_out() {
    let root_path = temp_dir("rooted-atomic-zero-open-retry-timeout");
    let destination_path = root_path.join("result.txt");
    fs::write(&destination_path, b"original")
        .expect("destination should be written");
    let lease = SourceReadLease::acquire(&destination_path)
        .expect("destination read lease should be acquired");
    let root = LocalRoot::open(&root_path).expect("root should open");
    let destination = LocalRelativePath::new("result.txt")
        .expect("destination should validate");
    let options = LocalAtomicWriteOptions::new()
        .with_parent()
        .with_open_retry_timeout(Duration::ZERO);
    let mut writer = root
        .begin_atomic_write_with_options(&destination, options)
        .expect("rooted atomic writer should begin");
    writer
        .write_all(b"replacement")
        .expect("replacement should be staged");
    let (sender, receiver) = std::sync::mpsc::sync_channel(1);
    let worker = std::thread::spawn(move || {
        sender.send(writer.commit()).expect("result should be sent");
    });

    lease
        .wait_for_break()
        .expect("commit should reach the destination open");
    let first_result = receiver.recv_timeout(Duration::from_millis(250));
    lease
        .release()
        .expect("destination lease should be released");
    worker.join().expect("commit worker should not panic");
    let result = first_result.unwrap_or_else(|_| {
        receiver
            .recv_timeout(Duration::from_secs(1))
            .expect("commit result should arrive after lease release")
    });
    let error =
        result.expect_err("zero timeout should reject the lease conflict");

    assert_eq!(
        LocalAtomicWriteStage::ReadDestinationMetadata,
        error.stage()
    );
    assert_eq!(std::io::ErrorKind::TimedOut, error.kind());
    assert_eq!(
        LocalAtomicDestinationState::Unchanged,
        error.destination_state()
    );
    assert_eq!(b"original", fs::read(&destination_path).unwrap().as_slice());
    assert_eq!(0, count_atomic_temp_files(&root_path));
    fs::remove_dir_all(root_path).unwrap();
}

/// Asserts one injected rooted commit failure through the public API.
#[cfg(all(coverage, target_os = "linux"))]
fn assert_injected_rooted_commit_error(
    test_name: &str,
    fault: &str,
    expected_stage: LocalAtomicWriteStage,
    expected_state: LocalAtomicDestinationState,
    expected_temporary_files: usize,
) {
    let Some(()) = run_in_coverage_fault_process(test_name, fault, move || {
        let root_path = temp_dir(fault);
        fs::write(root_path.join("result.txt"), b"old")
            .expect("destination fixture should be written");
        let root = LocalRoot::open(&root_path).expect("root should open");
        let destination = LocalRelativePath::new("result.txt")
            .expect("destination should validate");
        let mut writer = root
            .begin_atomic_write(&destination)
            .expect("rooted atomic writer should begin");
        writer
            .write_all(b"new")
            .expect("replacement should be staged");

        let error = writer
            .commit()
            .expect_err("injected rooted commit should fail");

        assert_eq!(expected_stage, error.stage());
        assert_eq!(expected_state, error.destination_state());
        assert_eq!(
            expected_temporary_files,
            count_atomic_temp_files(&root_path)
        );
        fs::remove_dir_all(root_path)
            .expect("test directory should be removed");
    }) else {
        return;
    };
}

/// Asserts one injected rooted staging-creation failure.
#[cfg(all(coverage, target_os = "linux"))]
fn assert_injected_rooted_begin_error(test_name: &str, fault: &str) {
    let Some(()) = run_in_coverage_fault_process(test_name, fault, move || {
        let root_path = temp_dir(fault);
        let root = LocalRoot::open(&root_path).expect("root should open");
        let destination = LocalRelativePath::new("result.txt")
            .expect("destination should validate");

        let error = root
            .begin_atomic_write(&destination)
            .expect_err("injected rooted writer creation should fail");

        assert_eq!(LocalAtomicWriteStage::CreateTemporaryFile, error.stage());
        assert_eq!(0, count_atomic_temp_files(&root_path));
        fs::remove_dir_all(root_path)
            .expect("test directory should be removed");
    }) else {
        return;
    };
}

/// Verifies rejection of an injected rooted destination identity mismatch.
#[cfg(all(coverage, target_os = "linux"))]
#[test]
fn test_commit_rejects_injected_rooted_identity_mismatch() {
    const TEST_NAME: &str = concat!(
        "local::local_root_atomic_writer_tests::",
        "test_commit_rejects_injected_rooted_identity_mismatch",
    );
    assert_injected_rooted_commit_error(
        TEST_NAME,
        "rooted-identity-mismatch",
        LocalAtomicWriteStage::ReplaceDestination,
        LocalAtomicDestinationState::Unchanged,
        0,
    );
}

/// Verifies precise missing state after an injected rooted identity mismatch.
#[cfg(all(coverage, target_os = "linux"))]
#[test]
fn test_commit_reports_injected_missing_rooted_identity() {
    const TEST_NAME: &str = concat!(
        "local::local_root_atomic_writer_tests::",
        "test_commit_reports_injected_missing_rooted_identity",
    );
    assert_injected_rooted_commit_error(
        TEST_NAME,
        "rooted-identity-missing",
        LocalAtomicWriteStage::ReplaceDestination,
        LocalAtomicDestinationState::Missing,
        1,
    );
}

/// Verifies propagation of injected rooted identity inspection failures.
#[cfg(all(coverage, target_os = "linux"))]
#[test]
fn test_commit_reports_injected_rooted_identity_inspection_error() {
    const TEST_NAME: &str = concat!(
        "local::local_root_atomic_writer_tests::",
        "test_commit_reports_injected_rooted_identity_inspection_error",
    );
    assert_injected_rooted_commit_error(
        TEST_NAME,
        "rooted-identity-inspect",
        LocalAtomicWriteStage::ReplaceDestination,
        LocalAtomicDestinationState::Unchanged,
        0,
    );
}

/// Verifies propagation of injected rooted replacement failures.
#[cfg(all(coverage, target_os = "linux"))]
#[test]
fn test_commit_reports_injected_rooted_install_error() {
    const TEST_NAME: &str = concat!(
        "local::local_root_atomic_writer_tests::",
        "test_commit_reports_injected_rooted_install_error",
    );
    assert_injected_rooted_commit_error(
        TEST_NAME,
        "rooted-install",
        LocalAtomicWriteStage::ReplaceDestination,
        LocalAtomicDestinationState::Unchanged,
        0,
    );
}

/// Verifies that an indeterminate rooted replacement outcome preserves
/// staging for caller-directed recovery.
#[cfg(all(coverage, target_os = "linux"))]
#[test]
fn test_commit_preserves_staging_after_indeterminate_rooted_install_error() {
    const TEST_NAME: &str = concat!(
        "local::local_root_atomic_writer_tests::",
        "test_commit_preserves_staging_after_indeterminate_rooted_install_error",
    );
    assert_injected_rooted_commit_error(
        TEST_NAME,
        "rooted-install-indeterminate",
        LocalAtomicWriteStage::ReplaceDestination,
        LocalAtomicDestinationState::Indeterminate,
        1,
    );
}

/// Verifies that one rooted no-replace staging unlink failure is retried.
#[cfg(all(coverage, target_os = "linux"))]
#[test]
fn test_commit_retries_injected_rooted_install_unlink_error() {
    const TEST_NAME: &str = concat!(
        "local::local_root_atomic_writer_tests::",
        "test_commit_retries_injected_rooted_install_unlink_error",
    );
    let Some(()) = run_in_coverage_fault_process(
        TEST_NAME,
        "atomic-install-unlink",
        move || {
            let root_path = temp_dir("rooted-atomic-install-unlink");
            let root = LocalRoot::open(&root_path).expect("root should open");
            let destination = LocalRelativePath::new("result.txt")
                .expect("destination should validate");
            let mut writer = root
                .begin_atomic_write(&destination)
                .expect("rooted atomic writer should begin");
            writer.write_all(b"new").expect("content should be staged");

            writer
                .commit()
                .expect("a one-shot staging unlink failure should recover");

            assert_eq!(
                b"new",
                fs::read(root_path.join("result.txt")).unwrap().as_slice(),
            );
            assert_eq!(0, count_atomic_temp_files(&root_path));
            fs::remove_dir_all(root_path)
                .expect("test directory should be removed");
        },
    ) else {
        return;
    };
}

/// Verifies that persistent rooted no-replace unlink failures preserve
/// staging and cleanup context.
#[cfg(all(coverage, target_os = "linux"))]
#[test]
fn test_commit_reports_persistent_rooted_install_unlink_error() {
    const TEST_NAME: &str = concat!(
        "local::local_root_atomic_writer_tests::",
        "test_commit_reports_persistent_rooted_install_unlink_error",
    );
    let Some(()) = run_in_coverage_fault_process(
        TEST_NAME,
        "atomic-install-unlink-persistent",
        move || {
            let root_path = temp_dir("rooted-atomic-install-unlink-persistent");
            let root = LocalRoot::open(&root_path).expect("root should open");
            let destination = LocalRelativePath::new("result.txt")
                .expect("destination should validate");
            let mut writer = root
                .begin_atomic_write(&destination)
                .expect("rooted atomic writer should begin");
            writer.write_all(b"new").expect("content should be staged");

            let error = writer
                .commit()
                .expect_err("persistent staging unlink should fail");

            assert_eq!(
                LocalAtomicWriteStage::ReplaceDestination,
                error.stage()
            );
            assert_eq!(
                LocalAtomicDestinationState::Replaced,
                error.destination_state(),
            );
            assert_eq!(
                Some(libc::EIO),
                error.cleanup_error().and_then(std::io::Error::raw_os_error),
            );
            assert_eq!(
                b"new",
                fs::read(root_path.join("result.txt")).unwrap().as_slice(),
            );
            assert_eq!(1, count_atomic_temp_files(&root_path));
            fs::remove_dir_all(root_path)
                .expect("test directory should be removed");
        },
    ) else {
        return;
    };
}

/// Verifies rooted parent synchronization after guard-level staging recovery.
#[cfg(all(coverage, target_os = "linux"))]
#[test]
fn test_commit_syncs_parent_after_rooted_install_unlink_recovery() {
    const TEST_NAME: &str = concat!(
        "local::local_root_atomic_writer_tests::",
        "test_commit_syncs_parent_after_rooted_install_unlink_recovery",
    );
    let Some(()) = run_in_coverage_fault_process(
        TEST_NAME,
        "atomic-install-unlink-recover-sync",
        move || {
            let root_path =
                temp_dir("rooted-atomic-install-unlink-recover-sync");
            let root = LocalRoot::open(&root_path).expect("root should open");
            let destination = LocalRelativePath::new("result.txt")
                .expect("destination should validate");
            let mut writer = root
                .begin_atomic_write(&destination)
                .expect("rooted atomic writer should begin");
            writer.write_all(b"new").expect("content should be staged");

            let error = writer
                .commit()
                .expect_err("injected rooted parent sync should fail");

            assert_eq!(LocalAtomicWriteStage::SyncParent, error.stage());
            assert_eq!(
                LocalAtomicDestinationState::Replaced,
                error.destination_state(),
            );
            assert_eq!(
                Some(libc::EIO),
                std::error::Error::source(&error).and_then(|source| {
                    source
                        .downcast_ref::<std::io::Error>()
                        .and_then(std::io::Error::raw_os_error)
                }),
            );
            assert!(error.cleanup_error().is_none());
            assert_eq!(
                b"new",
                fs::read(root_path.join("result.txt")).unwrap().as_slice(),
            );
            assert_eq!(0, count_atomic_temp_files(&root_path));
            fs::remove_dir_all(root_path)
                .expect("test directory should be removed");
        },
    ) else {
        return;
    };
}

/// Verifies that rooted install, cleanup, and parent-sync failures retain
/// distinct error context.
#[cfg(all(coverage, target_os = "linux"))]
#[test]
fn test_commit_retains_sync_error_after_persistent_rooted_install_unlink() {
    const TEST_NAME: &str = concat!(
        "local::local_root_atomic_writer_tests::",
        "test_commit_retains_sync_error_after_persistent_rooted_install_unlink",
    );
    let Some(()) = run_in_coverage_fault_process(
        TEST_NAME,
        "atomic-install-unlink-persistent-sync",
        move || {
            let root_path =
                temp_dir("rooted-atomic-install-unlink-persistent-sync");
            let root = LocalRoot::open(&root_path).expect("root should open");
            let destination = LocalRelativePath::new("result.txt")
                .expect("destination should validate");
            let mut writer = root
                .begin_atomic_write(&destination)
                .expect("rooted atomic writer should begin");
            writer.write_all(b"new").expect("content should be staged");

            let error = writer
                .commit()
                .expect_err("persistent rooted staging unlink should fail");

            assert_eq!(
                LocalAtomicWriteStage::ReplaceDestination,
                error.stage()
            );
            assert_eq!(
                LocalAtomicDestinationState::Replaced,
                error.destination_state(),
            );
            assert_eq!(
                Some(libc::EIO),
                error.cleanup_error().and_then(std::io::Error::raw_os_error),
            );
            assert_eq!(
                Some(libc::EIO),
                error
                    .parent_sync_error()
                    .and_then(std::io::Error::raw_os_error),
            );
            assert_eq!(
                b"new",
                fs::read(root_path.join("result.txt")).unwrap().as_slice(),
            );
            assert_eq!(1, count_atomic_temp_files(&root_path));
            fs::remove_dir_all(root_path)
                .expect("test directory should be removed");
        },
    ) else {
        return;
    };
}

/// Verifies propagation of injected rooted destination-open failures.
#[cfg(all(coverage, target_os = "linux"))]
#[test]
fn test_commit_reports_injected_rooted_destination_open_error() {
    const TEST_NAME: &str = concat!(
        "local::local_root_atomic_writer_tests::",
        "test_commit_reports_injected_rooted_destination_open_error",
    );
    assert_injected_rooted_commit_error(
        TEST_NAME,
        "rooted-destination-open",
        LocalAtomicWriteStage::ReadDestinationMetadata,
        LocalAtomicDestinationState::Unchanged,
        0,
    );
}

/// Verifies normalization of an injected missing rooted destination.
#[cfg(all(coverage, target_os = "linux"))]
#[test]
fn test_commit_reports_injected_missing_rooted_destination() {
    const TEST_NAME: &str = concat!(
        "local::local_root_atomic_writer_tests::",
        "test_commit_reports_injected_missing_rooted_destination",
    );
    assert_injected_rooted_commit_error(
        TEST_NAME,
        "rooted-destination-missing",
        LocalAtomicWriteStage::ReadDestinationMetadata,
        LocalAtomicDestinationState::Missing,
        1,
    );
}

/// Verifies propagation of injected descriptor-status read failures.
#[cfg(all(coverage, target_os = "linux"))]
#[test]
fn test_commit_reports_injected_nonblocking_status_read_error() {
    const TEST_NAME: &str = concat!(
        "local::local_root_atomic_writer_tests::",
        "test_commit_reports_injected_nonblocking_status_read_error",
    );
    assert_injected_rooted_commit_error(
        TEST_NAME,
        "unix-clear-nonblocking-get",
        LocalAtomicWriteStage::ReadDestinationMetadata,
        LocalAtomicDestinationState::Unchanged,
        0,
    );
}

/// Verifies propagation of injected descriptor-status update failures.
#[cfg(all(coverage, target_os = "linux"))]
#[test]
fn test_commit_reports_injected_nonblocking_status_update_error() {
    const TEST_NAME: &str = concat!(
        "local::local_root_atomic_writer_tests::",
        "test_commit_reports_injected_nonblocking_status_update_error",
    );
    assert_injected_rooted_commit_error(
        TEST_NAME,
        "unix-clear-nonblocking-set",
        LocalAtomicWriteStage::ReadDestinationMetadata,
        LocalAtomicDestinationState::Unchanged,
        0,
    );
}

/// Asserts one injected rooted destination-open normalization failure.
#[cfg(all(coverage, target_os = "linux"))]
fn assert_injected_rooted_destination_open_error(test_name: &str, fault: &str) {
    assert_injected_rooted_commit_error(
        test_name,
        fault,
        LocalAtomicWriteStage::ReadDestinationMetadata,
        LocalAtomicDestinationState::Unchanged,
        0,
    );
}

/// Verifies normalization of an injected invalid rooted destination.
#[cfg(all(coverage, target_os = "linux"))]
#[test]
fn test_commit_reports_injected_invalid_rooted_destination() {
    const TEST_NAME: &str = concat!(
        "local::local_root_atomic_writer_tests::",
        "test_commit_reports_injected_invalid_rooted_destination",
    );
    assert_injected_rooted_destination_open_error(
        TEST_NAME,
        "rooted-destination-invalid",
    );
}

/// Verifies propagation of an injected native rooted destination-open error.
#[cfg(all(coverage, target_os = "linux"))]
#[test]
fn test_commit_reports_injected_native_rooted_destination_open_error() {
    const TEST_NAME: &str = concat!(
        "local::local_root_atomic_writer_tests::",
        "test_commit_reports_injected_native_rooted_destination_open_error",
    );
    assert_injected_rooted_destination_open_error(
        TEST_NAME,
        "rooted-destination-native",
    );
}

/// Verifies retry of an injected rooted destination-open conflict.
#[cfg(all(coverage, target_os = "linux"))]
#[test]
fn test_commit_retries_injected_rooted_destination_open_conflict() {
    const TEST_NAME: &str = concat!(
        "local::local_root_atomic_writer_tests::",
        "test_commit_retries_injected_rooted_destination_open_conflict",
    );
    let Some(()) = run_in_coverage_fault_process(
        TEST_NAME,
        "rooted-destination-would-block",
        || {
            let root_path = temp_dir("rooted-destination-would-block");
            fs::write(root_path.join("result.txt"), b"old")
                .expect("destination fixture should be written");
            let root = LocalRoot::open(&root_path).expect("root should open");
            let destination = LocalRelativePath::new("result.txt")
                .expect("destination should validate");
            let mut writer = root
                .begin_atomic_write(&destination)
                .expect("rooted writer should begin");
            writer
                .write_all(b"new")
                .expect("replacement should be staged");
            writer
                .commit()
                .expect("transient conflict should be retried");
            fs::remove_dir_all(root_path)
                .expect("test directory should be removed");
        },
    ) else {
        return;
    };
}

/// Asserts one injected rooted identity-status failure.
#[cfg(all(coverage, target_os = "linux"))]
fn assert_injected_rooted_identity_status_error(test_name: &str, fault: &str) {
    assert_injected_rooted_commit_error(
        test_name,
        fault,
        LocalAtomicWriteStage::ReplaceDestination,
        LocalAtomicDestinationState::Unchanged,
        0,
    );
}

/// Verifies normalization of an injected missing rooted identity status.
#[cfg(all(coverage, target_os = "linux"))]
#[test]
fn test_commit_handles_injected_missing_rooted_identity_status() {
    const TEST_NAME: &str = concat!(
        "local::local_root_atomic_writer_tests::",
        "test_commit_handles_injected_missing_rooted_identity_status",
    );
    assert_injected_rooted_identity_status_error(
        TEST_NAME,
        "rooted-status-missing",
    );
}

/// Verifies rejection of an injected non-file rooted identity status.
#[cfg(all(coverage, target_os = "linux"))]
#[test]
fn test_commit_rejects_injected_rooted_identity_status_type() {
    const TEST_NAME: &str = concat!(
        "local::local_root_atomic_writer_tests::",
        "test_commit_rejects_injected_rooted_identity_status_type",
    );
    assert_injected_rooted_identity_status_error(
        TEST_NAME,
        "rooted-status-type",
    );
}

/// Verifies propagation of injected rooted identity-status errors.
#[cfg(all(coverage, target_os = "linux"))]
#[test]
fn test_commit_reports_injected_rooted_identity_status_error() {
    const TEST_NAME: &str = concat!(
        "local::local_root_atomic_writer_tests::",
        "test_commit_reports_injected_rooted_identity_status_error",
    );
    assert_injected_rooted_identity_status_error(
        TEST_NAME,
        "rooted-status-error",
    );
}

/// Verifies propagation of injected rooted identity conversion overflow.
#[cfg(all(coverage, target_os = "linux"))]
#[test]
fn test_commit_reports_injected_rooted_identity_overflow() {
    const TEST_NAME: &str = concat!(
        "local::local_root_atomic_writer_tests::",
        "test_commit_reports_injected_rooted_identity_overflow",
    );
    assert_injected_rooted_identity_status_error(
        TEST_NAME,
        "rooted-identity-overflow",
    );
}

/// Verifies propagation of rooted staging filename generation failures.
#[cfg(all(coverage, target_os = "linux"))]
#[test]
fn test_begin_reports_injected_rooted_staging_generation_error() {
    const TEST_NAME: &str = concat!(
        "local::local_root_atomic_writer_tests::",
        "test_begin_reports_injected_rooted_staging_generation_error",
    );
    assert_injected_rooted_begin_error(TEST_NAME, "rooted-staging-generate");
}

/// Verifies exhaustion of injected rooted staging-name collisions.
#[cfg(all(coverage, target_os = "linux"))]
#[test]
fn test_begin_reports_injected_rooted_staging_collision_exhaustion() {
    const TEST_NAME: &str = concat!(
        "local::local_root_atomic_writer_tests::",
        "test_begin_reports_injected_rooted_staging_collision_exhaustion",
    );
    assert_injected_rooted_begin_error(TEST_NAME, "rooted-staging-collision");
}

/// Verifies propagation of rooted staging open failures.
#[cfg(all(coverage, target_os = "linux"))]
#[test]
fn test_begin_reports_injected_rooted_staging_open_error() {
    const TEST_NAME: &str = concat!(
        "local::local_root_atomic_writer_tests::",
        "test_begin_reports_injected_rooted_staging_open_error",
    );
    assert_injected_rooted_begin_error(TEST_NAME, "rooted-staging-open");
}

/// Verifies propagation of injected rooted parent-creation failures.
#[cfg(all(coverage, target_os = "linux"))]
#[test]
fn test_begin_reports_injected_rooted_parent_creation_error() {
    const TEST_NAME: &str = concat!(
        "local::local_root_atomic_writer_tests::",
        "test_begin_reports_injected_rooted_parent_creation_error",
    );
    let Some(()) =
        run_in_coverage_fault_process(TEST_NAME, "rooted-mkdir-error", || {
            let root_path = temp_dir("rooted-mkdir-error");
            let root = LocalRoot::open(&root_path).expect("root should open");
            let destination = LocalRelativePath::new("nested/result.txt")
                .expect("destination should validate");

            let error = root
                .begin_atomic_write(&destination)
                .expect_err("injected parent creation should fail");

            assert_eq!(LocalAtomicWriteStage::PrepareParent, error.stage());
            fs::remove_dir_all(root_path)
                .expect("test directory should be removed");
        })
    else {
        return;
    };
}

/// Verifies descriptor-relative atomic replacement and explicit abort cleanup.
#[cfg(unix)]
#[test]
fn test_begin_atomic_write_commits_and_aborts() {
    let root_path = temp_dir("rooted-atomic-lifecycle");
    fs::write(root_path.join("result.txt"), b"old")
        .expect("destination fixture should be written");
    let root = LocalRoot::open(&root_path).expect("root should open");
    let destination = LocalRelativePath::new("result.txt")
        .expect("destination should validate");

    let mut writer = root
        .begin_atomic_write(&destination)
        .expect("rooted atomic writer should begin");
    writer
        .write_all(b"new")
        .expect("rooted atomic writer should write");
    writer.commit().expect("rooted atomic writer should commit");
    assert_eq!(
        b"new",
        fs::read(root_path.join("result.txt"))
            .expect("committed destination should be readable")
            .as_slice(),
    );

    let mut writer = root
        .begin_atomic_write(&destination)
        .expect("second rooted atomic writer should begin");
    writer
        .write_all(b"discarded")
        .expect("aborted data should be staged");
    writer.abort().expect("rooted atomic writer should abort");
    assert_eq!(0, count_atomic_temp_files(&root_path));
    assert_eq!(
        b"new",
        fs::read(root_path.join("result.txt"))
            .expect("aborted destination should remain readable")
            .as_slice(),
    );
    fs::remove_dir_all(root_path).expect("atomic fixture should be removed");
}

/// Verifies vectored writes through the descriptor-relative staging handle.
#[cfg(unix)]
#[test]
fn test_root_atomic_writer_forwards_vectored_writes() {
    let root_path = temp_dir("rooted-atomic-vectored");
    let root = LocalRoot::open(&root_path).expect("root should open");
    let destination = LocalRelativePath::new("result.txt")
        .expect("destination should validate");
    let mut writer = root
        .begin_atomic_write(&destination)
        .expect("rooted atomic writer should begin");
    let buffers = [IoSlice::new(b"ab"), IoSlice::new(b"cd")];

    let count = writer
        .write_vectored(&buffers)
        .expect("vectored write should succeed");
    writer.commit().expect("rooted atomic writer should commit");

    assert_eq!(4, count);
    assert_eq!(
        b"abcd",
        fs::read(root_path.join("result.txt"))
            .expect("committed destination should be readable")
            .as_slice(),
    );
    assert_eq!(0, count_atomic_temp_files(&root_path));
    fs::remove_dir_all(root_path).expect("atomic fixture should be removed");
}

/// Verifies best-effort cleanup when an uncommitted rooted writer is dropped.
#[cfg(unix)]
#[test]
fn test_drop_removes_rooted_atomic_staging_file() {
    let root_path = temp_dir("rooted-atomic-drop");
    let root = LocalRoot::open(&root_path).expect("root should open");
    let destination = LocalRelativePath::new("result.txt")
        .expect("destination should validate");

    let mut writer = root
        .begin_atomic_write(&destination)
        .expect("rooted atomic writer should begin");
    writer
        .write_all(b"discarded")
        .expect("discarded bytes should be staged");
    assert_eq!(1, count_atomic_temp_files(&root_path));
    drop(writer);

    assert_eq!(0, count_atomic_temp_files(&root_path));
    assert!(!root_path.join("result.txt").exists());
    fs::remove_dir_all(root_path).expect("atomic fixture should be removed");
}

/// Verifies that rooted atomic creation uses anchored parent traversal and
/// remains valid when the diagnostic root path is renamed.
#[cfg(unix)]
#[test]
fn test_commit_survives_root_rename_and_creates_parents() {
    let root_path = temp_dir("rooted-atomic-rename");
    let root = LocalRoot::open(&root_path).expect("root should open");
    let destination = LocalRelativePath::new("nested/result.txt")
        .expect("destination should validate");
    let mut writer = root
        .begin_atomic_write(&destination)
        .expect("rooted atomic writer should create its parent");
    writer
        .write_all(b"anchored")
        .expect("rooted atomic writer should write");
    let moved_path = root_path.with_extension("moved");
    fs::rename(&root_path, &moved_path).expect("root should be renamed");
    fs::create_dir(&root_path)
        .expect("replacement diagnostic root should exist");

    writer
        .commit()
        .expect("commit should use anchored parent descriptors");

    assert_eq!(
        b"anchored",
        fs::read(moved_path.join("nested/result.txt"))
            .expect("anchored destination should be readable")
            .as_slice(),
    );
    assert!(!root_path.join("nested/result.txt").exists());
    fs::remove_dir_all(root_path).expect("replacement root should be removed");
    fs::remove_dir_all(moved_path).expect("moved root should be removed");
}

/// Verifies that an atomic writer commits through its opened parent descriptor
/// after the intermediate name is replaced by an outside symlink.
#[cfg(unix)]
#[test]
fn test_commit_survives_intermediate_directory_replacement() {
    use std::os::unix::fs::symlink;

    let fixture = temp_dir("rooted-atomic-parent-replacement");
    let root_path = fixture.join("root");
    let parent_path = root_path.join("parent");
    let moved_parent_path = root_path.join("moved-parent");
    let outside_path = fixture.join("outside");
    fs::create_dir_all(&parent_path).expect("rooted parent should be created");
    fs::create_dir(&outside_path).expect("outside directory should be created");
    let root = LocalRoot::open(&root_path).expect("root should open");
    let destination = LocalRelativePath::new("parent/data.txt")
        .expect("destination should validate");
    let mut writer = root
        .begin_atomic_write(&destination)
        .expect("rooted atomic writer should begin");
    writer
        .write_all(b"anchored")
        .expect("rooted atomic contents should be staged");
    fs::rename(&parent_path, &moved_parent_path)
        .expect("intermediate parent should be renamed");
    symlink(&outside_path, &parent_path)
        .expect("outside symlink should replace the intermediate name");

    writer
        .commit()
        .expect("commit should use the opened parent descriptor");

    assert_eq!(
        b"anchored",
        fs::read(moved_parent_path.join("data.txt"))
            .expect("moved destination should be readable")
            .as_slice(),
    );
    assert!(!outside_path.join("data.txt").exists());
    assert_eq!(0, count_atomic_temp_files(&moved_parent_path));
    fs::remove_file(parent_path)
        .expect("replacement symlink should be removed");
    fs::remove_dir_all(fixture).expect("atomic fixture should be removed");
}

/// Verifies that commit synchronizes every newly created rooted parent entry.
#[cfg(target_os = "linux")]
#[test]
fn test_commit_syncs_new_parent_chain() {
    if env::var_os(ROOTED_ATOMIC_SYNC_CHILD_ENV).is_some() {
        let root_path = PathBuf::from(
            env::var_os(ROOTED_ATOMIC_SYNC_ROOT_ENV)
                .expect("traced child should receive its rooted fixture"),
        );
        let root = LocalRoot::open(&root_path).expect("root should open");
        let destination = LocalRelativePath::new("a/b/result.txt")
            .expect("destination should validate");
        let mut writer = root
            .begin_atomic_write(&destination)
            .expect("rooted atomic writer should create parents");
        writer
            .write_all(b"durable")
            .expect("rooted atomic writer should write");
        writer.commit().expect("rooted atomic writer should commit");
        return;
    }

    if Command::new("strace").arg("--version").output().is_err() {
        eprintln!("skipping rooted fsync trace because strace is unavailable");
        return;
    }

    let root_path = temp_dir("rooted-atomic-sync-chain");
    let trace_path = root_path.with_extension("strace");
    let status = Command::new("strace")
        .args(["-f", "-y", "-e", "trace=fsync", "-o"])
        .arg(&trace_path)
        .arg(env::current_exe().expect("test executable path should resolve"))
        .args([
            "--exact",
            "local::local_root_atomic_writer_tests::test_commit_syncs_new_parent_chain",
            "--nocapture",
        ])
        .env(ROOTED_ATOMIC_SYNC_CHILD_ENV, "1")
        .env(ROOTED_ATOMIC_SYNC_ROOT_ENV, &root_path)
        .status()
        .expect("strace should launch the traced child");
    assert!(status.success(), "traced child should succeed");

    let trace = fs::read_to_string(&trace_path)
        .expect("fsync trace should be readable");
    let nested_marker = format!("<{}>", root_path.join("a/b").display());
    let parent_marker = format!("<{}>", root_path.join("a").display());
    let root_marker = format!("<{}>", root_path.display());
    let nested_index = trace
        .find(&nested_marker)
        .expect("trace should synchronize the final parent");
    let parent_index = trace
        .find(&parent_marker)
        .expect("trace should synchronize the created parent entry");
    let root_index = trace
        .find(&root_marker)
        .expect("trace should synchronize the created root child entry");
    assert!(
        nested_index < parent_index && parent_index < root_index,
        "rooted parent descriptors should be synchronized deepest first: {trace}"
    );

    fs::remove_file(trace_path).expect("trace file should be removed");
    fs::remove_dir_all(root_path).expect("sync fixture should be removed");
}

/// Verifies permission preservation and final symbolic-link denial.
#[cfg(unix)]
#[test]
fn test_begin_atomic_write_preserves_permissions_and_rejects_symlink() {
    use std::os::unix::fs::{
        PermissionsExt,
        symlink,
    };

    let fixture = temp_dir("rooted-atomic-permissions");
    let root_path = fixture.join("root");
    fs::create_dir(&root_path).expect("root should be created");
    let destination_path = root_path.join("result.txt");
    fs::write(&destination_path, b"old")
        .expect("destination fixture should be written");
    fs::set_permissions(&destination_path, fs::Permissions::from_mode(0o640))
        .expect("destination permissions should be set");
    let root = LocalRoot::open(&root_path).expect("root should open");
    let destination = LocalRelativePath::new("result.txt")
        .expect("destination should validate");
    let mut writer = root
        .begin_atomic_write(&destination)
        .expect("rooted atomic writer should begin");
    writer
        .write_all(b"new")
        .expect("replacement should be staged");
    writer.commit().expect("replacement should commit");
    assert_eq!(
        0o640,
        fs::metadata(&destination_path)
            .expect("destination metadata should be readable")
            .permissions()
            .mode()
            & 0o777,
    );

    let outside_path = fixture.join("outside.txt");
    fs::write(&outside_path, b"outside")
        .expect("outside fixture should be written");
    let linked_path = root_path.join("linked.txt");
    symlink(&outside_path, &linked_path)
        .expect("final symlink should be created");
    let linked = LocalRelativePath::new("linked.txt")
        .expect("linked destination should validate lexically");
    let error = root
        .begin_atomic_write(&linked)
        .expect_err("final symlink should be rejected");

    assert_eq!(LocalAtomicWriteStage::InspectDestination, error.stage());
    assert_eq!(b"outside", fs::read(outside_path).unwrap().as_slice());
    fs::remove_dir_all(fixture).expect("permission fixture should be removed");
}

/// Verifies that rooted commit snapshots mode and xattrs at commit time.
#[cfg(any(
    target_os = "linux",
    target_os = "android",
    target_os = "macos",
    target_os = "freebsd",
))]
#[test]
fn test_begin_atomic_write_preserves_commit_time_metadata() {
    use std::os::unix::fs::PermissionsExt;

    const XATTR_NAME: &str = "user.qubit-local-files";

    let root_path = temp_dir("rooted-atomic-commit-time-metadata");
    let destination_path = root_path.join("result.txt");
    fs::write(&destination_path, b"old")
        .expect("destination fixture should be written");
    fs::set_permissions(&destination_path, fs::Permissions::from_mode(0o600))
        .expect("initial destination mode should be set");
    set_user_xattr(&destination_path, XATTR_NAME, b"initial")
        .expect("initial destination xattr should be set");
    let root = LocalRoot::open(&root_path).expect("root should open");
    let destination = LocalRelativePath::new("result.txt")
        .expect("destination should validate");
    let mut writer = root
        .begin_atomic_write(&destination)
        .expect("rooted atomic writer should begin");
    fs::set_permissions(&destination_path, fs::Permissions::from_mode(0o640))
        .expect("destination mode should change before commit");
    set_user_xattr(&destination_path, XATTR_NAME, b"latest")
        .expect("destination xattr should change before commit");
    writer
        .write_all(b"new")
        .expect("replacement contents should be staged");

    writer
        .commit()
        .expect("rooted atomic commit should succeed");

    assert_eq!(
        0o640,
        fs::metadata(&destination_path)
            .expect("committed metadata should be readable")
            .permissions()
            .mode()
            & 0o777,
    );
    assert_eq!(
        b"latest",
        get_user_xattr(&destination_path, XATTR_NAME)
            .expect("committed xattr should be readable")
            .as_slice(),
    );
    fs::remove_dir_all(root_path).expect("test directory should be removed");
}

/// Verifies that a final entry replaced by a symbolic link after staging is
/// rejected without modifying the link target.
#[cfg(unix)]
#[test]
fn test_commit_rejects_final_symlink_replacement() {
    use std::os::unix::fs::symlink;

    let fixture = temp_dir("rooted-atomic-final-replacement");
    let root_path = fixture.join("root");
    fs::create_dir(&root_path).expect("root should be created");
    let destination_path = root_path.join("result.txt");
    fs::write(&destination_path, b"old")
        .expect("destination fixture should be written");
    let outside_path = fixture.join("outside.txt");
    fs::write(&outside_path, b"outside")
        .expect("outside fixture should be written");
    let root = LocalRoot::open(&root_path).expect("root should open");
    let destination = LocalRelativePath::new("result.txt")
        .expect("destination should validate");
    let mut writer = root
        .begin_atomic_write(&destination)
        .expect("rooted atomic writer should begin");
    writer
        .write_all(b"replacement")
        .expect("replacement should be staged");
    let displaced_path = root_path.join("displaced.txt");
    fs::rename(&destination_path, &displaced_path)
        .expect("destination should be displaced");
    symlink(&outside_path, &destination_path)
        .expect("destination symlink should be installed");

    let error = writer
        .commit()
        .expect_err("commit should reject the replacement symlink");

    assert_eq!(
        LocalAtomicWriteStage::ReadDestinationMetadata,
        error.stage(),
    );
    assert_eq!(b"outside", fs::read(&outside_path).unwrap().as_slice());
    assert!(destination_path.is_symlink());
    assert_eq!(0, count_atomic_temp_files(&root_path));
    fs::remove_dir_all(fixture).expect("replacement fixture should be removed");
}

/// Verifies explicit flushing and creation of a previously missing atomic
/// destination.
#[cfg(unix)]
#[test]
fn test_commit_flushes_and_creates_missing_destination() {
    let root_path = temp_dir("rooted-atomic-new");
    let root = LocalRoot::open(&root_path).expect("root should open");
    let destination = LocalRelativePath::new("result.txt")
        .expect("destination should validate");
    let mut writer = root
        .begin_atomic_write(&destination)
        .expect("rooted atomic writer should begin");

    writer.write_all(b"new").expect("new data should be staged");
    writer.flush().expect("staging file should flush");
    writer.commit().expect("new destination should commit");

    assert_eq!(
        b"new",
        fs::read(root_path.join("result.txt")).unwrap().as_slice(),
    );
    fs::remove_dir_all(root_path)
        .expect("new atomic fixture should be removed");
}

/// Verifies that a pre-installation rooted failure returns its writer for
/// abort.
#[cfg(unix)]
#[test]
fn test_root_atomic_writer_recoverable_commit_returns_writer_for_abort() {
    let root_path = temp_dir("rooted-atomic-recoverable-commit");
    let destination_path = root_path.join("result.txt");
    fs::write(&destination_path, b"original")
        .expect("destination should be written");
    let root = LocalRoot::open(&root_path).expect("root should open");
    let destination = LocalRelativePath::new("result.txt")
        .expect("destination should validate");
    let mut writer = root
        .begin_atomic_write(&destination)
        .expect("rooted atomic writer should begin");
    writer
        .write_all(b"replacement")
        .expect("replacement should be staged");
    fs::remove_file(&destination_path).expect("destination should be removed");

    let mut commit_error = writer
        .commit_recoverable()
        .expect_err("missing destination should retain the rooted writer");
    assert_eq!(
        LocalAtomicDestinationState::Missing,
        commit_error.error().destination_state(),
    );
    assert!(commit_error.writer().is_some());
    assert!(commit_error.writer_mut().is_some());
    let (error, writer) = commit_error.into_parts();
    let writer =
        writer.expect("pre-publication failure should return rooted writer");

    assert_eq!(
        LocalAtomicDestinationState::Missing,
        error.destination_state(),
    );
    assert_eq!(1, count_atomic_temp_files(&root_path));
    writer
        .abort()
        .expect("returned rooted writer should remove staging");
    assert_eq!(0, count_atomic_temp_files(&root_path));
    fs::remove_dir_all(root_path).expect("test directory should be removed");
}

/// Verifies rooted no-replace installation and missing-state staging retention.
#[cfg(unix)]
#[test]
fn test_commit_reports_precise_namespace_states() {
    let root_path = temp_dir("rooted-atomic-namespace-states");
    let root = LocalRoot::open(&root_path).expect("root should open");
    let created = LocalRelativePath::new("created.txt")
        .expect("created destination should validate");
    let mut writer = root
        .begin_atomic_write(&created)
        .expect("missing destination writer should begin");
    writer
        .write_all(b"replacement")
        .expect("replacement should be staged");
    fs::write(root_path.join("created.txt"), b"concurrent")
        .expect("concurrent destination should be created");

    let error = writer
        .commit()
        .expect_err("rooted install must not replace a concurrent target");

    assert_eq!(
        LocalAtomicDestinationState::Unchanged,
        error.destination_state(),
    );
    assert_eq!(
        b"concurrent",
        fs::read(root_path.join("created.txt"))
            .expect("concurrent destination should remain")
            .as_slice(),
    );
    assert_eq!(0, count_atomic_temp_files(&root_path));

    let existing = LocalRelativePath::new("existing.txt")
        .expect("existing destination should validate");
    fs::write(root_path.join("existing.txt"), b"original")
        .expect("existing destination should be written");
    let mut writer = root
        .begin_atomic_write(&existing)
        .expect("existing destination writer should begin");
    writer
        .write_all(b"retained")
        .expect("replacement should be staged");
    fs::remove_file(root_path.join("existing.txt"))
        .expect("existing destination should disappear");

    let error = writer
        .commit()
        .expect_err("missing existing destination should reject replacement");

    assert_eq!(
        LocalAtomicDestinationState::Missing,
        error.destination_state(),
    );
    assert_eq!(1, count_atomic_temp_files(&root_path));
    fs::remove_dir_all(root_path).expect("test directory should be removed");
}

/// Verifies structured preparation errors for an ordinary-file parent and a
/// directory destination.
#[cfg(unix)]
#[test]
fn test_begin_atomic_write_reports_parent_and_destination_type_errors() {
    let root_path = temp_dir("rooted-atomic-types");
    fs::write(root_path.join("parent-file"), b"file")
        .expect("parent file fixture should be written");
    fs::create_dir(root_path.join("destination-dir"))
        .expect("destination directory fixture should be created");
    let root = LocalRoot::open(&root_path).expect("root should open");

    let invalid_parent = LocalRelativePath::new("parent-file/result.txt")
        .expect("invalid parent should validate lexically");
    let error = root
        .begin_atomic_write(&invalid_parent)
        .expect_err("ordinary-file parent should fail");
    assert_eq!(LocalAtomicWriteStage::PrepareParent, error.stage());

    let directory = LocalRelativePath::new("destination-dir")
        .expect("directory destination should validate lexically");
    let error = root
        .begin_atomic_write(&directory)
        .expect_err("directory destination should fail");
    assert_eq!(LocalAtomicWriteStage::InspectDestination, error.stage());

    fs::remove_dir_all(root_path)
        .expect("atomic-type fixture should be removed");
}

/// Verifies that explicit abort reports a staging entry removed behind the
/// writer instead of silently claiming successful cleanup.
#[cfg(unix)]
#[test]
fn test_abort_reports_missing_staging_entry() {
    let root_path = temp_dir("rooted-atomic-abort-error");
    let root = LocalRoot::open(&root_path).expect("root should open");
    let destination = LocalRelativePath::new("result.txt")
        .expect("destination should validate");
    let writer = root
        .begin_atomic_write(&destination)
        .expect("rooted atomic writer should begin");
    let temporary_path = fs::read_dir(&root_path)
        .expect("root directory should be readable")
        .map(|entry| entry.expect("root entry should be readable").path())
        .find(|path| {
            path.file_name().and_then(|name| name.to_str()).is_some_and(
                |name| {
                    name.starts_with(".atomic-write-") && name.ends_with(".tmp")
                },
            )
        })
        .expect("staging entry should exist");
    fs::remove_file(temporary_path)
        .expect("staging entry should be removed behind the writer");

    let error = writer
        .abort()
        .expect_err("abort should report the missing staging entry");

    assert_eq!(LocalAtomicWriteStage::CleanupTemporaryFile, error.stage());
    assert_eq!(std::io::ErrorKind::NotFound, error.kind());
    fs::remove_dir_all(root_path)
        .expect("abort-error fixture should be removed");
}
