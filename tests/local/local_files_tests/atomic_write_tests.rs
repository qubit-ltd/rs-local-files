// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

#[cfg(coverage)]
use qubit_local_files::LocalAtomicWriteError;
use qubit_local_files::{
    LocalAtomicDestinationState,
    LocalAtomicWriteStage,
    LocalFiles,
};
use std::error::Error as StdError;
use std::io::{
    Error,
    ErrorKind,
    Write,
};

#[cfg(unix)]
use super::super::test_support::PermissionsExt;
#[cfg(unix)]
use super::super::test_support::create_fifo;
#[cfg(coverage)]
use super::super::test_support::run_in_coverage_fault_process;
use super::super::test_support::{
    CURRENT_DIR_LOCK,
    CurrentDirGuard,
    count_atomic_temp_files,
    fs,
    temp_dir,
};
#[cfg(windows)]
use super::super::test_support::{
    alternate_data_stream_path,
    clear_readonly_attribute,
    path_with_interior_nul,
    read_dacl_bytes,
    set_world_full_control_dacl,
};
#[cfg(any(
    target_os = "linux",
    target_os = "android",
    target_os = "macos",
    target_os = "freebsd",
))]
use super::super::test_support::{
    get_user_xattr,
    set_user_xattr,
};
#[cfg(target_os = "freebsd")]
use super::super::test_support::{
    install_supported_test_acl,
    read_freebsd_acl_text,
};
#[cfg(target_os = "macos")]
use super::super::test_support::{
    read_macos_acl_text,
    set_current_user_read_acl,
};
#[cfg(target_os = "linux")]
use super::copy_dir_tests::directory_write_restrictions_are_enforced;

/// Runs one atomic-write fault in an isolated coverage subprocess.
#[cfg(all(coverage, target_os = "linux"))]
fn run_atomic_write_fault(
    test_name: &str,
    fault: &str,
    destination_existed: bool,
) -> Option<(
    std::path::PathBuf,
    std::path::PathBuf,
    Result<(), LocalAtomicWriteError>,
)> {
    run_in_coverage_fault_process(test_name, fault, move || {
        let dir = temp_dir(fault);
        let destination = dir.join("destination.txt");
        if destination_existed {
            fs::write(&destination, b"old")
                .expect("existing destination should be written");
        }
        let result = LocalFiles::atomic_write(&destination, b"new");
        (dir, destination, result)
    })
}

/// Asserts one injected metadata-preservation failure through atomic write.
#[cfg(all(coverage, target_os = "linux"))]
fn assert_injected_metadata_error(test_name: &str, fault: &str) {
    let Some((dir, destination, result)) =
        run_in_coverage_fault_process(test_name, fault, move || {
            let dir = temp_dir(fault);
            let destination = dir.join("destination.txt");
            fs::write(&destination, b"old")
                .expect("existing destination should be written");
            set_user_xattr(&destination, "user.coverage-source", b"value")
                .expect("source xattr should be written");
            let result = LocalFiles::atomic_write(&destination, b"new");
            (dir, destination, result)
        })
    else {
        return;
    };

    let error = result.expect_err("injected metadata operation should fail");
    assert_eq!(
        LocalAtomicWriteStage::ApplyDestinationMetadata,
        error.stage(),
    );
    assert_eq!(
        LocalAtomicDestinationState::Unchanged,
        error.destination_state(),
    );
    assert_eq!(b"old", fs::read(&destination).unwrap().as_slice());
    fs::remove_dir_all(dir).expect("test directory should be removed");
}

/// Asserts one injected existing-destination atomic failure.
#[cfg(all(coverage, target_os = "linux"))]
fn assert_injected_atomic_error(
    test_name: &str,
    fault: &str,
    expected_stage: LocalAtomicWriteStage,
) {
    let Some((dir, destination, result)) =
        run_atomic_write_fault(test_name, fault, true)
    else {
        return;
    };

    let error = result.expect_err("injected atomic operation should fail");
    assert_eq!(expected_stage, error.stage());
    assert_eq!(
        LocalAtomicDestinationState::Unchanged,
        error.destination_state(),
    );
    assert_eq!(b"old", fs::read(&destination).unwrap().as_slice());
    fs::remove_dir_all(dir).expect("test directory should be removed");
}

/// Verifies that an injected native no-replace failure keeps the destination
/// unchanged and reports the replacement stage.
#[cfg(all(coverage, target_os = "linux"))]
#[test]
fn test_atomic_write_reports_injected_native_install_error() {
    const TEST_NAME: &str = concat!(
        "local::local_files_tests::atomic_write_tests::",
        "test_atomic_write_reports_injected_native_install_error",
    );

    let dir = temp_dir("atomic-injected-native-install-error");
    let destination = dir.join("destination.txt");
    let child_destination = destination.clone();
    let Some(result) = run_in_coverage_fault_process(
        TEST_NAME,
        "atomic-install-before-native-call",
        move || LocalFiles::atomic_write(&child_destination, b"new"),
    ) else {
        fs::remove_dir_all(dir).expect("test directory should be removed");
        return;
    };

    let error = result.expect_err("injected native install should fail");
    assert_eq!(LocalAtomicWriteStage::ReplaceDestination, error.stage());
    assert_eq!(
        LocalAtomicDestinationState::Unchanged,
        error.destination_state(),
    );
    assert!(!destination.exists(), "destination must remain missing");
    assert_eq!(0, count_atomic_temp_files(&dir));
    fs::remove_dir_all(dir).expect("test directory should be removed");
}

/// Verifies that the native no-replace fallback installs a missing file.
#[cfg(all(coverage, target_os = "linux"))]
#[test]
fn test_atomic_write_uses_injected_native_install_fallback() {
    const TEST_NAME: &str = concat!(
        "local::local_files_tests::atomic_write_tests::",
        "test_atomic_write_uses_injected_native_install_fallback",
    );
    let Some((dir, destination, result)) =
        run_atomic_write_fault(TEST_NAME, "atomic-install-fallback", false)
    else {
        return;
    };

    result.expect("fallback install should succeed");
    assert_eq!(b"new", fs::read(&destination).unwrap().as_slice());
    fs::remove_dir_all(dir).expect("test directory should be removed");
}

/// Verifies that an injected fallback link failure leaves no destination.
#[cfg(all(coverage, target_os = "linux"))]
#[test]
fn test_atomic_write_reports_injected_install_link_error() {
    const TEST_NAME: &str = concat!(
        "local::local_files_tests::atomic_write_tests::",
        "test_atomic_write_reports_injected_install_link_error",
    );
    let Some((dir, destination, result)) =
        run_atomic_write_fault(TEST_NAME, "atomic-install-link", false)
    else {
        return;
    };

    let error = result.expect_err("injected link should fail");
    assert_eq!(LocalAtomicWriteStage::ReplaceDestination, error.stage());
    assert_eq!(
        LocalAtomicDestinationState::Unchanged,
        error.destination_state()
    );
    assert!(!destination.exists());
    fs::remove_dir_all(dir).expect("test directory should be removed");
}

/// Verifies that one injected fallback unlink failure is retried.
#[cfg(all(coverage, target_os = "linux"))]
#[test]
fn test_atomic_write_retries_injected_install_unlink_error() {
    const TEST_NAME: &str = concat!(
        "local::local_files_tests::atomic_write_tests::",
        "test_atomic_write_retries_injected_install_unlink_error",
    );
    let Some((dir, destination, result)) =
        run_atomic_write_fault(TEST_NAME, "atomic-install-unlink", false)
    else {
        return;
    };

    result.expect("a one-shot staging unlink failure should be recovered");
    assert_eq!(b"new", fs::read(&destination).unwrap().as_slice());
    assert_eq!(0, count_atomic_temp_files(&dir));
    fs::remove_dir_all(dir).expect("test directory should be removed");
}

/// Verifies that persistent fallback unlink failures retain every error and
/// leave staging available for recovery.
#[cfg(all(coverage, target_os = "linux"))]
#[test]
fn test_atomic_write_reports_persistent_install_unlink_error() {
    const TEST_NAME: &str = concat!(
        "local::local_files_tests::atomic_write_tests::",
        "test_atomic_write_reports_persistent_install_unlink_error",
    );
    let Some((dir, destination, result)) = run_atomic_write_fault(
        TEST_NAME,
        "atomic-install-unlink-persistent",
        false,
    ) else {
        return;
    };

    let error = result.expect_err("persistent staging unlink should fail");
    assert_eq!(LocalAtomicWriteStage::ReplaceDestination, error.stage());
    assert_eq!(
        LocalAtomicDestinationState::Replaced,
        error.destination_state()
    );
    assert_eq!(
        Some(libc::EIO),
        error.cleanup_error().and_then(Error::raw_os_error)
    );
    assert_eq!(b"new", fs::read(&destination).unwrap().as_slice());
    assert_eq!(1, count_atomic_temp_files(&dir));
    fs::remove_dir_all(dir).expect("test directory should be removed");
}

/// Verifies parent synchronization after guard-level staging recovery.
#[cfg(all(coverage, target_os = "linux"))]
#[test]
fn test_atomic_write_syncs_parent_after_install_unlink_recovery() {
    const TEST_NAME: &str = concat!(
        "local::local_files_tests::atomic_write_tests::",
        "test_atomic_write_syncs_parent_after_install_unlink_recovery",
    );
    let Some((dir, destination, result)) = run_atomic_write_fault(
        TEST_NAME,
        "atomic-install-unlink-recover-sync",
        false,
    ) else {
        return;
    };

    let error = result.expect_err("injected parent sync should fail");
    assert_eq!(LocalAtomicWriteStage::SyncParent, error.stage());
    assert_eq!(
        LocalAtomicDestinationState::Replaced,
        error.destination_state(),
    );
    assert_eq!(
        Some(libc::EIO),
        error.source().and_then(|source| {
            source.downcast_ref::<Error>().and_then(Error::raw_os_error)
        })
    );
    assert!(error.cleanup_error().is_none());
    assert_eq!(b"new", fs::read(&destination).unwrap().as_slice());
    assert_eq!(0, count_atomic_temp_files(&dir));
    fs::remove_dir_all(dir).expect("test directory should be removed");
}

/// Verifies that an indeterminate staging name is preserved while a secondary
/// parent-sync failure remains inspectable.
#[cfg(all(coverage, target_os = "linux"))]
#[test]
fn test_atomic_write_retains_sync_error_with_indeterminate_staging() {
    const TEST_NAME: &str = concat!(
        "local::local_files_tests::atomic_write_tests::",
        "test_atomic_write_retains_sync_error_with_indeterminate_staging",
    );
    let Some((dir, destination, result)) = run_atomic_write_fault(
        TEST_NAME,
        "atomic-install-unlink-indeterminate-sync",
        false,
    ) else {
        return;
    };

    let error = result.expect_err("indeterminate staging unlink should fail");
    assert_eq!(LocalAtomicWriteStage::ReplaceDestination, error.stage());
    assert_eq!(
        LocalAtomicDestinationState::Replaced,
        error.destination_state(),
    );
    assert!(error.cleanup_error().is_none());
    assert_eq!(
        Some(libc::EIO),
        error.parent_sync_error().and_then(Error::raw_os_error),
    );
    assert!(
        error
            .to_string()
            .contains("parent synchronization also failed"),
    );
    assert_eq!(b"new", fs::read(&destination).unwrap().as_slice());
    assert_eq!(1, count_atomic_temp_files(&dir));
    fs::remove_dir_all(dir).expect("test directory should be removed");
}

/// Verifies that an unrecovered staging unlink keeps a secondary parent-sync
/// failure without replacing the primary installation error.
#[cfg(all(coverage, target_os = "linux"))]
#[test]
fn test_atomic_write_retains_sync_error_after_persistent_install_unlink() {
    const TEST_NAME: &str = concat!(
        "local::local_files_tests::atomic_write_tests::",
        "test_atomic_write_retains_sync_error_after_persistent_install_unlink",
    );
    let Some((dir, destination, result)) = run_atomic_write_fault(
        TEST_NAME,
        "atomic-install-unlink-persistent-sync",
        false,
    ) else {
        return;
    };

    let error = result.expect_err("persistent staging unlink should fail");
    assert_eq!(LocalAtomicWriteStage::ReplaceDestination, error.stage());
    assert_eq!(
        LocalAtomicDestinationState::Replaced,
        error.destination_state(),
    );
    assert_eq!(
        Some(libc::EIO),
        error.cleanup_error().and_then(Error::raw_os_error),
    );
    assert_eq!(
        Some(libc::EIO),
        error.parent_sync_error().and_then(Error::raw_os_error),
    );
    assert!(
        error
            .to_string()
            .contains("parent synchronization also failed"),
    );
    assert_eq!(b"new", fs::read(&destination).unwrap().as_slice());
    assert_eq!(1, count_atomic_temp_files(&dir));
    fs::remove_dir_all(dir).expect("test directory should be removed");
}

/// Verifies that an injected existing-file replacement error is normalized.
#[cfg(all(coverage, target_os = "linux"))]
#[test]
fn test_atomic_write_reports_injected_existing_replace_error() {
    const TEST_NAME: &str = concat!(
        "local::local_files_tests::atomic_write_tests::",
        "test_atomic_write_reports_injected_existing_replace_error",
    );
    let Some((dir, destination, result)) =
        run_atomic_write_fault(TEST_NAME, "atomic-install-replace", true)
    else {
        return;
    };

    let error = result.expect_err("injected replacement should fail");
    assert_eq!(LocalAtomicWriteStage::ReplaceDestination, error.stage());
    assert_eq!(
        LocalAtomicDestinationState::Unchanged,
        error.destination_state()
    );
    assert_eq!(b"old", fs::read(&destination).unwrap().as_slice());
    fs::remove_dir_all(dir).expect("test directory should be removed");
}

/// Verifies that an indeterminate replacement outcome preserves staging for
/// caller-directed recovery.
#[cfg(all(coverage, target_os = "linux"))]
#[test]
fn test_atomic_write_preserves_staging_after_indeterminate_replace_error() {
    const TEST_NAME: &str = concat!(
        "local::local_files_tests::atomic_write_tests::",
        "test_atomic_write_preserves_staging_after_indeterminate_replace_error",
    );
    let Some((dir, destination, result)) = run_atomic_write_fault(
        TEST_NAME,
        "atomic-install-replace-indeterminate",
        true,
    ) else {
        return;
    };

    let error = result.expect_err("indeterminate replacement should fail");
    assert_eq!(LocalAtomicWriteStage::ReplaceDestination, error.stage());
    assert_eq!(
        LocalAtomicDestinationState::Indeterminate,
        error.destination_state(),
    );
    assert!(error.cleanup_error().is_none());
    assert!(error.parent_sync_error().is_none());
    assert_eq!(b"old", fs::read(&destination).unwrap().as_slice());
    assert_eq!(1, count_atomic_temp_files(&dir));
    fs::remove_dir_all(dir).expect("test directory should be removed");
}

/// Verifies propagation of an injected xattr-list error.
#[cfg(all(coverage, target_os = "linux"))]
#[test]
fn test_atomic_write_reports_injected_metadata_list_error() {
    const TEST_NAME: &str = concat!(
        "local::local_files_tests::atomic_write_tests::",
        "test_atomic_write_reports_injected_metadata_list_error",
    );
    assert_injected_metadata_error(TEST_NAME, "atomic-metadata-list");
}

/// Verifies propagation of an injected xattr-read error.
#[cfg(all(coverage, target_os = "linux"))]
#[test]
fn test_atomic_write_reports_injected_metadata_read_error() {
    const TEST_NAME: &str = concat!(
        "local::local_files_tests::atomic_write_tests::",
        "test_atomic_write_reports_injected_metadata_read_error",
    );
    assert_injected_metadata_error(TEST_NAME, "atomic-metadata-read");
}

/// Verifies propagation of an injected xattr-write error.
#[cfg(all(coverage, target_os = "linux"))]
#[test]
fn test_atomic_write_reports_injected_metadata_write_error() {
    const TEST_NAME: &str = concat!(
        "local::local_files_tests::atomic_write_tests::",
        "test_atomic_write_reports_injected_metadata_write_error",
    );
    assert_injected_metadata_error(TEST_NAME, "atomic-metadata-write");
}

/// Verifies propagation of an injected xattr-removal error.
#[cfg(all(coverage, target_os = "linux"))]
#[test]
fn test_atomic_write_reports_injected_metadata_remove_error() {
    const TEST_NAME: &str = concat!(
        "local::local_files_tests::atomic_write_tests::",
        "test_atomic_write_reports_injected_metadata_remove_error",
    );
    assert_injected_metadata_error(TEST_NAME, "atomic-metadata-remove");
}

/// Verifies propagation of an injected mode-application error.
#[cfg(all(coverage, target_os = "linux"))]
#[test]
fn test_atomic_write_reports_injected_metadata_mode_error() {
    const TEST_NAME: &str = concat!(
        "local::local_files_tests::atomic_write_tests::",
        "test_atomic_write_reports_injected_metadata_mode_error",
    );
    assert_injected_metadata_error(TEST_NAME, "atomic-metadata-mode");
}

/// Verifies propagation of an injected ownership-application error.
#[cfg(all(coverage, target_os = "linux"))]
#[test]
fn test_atomic_write_reports_injected_metadata_owner_error() {
    const TEST_NAME: &str = concat!(
        "local::local_files_tests::atomic_write_tests::",
        "test_atomic_write_reports_injected_metadata_owner_error",
    );
    assert_injected_metadata_error(TEST_NAME, "atomic-metadata-owner");
}

/// Verifies propagation of an injected native ownership syscall failure.
#[cfg(all(coverage, target_os = "linux"))]
#[test]
fn test_atomic_write_reports_injected_metadata_owner_native_error() {
    const TEST_NAME: &str = concat!(
        "local::local_files_tests::atomic_write_tests::",
        "test_atomic_write_reports_injected_metadata_owner_native_error",
    );
    assert_injected_metadata_error(TEST_NAME, "atomic-metadata-owner-native");
}

/// Verifies propagation of an injected source-metadata error.
#[cfg(all(coverage, target_os = "linux"))]
#[test]
fn test_atomic_write_reports_injected_metadata_source_stat_error() {
    const TEST_NAME: &str = concat!(
        "local::local_files_tests::atomic_write_tests::",
        "test_atomic_write_reports_injected_metadata_source_stat_error",
    );
    assert_injected_metadata_error(TEST_NAME, "atomic-metadata-source-stat");
}

/// Verifies propagation of an injected staging-metadata error.
#[cfg(all(coverage, target_os = "linux"))]
#[test]
fn test_atomic_write_reports_injected_metadata_staging_stat_error() {
    const TEST_NAME: &str = concat!(
        "local::local_files_tests::atomic_write_tests::",
        "test_atomic_write_reports_injected_metadata_staging_stat_error",
    );
    assert_injected_metadata_error(TEST_NAME, "atomic-metadata-staging-stat");
}

/// Verifies propagation of an injected native-mode conversion error.
#[cfg(all(coverage, target_os = "linux"))]
#[test]
fn test_atomic_write_reports_injected_metadata_native_mode_error() {
    const TEST_NAME: &str = concat!(
        "local::local_files_tests::atomic_write_tests::",
        "test_atomic_write_reports_injected_metadata_native_mode_error",
    );
    assert_injected_metadata_error(TEST_NAME, "atomic-metadata-native-mode");
}

/// Verifies that an injected unsupported xattr interface skips xattr copying.
#[cfg(all(coverage, target_os = "linux"))]
#[test]
fn test_atomic_write_handles_injected_metadata_not_supported() {
    const TEST_NAME: &str = concat!(
        "local::local_files_tests::atomic_write_tests::",
        "test_atomic_write_handles_injected_metadata_not_supported",
    );
    let Some((dir, destination, result)) = run_in_coverage_fault_process(
        TEST_NAME,
        "atomic-metadata-not-supported",
        move || {
            let dir = temp_dir("atomic-metadata-not-supported");
            let destination = dir.join("destination.txt");
            fs::write(&destination, b"old")
                .expect("existing destination should be written");
            set_user_xattr(&destination, "user.coverage-source", b"value")
                .expect("source xattr should be written");
            let result = LocalFiles::atomic_write(&destination, b"new");
            (dir, destination, result)
        },
    ) else {
        return;
    };

    result.expect("unsupported xattr interface should be tolerated");
    assert_eq!(b"new", fs::read(&destination).unwrap().as_slice());
    assert!(
        get_user_xattr(&destination, "user.coverage-source").is_err(),
        "xattr must not be copied when the interface is unsupported",
    );
    fs::remove_dir_all(dir).expect("test directory should be removed");
}

/// Verifies that an injected disappearing source xattr is reported.
#[cfg(all(coverage, target_os = "linux"))]
#[test]
fn test_atomic_write_reports_injected_metadata_source_missing() {
    const TEST_NAME: &str = concat!(
        "local::local_files_tests::atomic_write_tests::",
        "test_atomic_write_reports_injected_metadata_source_missing",
    );
    assert_injected_metadata_error(TEST_NAME, "atomic-metadata-source-missing");
}

/// Verifies that an injected invalid native xattr name is reported.
#[cfg(all(coverage, target_os = "linux"))]
#[test]
fn test_atomic_write_reports_injected_metadata_invalid_name() {
    const TEST_NAME: &str = concat!(
        "local::local_files_tests::atomic_write_tests::",
        "test_atomic_write_reports_injected_metadata_invalid_name",
    );
    assert_injected_metadata_error(TEST_NAME, "atomic-metadata-invalid-name");
}

/// Verifies propagation of an injected staging xattr-list error.
#[cfg(all(coverage, target_os = "linux"))]
#[test]
fn test_atomic_write_reports_injected_metadata_staging_list_error() {
    const TEST_NAME: &str = concat!(
        "local::local_files_tests::atomic_write_tests::",
        "test_atomic_write_reports_injected_metadata_staging_list_error",
    );
    assert_injected_metadata_error(TEST_NAME, "atomic-metadata-staging-list");
}

/// Verifies propagation of an injected xattr-list buffer read error.
#[cfg(all(coverage, target_os = "linux"))]
#[test]
fn test_atomic_write_reports_injected_metadata_list_read_error() {
    const TEST_NAME: &str = concat!(
        "local::local_files_tests::atomic_write_tests::",
        "test_atomic_write_reports_injected_metadata_list_read_error",
    );
    assert_injected_metadata_error(TEST_NAME, "atomic-metadata-list-read");
}

/// Verifies that an xattr-list size race retries before a later native error.
#[cfg(all(coverage, target_os = "linux"))]
#[test]
fn test_atomic_write_retries_injected_metadata_list_range() {
    const TEST_NAME: &str = concat!(
        "local::local_files_tests::atomic_write_tests::",
        "test_atomic_write_retries_injected_metadata_list_range",
    );
    assert_injected_metadata_error(TEST_NAME, "atomic-metadata-list-range");
}

/// Verifies that persistent xattr-list size races eventually return an error.
#[cfg(all(coverage, target_os = "linux"))]
#[test]
fn test_atomic_write_limits_persistent_metadata_list_range() {
    const TEST_NAME: &str = concat!(
        "local::local_files_tests::atomic_write_tests::",
        "test_atomic_write_limits_persistent_metadata_list_range",
    );
    assert_injected_metadata_error(
        TEST_NAME,
        "atomic-metadata-list-range-persistent",
    );
}

/// Verifies propagation of an injected xattr-value buffer read error.
#[cfg(all(coverage, target_os = "linux"))]
#[test]
fn test_atomic_write_reports_injected_metadata_value_read_error() {
    const TEST_NAME: &str = concat!(
        "local::local_files_tests::atomic_write_tests::",
        "test_atomic_write_reports_injected_metadata_value_read_error",
    );
    assert_injected_metadata_error(TEST_NAME, "atomic-metadata-value-read");
}

/// Verifies that persistent xattr-value size races eventually return an error.
#[cfg(all(coverage, target_os = "linux"))]
#[test]
fn test_atomic_write_limits_persistent_metadata_value_range() {
    const TEST_NAME: &str = concat!(
        "local::local_files_tests::atomic_write_tests::",
        "test_atomic_write_limits_persistent_metadata_value_range",
    );
    assert_injected_metadata_error(
        TEST_NAME,
        "atomic-metadata-value-range-persistent",
    );
}

/// Verifies ordering and lookup of an injected security xattr name.
#[cfg(all(coverage, target_os = "linux"))]
#[test]
fn test_atomic_write_reports_injected_metadata_security_name() {
    const TEST_NAME: &str = concat!(
        "local::local_files_tests::atomic_write_tests::",
        "test_atomic_write_reports_injected_metadata_security_name",
    );
    assert_injected_metadata_error(TEST_NAME, "atomic-metadata-security-name");
}

/// Verifies that an injected equal staging value skips redundant xattr copy.
#[cfg(all(coverage, target_os = "linux"))]
#[test]
fn test_atomic_write_skips_injected_equal_metadata_value() {
    const TEST_NAME: &str = concat!(
        "local::local_files_tests::atomic_write_tests::",
        "test_atomic_write_skips_injected_equal_metadata_value",
    );
    let Some((dir, destination, result)) = run_in_coverage_fault_process(
        TEST_NAME,
        "atomic-metadata-equal-value",
        move || {
            let dir = temp_dir("atomic-metadata-equal-value");
            let destination = dir.join("destination.txt");
            fs::write(&destination, b"old")
                .expect("existing destination should be written");
            set_user_xattr(&destination, "user.coverage-source", b"value")
                .expect("source xattr should be written");
            let result = LocalFiles::atomic_write(&destination, b"new");
            (dir, destination, result)
        },
    ) else {
        return;
    };

    result.expect("equal staging xattr should not be rewritten");
    assert!(
        get_user_xattr(&destination, "user.coverage-source").is_err(),
        "the injected staging value is not persisted by a skipped write",
    );
    fs::remove_dir_all(dir).expect("test directory should be removed");
}

/// Verifies propagation of an injected destination-open error.
#[cfg(all(coverage, target_os = "linux"))]
#[test]
fn test_atomic_write_reports_injected_destination_open_error() {
    const TEST_NAME: &str = concat!(
        "local::local_files_tests::atomic_write_tests::",
        "test_atomic_write_reports_injected_destination_open_error",
    );
    assert_injected_atomic_error(
        TEST_NAME,
        "atomic-destination-open",
        LocalAtomicWriteStage::ReadDestinationMetadata,
    );
}

/// Verifies propagation of an injected destination-stat error.
#[cfg(all(coverage, target_os = "linux"))]
#[test]
fn test_atomic_write_reports_injected_destination_stat_error() {
    const TEST_NAME: &str = concat!(
        "local::local_files_tests::atomic_write_tests::",
        "test_atomic_write_reports_injected_destination_stat_error",
    );
    assert_injected_atomic_error(
        TEST_NAME,
        "atomic-destination-stat",
        LocalAtomicWriteStage::ReadDestinationMetadata,
    );
}

/// Verifies rejection of an injected non-file destination handle.
#[cfg(all(coverage, target_os = "linux"))]
#[test]
fn test_atomic_write_reports_injected_destination_type_error() {
    const TEST_NAME: &str = concat!(
        "local::local_files_tests::atomic_write_tests::",
        "test_atomic_write_reports_injected_destination_type_error",
    );
    assert_injected_atomic_error(
        TEST_NAME,
        "atomic-destination-type",
        LocalAtomicWriteStage::ReadDestinationMetadata,
    );
}

/// Verifies retry of an injected nonblocking destination-open conflict.
#[cfg(all(coverage, target_os = "linux"))]
#[test]
fn test_atomic_write_retries_injected_destination_open_conflict() {
    const TEST_NAME: &str = concat!(
        "local::local_files_tests::atomic_write_tests::",
        "test_atomic_write_retries_injected_destination_open_conflict",
    );
    let Some((dir, destination, result)) = run_atomic_write_fault(
        TEST_NAME,
        "atomic-destination-would-block",
        true,
    ) else {
        return;
    };
    result.expect("transient destination-open conflict should be retried");
    assert_eq!(b"new", fs::read(&destination).unwrap().as_slice());
    fs::remove_dir_all(dir).expect("test directory should be removed");
}

/// Verifies normalization of an injected invalid destination resource.
#[cfg(all(coverage, target_os = "linux"))]
#[test]
fn test_atomic_write_reports_injected_invalid_destination_open() {
    const TEST_NAME: &str = concat!(
        "local::local_files_tests::atomic_write_tests::",
        "test_atomic_write_reports_injected_invalid_destination_open",
    );
    assert_injected_atomic_error(
        TEST_NAME,
        "atomic-destination-invalid",
        LocalAtomicWriteStage::ReadDestinationMetadata,
    );
}

/// Verifies propagation of an injected native destination-open failure.
#[cfg(all(coverage, target_os = "linux"))]
#[test]
fn test_atomic_write_reports_injected_native_destination_open_error() {
    const TEST_NAME: &str = concat!(
        "local::local_files_tests::atomic_write_tests::",
        "test_atomic_write_reports_injected_native_destination_open_error",
    );
    assert_injected_atomic_error(
        TEST_NAME,
        "atomic-destination-native",
        LocalAtomicWriteStage::ReadDestinationMetadata,
    );
}

/// Verifies normalization of an injected destination identity mismatch.
#[cfg(all(coverage, target_os = "linux"))]
#[test]
fn test_atomic_write_reports_injected_identity_mismatch() {
    const TEST_NAME: &str = concat!(
        "local::local_files_tests::atomic_write_tests::",
        "test_atomic_write_reports_injected_identity_mismatch",
    );
    assert_injected_atomic_error(
        TEST_NAME,
        "atomic-identity-mismatch",
        LocalAtomicWriteStage::ReplaceDestination,
    );
}

/// Verifies propagation of an injected destination identity inspection error.
#[cfg(all(coverage, target_os = "linux"))]
#[test]
fn test_atomic_write_reports_injected_identity_inspect_error() {
    const TEST_NAME: &str = concat!(
        "local::local_files_tests::atomic_write_tests::",
        "test_atomic_write_reports_injected_identity_inspect_error",
    );
    assert_injected_atomic_error(
        TEST_NAME,
        "atomic-identity-inspect",
        LocalAtomicWriteStage::ReplaceDestination,
    );
}

/// Verifies normalization of an injected missing destination identity.
#[cfg(all(coverage, target_os = "linux"))]
#[test]
fn test_atomic_write_reports_injected_identity_missing() {
    const TEST_NAME: &str = concat!(
        "local::local_files_tests::atomic_write_tests::",
        "test_atomic_write_reports_injected_identity_missing",
    );
    let Some((dir, destination, result)) =
        run_atomic_write_fault(TEST_NAME, "atomic-identity-missing", true)
    else {
        return;
    };

    let error = result.expect_err("injected missing identity should fail");
    assert_eq!(LocalAtomicWriteStage::ReplaceDestination, error.stage());
    assert_eq!(
        LocalAtomicDestinationState::Missing,
        error.destination_state()
    );
    assert_eq!(b"old", fs::read(&destination).unwrap().as_slice());
    let temporary_path = error
        .temporary_path()
        .expect("indeterminate staging path should be retained");
    assert!(temporary_path.exists());
    fs::remove_file(temporary_path)
        .expect("retained staging file should be removed");
    fs::remove_dir_all(dir).expect("test directory should be removed");
}

#[test]
fn test_atomic_write_creates_parent_directories_and_replaces_file() {
    let dir = temp_dir("atomic-replace");
    let path = dir.join("nested").join("out.txt");

    LocalFiles::atomic_write(&path, b"first")
        .expect("first atomic write should succeed");
    LocalFiles::atomic_write(&path, b"second")
        .expect("second atomic write should replace file");

    assert_eq!(b"second", fs::read(&path).unwrap().as_slice());
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn test_atomic_write_does_not_replace_concurrently_created_destination() {
    let dir = temp_dir("atomic-concurrent-create");
    let path = dir.join("out.txt");
    let mut writer = LocalFiles::begin_atomic_write(&path)
        .expect("atomic writer should begin with a missing destination");
    writer
        .write_all(b"replacement")
        .expect("replacement contents should be staged");
    fs::write(&path, b"concurrent")
        .expect("concurrent destination should be installed");

    let error = writer
        .commit()
        .expect_err("commit must not replace a concurrently created target");

    assert_eq!(LocalAtomicWriteStage::ReplaceDestination, error.stage());
    assert_eq!(
        LocalAtomicDestinationState::Unchanged,
        error.destination_state(),
    );
    assert_eq!(
        b"concurrent",
        fs::read(&path)
            .expect("concurrent destination should remain readable")
            .as_slice(),
    );
    assert_eq!(0, count_atomic_temp_files(&dir));
    fs::remove_dir_all(dir).expect("test directory should be removed");
}

#[cfg(unix)]
#[test]
fn test_atomic_write_syncs_parents_of_newly_created_directories() {
    let dir = temp_dir("atomic-sync-created-parent-chain");
    let first_created_parent = dir.join("first");
    let path = first_created_parent.join("second").join("out.txt");
    let mut permission_check_is_effective = false;

    let result = LocalFiles::atomic_write_with(&path, |writer| {
        writer.write_all(b"durable")?;
        fs::set_permissions(
            &first_created_parent,
            fs::Permissions::from_mode(0o111),
        )?;
        permission_check_is_effective = matches!(
            fs::File::open(&first_created_parent),
            Err(error) if error.kind() == ErrorKind::PermissionDenied
        );
        Ok(())
    });

    fs::set_permissions(
        &first_created_parent,
        fs::Permissions::from_mode(0o700),
    )
    .expect("created parent permissions should be restored");
    if !permission_check_is_effective {
        fs::remove_dir_all(dir).expect("test directory should be removed");
        return;
    }
    let error = result.expect_err(
        "syncing the parent of a newly created directory should be attempted",
    );

    assert_eq!(LocalAtomicWriteStage::SyncParent, error.stage());
    assert_eq!(ErrorKind::PermissionDenied, error.kind());
    assert_eq!(
        LocalAtomicDestinationState::Replaced,
        error.destination_state(),
    );
    assert_eq!(
        b"durable",
        fs::read(&path)
            .expect("committed destination should remain readable")
            .as_slice()
    );
    fs::remove_dir_all(dir).expect("test directory should be removed");
}

#[cfg(unix)]
#[test]
fn test_atomic_write_handles_lexical_parent_aliases() {
    let dir = temp_dir("atomic-lexical-parent-aliases");
    let aliased_parent = dir.join("created").join("..");
    let destination = aliased_parent.join("out.txt");

    LocalFiles::atomic_write(&destination, b"aliased")
        .expect("directory alias should resolve after its prefix is created");

    assert!(dir.join("created").is_dir());
    assert_eq!(
        b"aliased",
        fs::read(dir.join("out.txt"))
            .expect("aliased destination should be readable")
            .as_slice()
    );

    let blocker = dir.join("blocker");
    fs::write(&blocker, b"not a directory")
        .expect("blocking file should be written");
    let error = LocalFiles::atomic_write(
        dir.join("missing").join("..").join("blocker/out.txt"),
        b"blocked",
    )
    .expect_err("aliased regular-file parent should be rejected");

    assert_eq!(LocalAtomicWriteStage::PrepareParent, error.stage());
    assert_eq!(ErrorKind::AlreadyExists, error.kind());
    fs::remove_dir_all(dir).expect("test directory should be removed");
}

#[cfg(unix)]
#[test]
fn test_atomic_write_rejects_dangling_symlink_parent() {
    let dir = temp_dir("atomic-dangling-parent-symlink");
    let dangling = dir.join("dangling");
    std::os::unix::fs::symlink(dir.join("missing-target"), &dangling)
        .expect("dangling parent symlink should be created");

    let error = LocalFiles::atomic_write(dangling.join("out.txt"), b"blocked")
        .expect_err("dangling parent symlink should not become a directory");

    assert_eq!(LocalAtomicWriteStage::PrepareParent, error.stage());
    assert_eq!(ErrorKind::AlreadyExists, error.kind());
    assert!(
        fs::symlink_metadata(&dangling)
            .expect("dangling symlink should remain")
            .file_type()
            .is_symlink()
    );
    fs::remove_dir_all(dir).expect("test directory should be removed");
}

#[cfg(unix)]
#[test]
fn test_atomic_write_reports_parent_chain_creation_error() {
    let dir = temp_dir("atomic-parent-chain-creation-error");
    let restricted = dir.join("restricted");
    let probe = restricted.join("probe");
    let destination = restricted.join("missing/out.txt");
    fs::create_dir(&restricted)
        .expect("restricted directory should be created");
    fs::set_permissions(&restricted, fs::Permissions::from_mode(0o500))
        .expect("restricted directory permissions should be set");
    let probe_result = fs::create_dir(&probe);
    fs::set_permissions(&restricted, fs::Permissions::from_mode(0o700))
        .expect("restricted directory permissions should be restored");
    match probe_result {
        Err(error) if error.kind() == ErrorKind::PermissionDenied => {}
        Ok(()) => {
            fs::remove_dir_all(dir).expect("test directory should be removed");
            return;
        }
        Err(error) => panic!("permission probe should be creatable: {error}"),
    }
    fs::set_permissions(&restricted, fs::Permissions::from_mode(0o500))
        .expect("restricted directory permissions should be set");

    let result = LocalFiles::atomic_write(&destination, b"blocked");
    fs::set_permissions(&restricted, fs::Permissions::from_mode(0o700))
        .expect("restricted directory permissions should be restored");
    let error = result.expect_err("non-writable parent should reject creation");

    assert_eq!(LocalAtomicWriteStage::PrepareParent, error.stage());
    assert_eq!(ErrorKind::PermissionDenied, error.kind());
    fs::remove_dir_all(dir).expect("test directory should be removed");
}

#[cfg(windows)]
#[test]
fn test_atomic_write_rejects_windows_target_with_interior_nul() {
    let dir = temp_dir("atomic-write-windows-nul-target");
    let prefix = dir.join("existing-target");
    fs::write(&prefix, b"original").expect("prefix target should be written");
    let target = path_with_interior_nul(&dir, "existing-target");

    let error = LocalFiles::atomic_write(&target, b"replacement")
        .expect_err("Windows NUL target should be rejected before replacement");

    assert_eq!(ErrorKind::InvalidInput, error.kind());
    assert_eq!(
        LocalAtomicDestinationState::Unchanged,
        error.destination_state(),
    );
    assert_eq!(b"original", fs::read(&prefix).unwrap().as_slice());
    assert_eq!(0, count_atomic_temp_files(&dir));
    fs::remove_dir_all(dir).expect("test directory should be removed");
}

#[cfg(windows)]
#[test]
fn test_atomic_write_preserves_windows_dacl_and_stream() {
    let dir = temp_dir("atomic-windows-native-metadata");
    let path = dir.join("out.txt");
    let stream = alternate_data_stream_path(&path, "qubit-stream");
    fs::write(&path, b"old").expect("destination fixture should be written");
    fs::write(&stream, b"stream-data")
        .expect("alternate data stream should be written");
    set_world_full_control_dacl(&path)
        .expect("custom destination DACL should be applied");
    let original_dacl = read_dacl_bytes(&path)
        .expect("custom destination DACL should be readable");
    LocalFiles::atomic_write(&path, b"new")
        .expect("native Windows replacement should preserve metadata");

    assert_eq!(b"new", fs::read(&path).unwrap().as_slice());
    assert_eq!(
        original_dacl,
        read_dacl_bytes(&path).expect("committed DACL should be readable"),
    );
    assert_eq!(
        b"stream-data",
        fs::read(&stream)
            .expect("preserved alternate stream should be readable")
            .as_slice(),
    );

    fs::remove_dir_all(dir).expect("test directory should be removed");
}

#[cfg(windows)]
#[test]
fn test_atomic_write_rejects_windows_readonly_destination() {
    let dir = temp_dir("atomic-windows-readonly");
    let path = dir.join("out.txt");
    fs::write(&path, b"old").expect("destination fixture should be written");
    let mut permissions = fs::metadata(&path)
        .expect("destination metadata should be readable")
        .permissions();
    permissions.set_readonly(true);
    fs::set_permissions(&path, permissions)
        .expect("destination readonly attribute should be set");

    let error = LocalFiles::atomic_write(&path, b"new")
        .expect_err("ReplaceFileW should reject a readonly destination");

    assert_eq!(LocalAtomicWriteStage::ReplaceDestination, error.stage());
    assert_eq!(ErrorKind::PermissionDenied, error.kind());
    assert_eq!(
        LocalAtomicDestinationState::Unchanged,
        error.destination_state(),
    );
    assert_eq!(b"old", fs::read(&path).unwrap().as_slice());
    assert!(
        fs::metadata(&path)
            .expect("destination metadata should remain readable")
            .permissions()
            .readonly()
    );
    assert_eq!(0, count_atomic_temp_files(&dir));

    clear_readonly_attribute(&path)
        .expect("readonly attribute should be cleared for cleanup");
    fs::remove_dir_all(dir).expect("test directory should be removed");
}

#[cfg(windows)]
#[test]
fn test_atomic_write_ignores_windows_parent_sync_sharing_violation() {
    use std::os::windows::fs::OpenOptionsExt;

    const FILE_LIST_DIRECTORY: u32 = 0x0001;
    const FILE_SHARE_WRITE: u32 = 0x0000_0002;
    const FILE_SHARE_DELETE: u32 = 0x0000_0004;
    const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x0200_0000;
    const ERROR_SHARING_VIOLATION: i32 = 32;

    let dir = temp_dir("atomic-parent-sync-sharing-violation");
    let parent = dir.join("locked-parent");
    fs::create_dir(&parent).unwrap();

    let locked_parent = match std::fs::OpenOptions::new()
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS)
        .access_mode(FILE_LIST_DIRECTORY)
        .share_mode(FILE_SHARE_WRITE | FILE_SHARE_DELETE)
        .open(&parent)
    {
        Ok(file) => file,
        Err(error) if error.raw_os_error() == Some(ERROR_SHARING_VIOLATION) => {
            fs::remove_dir_all(dir).unwrap();
            return;
        }
        Err(error) => panic!(
            "parent directory should be locked for restricted sharing: {error}"
        ),
    };

    let path = parent.join("out.txt");
    LocalFiles::atomic_write(&path, b"data").expect(
        "atomic write should ignore unavailable Windows parent directory sync",
    );
    assert_eq!(b"data", fs::read(&path).unwrap().as_slice());

    drop(locked_parent);
    fs::remove_dir_all(dir).unwrap();
}

#[cfg(unix)]
#[test]
fn test_atomic_write_preserves_existing_file_permissions() {
    let dir = temp_dir("atomic-permissions");
    let path = dir.join("out.txt");
    fs::write(&path, b"old").unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o754)).unwrap();

    LocalFiles::atomic_write(&path, b"new")
        .expect("atomic write should preserve permissions");

    let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
    assert_eq!(0o754, mode);
    assert_eq!(b"new", fs::read(&path).unwrap().as_slice());
    fs::remove_dir_all(dir).unwrap();
}

#[cfg(any(
    target_os = "linux",
    target_os = "android",
    target_os = "macos",
    target_os = "freebsd",
))]
#[test]
fn test_atomic_write_preserves_commit_time_xattr() {
    const XATTR_NAME: &str = "user.qubit-local-files";

    let dir = temp_dir("atomic-commit-time-xattr");
    let path = dir.join("out.txt");
    fs::write(&path, b"old").expect("destination fixture should be written");
    set_user_xattr(&path, XATTR_NAME, b"initial")
        .expect("initial destination xattr should be set");
    let mut writer = LocalFiles::begin_atomic_write(&path)
        .expect("atomic writer should begin");
    set_user_xattr(&path, XATTR_NAME, b"latest")
        .expect("destination xattr should change before commit");
    writer
        .write_all(b"new")
        .expect("replacement contents should be staged");

    writer.commit().expect("atomic commit should succeed");

    assert_eq!(
        b"latest",
        get_user_xattr(&path, XATTR_NAME)
            .expect("committed xattr should be readable")
            .as_slice(),
    );
    fs::remove_dir_all(dir).expect("test directory should be removed");
}

#[cfg(target_os = "macos")]
#[test]
fn test_atomic_write_preserves_commit_time_macos_acl() {
    let dir = temp_dir("atomic-commit-time-macos-acl");
    let path = dir.join("out.txt");
    fs::write(&path, b"old").expect("destination fixture should be written");
    let mut writer = LocalFiles::begin_atomic_write(&path)
        .expect("atomic writer should begin");
    set_current_user_read_acl(&path)
        .expect("explicit macOS ACL should be installed");
    let expected_acl =
        read_macos_acl_text(&path).expect("destination ACL should be readable");
    writer
        .write_all(b"new")
        .expect("replacement contents should be staged");

    writer.commit().expect("atomic commit should succeed");

    assert_eq!(
        expected_acl,
        read_macos_acl_text(&path).expect("committed ACL should be readable"),
    );
    fs::remove_dir_all(dir).expect("test directory should be removed");
}

#[cfg(target_os = "freebsd")]
#[test]
fn test_atomic_write_preserves_commit_time_freebsd_acl() {
    let dir = temp_dir("atomic-commit-time-freebsd-acl");
    let path = dir.join("out.txt");
    fs::write(&path, b"old").expect("destination fixture should be written");
    let mut writer = LocalFiles::begin_atomic_write(&path)
        .expect("atomic writer should begin");
    let Some(acl_type) = install_supported_test_acl(&path)
        .expect("a supported FreeBSD ACL fixture should be installed")
    else {
        fs::remove_dir_all(dir).expect("test directory should be removed");
        return;
    };
    let expected_acl = read_freebsd_acl_text(&path, acl_type)
        .expect("destination ACL should be readable");
    writer
        .write_all(b"new")
        .expect("replacement contents should be staged");

    writer.commit().expect("atomic commit should succeed");

    assert_eq!(
        expected_acl,
        read_freebsd_acl_text(&path, acl_type)
            .expect("committed ACL should be readable"),
    );
    fs::remove_dir_all(dir).expect("test directory should be removed");
}

#[cfg(unix)]
#[test]
fn test_atomic_write_preserves_commit_time_mode() {
    let dir = temp_dir("atomic-commit-time-mode");
    let path = dir.join("out.txt");
    fs::write(&path, b"old").expect("destination fixture should be written");
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600))
        .expect("initial destination mode should be set");
    let mut writer = LocalFiles::begin_atomic_write(&path)
        .expect("atomic writer should begin");
    fs::set_permissions(&path, fs::Permissions::from_mode(0o640))
        .expect("destination mode should change before commit");
    writer
        .write_all(b"new")
        .expect("replacement contents should be staged");

    writer.commit().expect("atomic commit should succeed");

    assert_eq!(
        0o640,
        fs::metadata(&path)
            .expect("committed destination metadata should be readable")
            .permissions()
            .mode()
            & 0o777,
    );
    fs::remove_dir_all(dir).expect("test directory should be removed");
}

#[test]
fn test_atomic_write_supports_parentless_relative_path() {
    let _lock = CURRENT_DIR_LOCK
        .lock()
        .expect("current dir lock should be acquired");
    let dir = temp_dir("atomic-parentless");
    let _guard = CurrentDirGuard::change_to(&dir);

    LocalFiles::atomic_write("out.txt", b"data")
        .expect("parentless atomic write should succeed");

    assert_eq!(b"data", fs::read(dir.join("out.txt")).unwrap().as_slice());
    drop(_guard);
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn test_atomic_write_creates_missing_relative_parent_chain() {
    let _lock = CURRENT_DIR_LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let dir = temp_dir("atomic-missing-relative-parent-chain");
    let _guard = CurrentDirGuard::change_to(&dir);

    LocalFiles::atomic_write("first/second/out.txt", b"relative")
        .expect("relative parent chain should be created");

    assert_eq!(
        b"relative",
        fs::read("first/second/out.txt")
            .expect("relative destination should be readable")
            .as_slice()
    );
    drop(_guard);
    fs::remove_dir_all(dir).expect("test directory should be removed");
}

#[cfg(unix)]
#[test]
fn test_atomic_write_returns_parent_inspection_error() {
    let dir = temp_dir("atomic-parent-inspection-error");
    let restricted = dir.join("restricted");
    let path = restricted.join("missing").join("out.txt");
    fs::create_dir(&restricted)
        .expect("restricted directory should be created");
    fs::set_permissions(&restricted, fs::Permissions::from_mode(0o000))
        .expect("restricted directory permissions should be set");
    let probe = fs::metadata(restricted.join("missing"));
    if !matches!(
        probe,
        Err(ref error) if error.kind() == ErrorKind::PermissionDenied
    ) {
        fs::set_permissions(&restricted, fs::Permissions::from_mode(0o700))
            .expect("restricted directory permissions should be restored");
        fs::remove_dir_all(dir).expect("test directory should be removed");
        return;
    }

    let error = LocalFiles::atomic_write(&path, b"blocked")
        .expect_err("unsearchable parent should reject atomic preparation");

    fs::set_permissions(&restricted, fs::Permissions::from_mode(0o700))
        .expect("restricted directory permissions should be restored");
    fs::remove_dir_all(dir).expect("test directory should be removed");
    assert_eq!(LocalAtomicWriteStage::PrepareParent, error.stage());
    assert_eq!(ErrorKind::PermissionDenied, error.kind());
    assert_eq!(
        LocalAtomicDestinationState::Unchanged,
        error.destination_state(),
    );
}

#[test]
fn test_atomic_write_with_preserves_existing_file_and_removes_temp_on_error() {
    let dir = temp_dir("atomic-error");
    let path = dir.join("out.txt");
    fs::write(&path, b"old").expect("original target should be written");

    let error = LocalFiles::atomic_write_with(&path, |writer| {
        writer.write_all(b"new")?;
        Err(Error::other("write failed"))
    })
    .expect_err("writer error should be returned");

    assert_eq!(LocalAtomicWriteStage::WriteTemporaryFile, error.stage());
    assert_eq!(path, error.path());
    assert!(error.temporary_path().is_some());
    assert_eq!(
        LocalAtomicDestinationState::Unchanged,
        error.destination_state(),
    );
    assert_eq!(ErrorKind::Other, error.kind());
    assert_eq!(
        "write failed",
        StdError::source(&error)
            .expect("native source should be retained")
            .to_string(),
    );
    assert_eq!(
        b"old",
        fs::read(&path)
            .expect("original target should remain readable")
            .as_slice()
    );
    assert_eq!(0, count_atomic_temp_files(&dir));
    fs::remove_dir_all(dir).expect("atomic error fixture should be removed");
}

#[cfg(target_os = "linux")]
#[test]
fn test_atomic_write_with_reports_temporary_cleanup_failure() {
    let dir = temp_dir("atomic-staging-cleanup-error");
    let path = dir.join("out.txt");
    fs::write(&path, b"old").expect("original target should be written");
    if !directory_write_restrictions_are_enforced(&dir) {
        fs::remove_dir_all(dir).expect("test directory should be removed");
        return;
    }

    let restricted_dir = dir.clone();
    let error = LocalFiles::atomic_write_with(&path, move |writer| {
        writer.write_all(b"new")?;
        fs::set_permissions(
            &restricted_dir,
            fs::Permissions::from_mode(0o500),
        )?;
        Err(Error::other("write failed"))
    })
    .expect_err("write and staging cleanup should both fail");

    let temporary_path = error
        .temporary_path()
        .map(ToOwned::to_owned)
        .expect("atomic error should retain the staging path");
    let cleanup_error_kind = error
        .cleanup_error()
        .map(Error::kind)
        .expect("atomic error should retain the cleanup failure");
    let error_message = error.to_string();
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o700))
        .expect("directory permissions should be restored");
    let temporary_path_remained = temporary_path.exists();
    fs::remove_dir_all(dir).expect("test directory should be removed");

    assert_eq!(LocalAtomicWriteStage::WriteTemporaryFile, error.stage());
    assert_eq!(ErrorKind::Other, error.kind());
    assert_eq!(ErrorKind::PermissionDenied, cleanup_error_kind);
    assert!(error_message.contains(&temporary_path.display().to_string()));
    assert!(error_message.contains("staging cleanup also failed"));
    assert!(temporary_path_remained);
}

#[test]
fn test_atomic_write_with_removes_temporary_file_when_callback_panics() {
    let dir = temp_dir("atomic-write-panic");
    let path = dir.join("out.txt");
    fs::write(&path, b"old").expect("original target should be written");

    let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        let _ = LocalFiles::atomic_write_with(&path, |writer| {
            writer.write_all(b"new")?;
            panic!("intentional atomic-write callback panic");
        });
    }));

    let contents = fs::read(&path).expect("destination should remain readable");
    let temporary_file_count = count_atomic_temp_files(&dir);
    fs::remove_dir_all(dir).expect("test directory should be removed");

    assert!(panic.is_err());
    assert_eq!(b"old", contents.as_slice());
    assert_eq!(0, temporary_file_count, "staging file must be removed");
}

#[test]
fn test_atomic_write_with_uses_guarded_atomic_writer() {
    let dir = temp_dir("atomic-guarded-callback");
    let path = dir.join("out.txt");

    LocalFiles::atomic_write_with(
        &path,
        |writer: &mut qubit_local_files::LocalAtomicWriter| {
            writer.write_all(b"committed")
        },
    )
    .expect("guarded atomic callback should commit");

    assert_eq!(
        b"committed",
        fs::read(&path)
            .expect("committed destination should be readable")
            .as_slice(),
    );
    assert_eq!(0, count_atomic_temp_files(&dir));
    fs::remove_dir_all(dir).expect("atomic fixture should be removed");
}

#[cfg(unix)]
#[test]
fn test_atomic_write_rejects_symlink_without_modifying_target() {
    use std::os::unix::fs::symlink;

    let dir = temp_dir("atomic-replace-symlink");
    let target = dir.join("target.txt");
    let link = dir.join("link.txt");
    fs::write(&target, b"target").unwrap();
    symlink(&target, &link).unwrap();

    let error = LocalFiles::atomic_write(&link, b"replacement")
        .expect_err("symlink destination should be rejected");

    assert!(
        fs::symlink_metadata(&link)
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert_eq!(LocalAtomicWriteStage::InspectDestination, error.stage());
    assert_eq!(ErrorKind::InvalidInput, error.kind());
    assert_eq!(b"target", fs::read(&target).unwrap().as_slice());
    assert_eq!(0, count_atomic_temp_files(&dir));
    fs::remove_dir_all(dir).unwrap();
}

#[cfg(unix)]
#[test]
fn test_atomic_write_rejects_fifo_destination() {
    use std::os::unix::fs::FileTypeExt;

    let dir = temp_dir("atomic-fifo-destination");
    let path = dir.join("pipe");
    create_fifo(&path);

    let error = LocalFiles::atomic_write(&path, b"replacement")
        .expect_err("FIFO destination should be rejected");

    assert_eq!(LocalAtomicWriteStage::InspectDestination, error.stage());
    assert_eq!(ErrorKind::InvalidInput, error.kind());
    assert!(fs::symlink_metadata(&path).unwrap().file_type().is_fifo());
    assert_eq!(0, count_atomic_temp_files(&dir));
    fs::remove_dir_all(dir).unwrap();
}

#[cfg(unix)]
#[test]
fn test_atomic_write_rejects_unix_socket_destination() {
    use std::os::unix::fs::FileTypeExt;
    use std::os::unix::net::UnixListener;

    let dir = temp_dir("atomic-socket-destination");
    let path = dir.join("socket");
    let listener = UnixListener::bind(&path).expect("socket should be bound");

    let error = LocalFiles::atomic_write(&path, b"replacement")
        .expect_err("socket destination should be rejected");

    assert_eq!(LocalAtomicWriteStage::InspectDestination, error.stage());
    assert_eq!(ErrorKind::InvalidInput, error.kind());
    assert!(fs::symlink_metadata(&path).unwrap().file_type().is_socket());
    assert_eq!(0, count_atomic_temp_files(&dir));
    drop(listener);
    fs::remove_dir_all(dir).unwrap();
}

#[cfg(unix)]
#[test]
fn test_atomic_write_returns_temp_create_error() {
    let dir = temp_dir("atomic-temp-create-error");
    let path = dir.join("out.txt");
    fs::set_permissions(&dir, fs::Permissions::from_mode(0o500)).unwrap();

    let error = LocalFiles::atomic_write(&path, b"data")
        .expect_err("unwritable dir should fail temp creation");

    fs::set_permissions(&dir, fs::Permissions::from_mode(0o700)).unwrap();
    assert_eq!(ErrorKind::PermissionDenied, error.kind());
    assert_eq!(LocalAtomicWriteStage::CreateTemporaryFile, error.stage());
    assert_eq!(
        LocalAtomicDestinationState::Unchanged,
        error.destination_state(),
    );
    assert!(!path.exists());
    fs::remove_dir_all(dir).unwrap();
}

#[cfg(unix)]
#[test]
fn test_atomic_write_rejects_self_referential_symlink() {
    use std::os::unix::fs::symlink;

    let dir = temp_dir("atomic-metadata-error");
    let path = dir.join("loop");
    symlink(&path, &path).unwrap();

    let error = LocalFiles::atomic_write(&path, b"data")
        .expect_err("self-referential symlink should be rejected");

    assert!(
        fs::symlink_metadata(&path)
            .unwrap()
            .file_type()
            .is_symlink()
    );
    assert_eq!(LocalAtomicWriteStage::InspectDestination, error.stage());
    assert_eq!(ErrorKind::InvalidInput, error.kind());
    assert_eq!(0, count_atomic_temp_files(&dir));
    fs::remove_dir_all(dir).unwrap();
}

#[test]
fn test_atomic_write_rejects_directory_destination() {
    let dir = temp_dir("rename-error");
    let path = dir.join("target-dir");
    fs::create_dir(&path).unwrap();

    let error = LocalFiles::atomic_write(&path, b"data")
        .expect_err("directory destination should be rejected");

    assert_eq!(ErrorKind::InvalidInput, error.kind());
    assert_eq!(LocalAtomicWriteStage::InspectDestination, error.stage());
    assert_eq!(
        LocalAtomicDestinationState::Unchanged,
        error.destination_state(),
    );
    assert!(path.is_dir());
    assert_eq!(0, count_atomic_temp_files(&dir));
    fs::remove_dir_all(dir).unwrap();
}

#[cfg(unix)]
#[test]
fn test_atomic_write_returns_parent_sync_open_error_when_directory_is_not_readable()
 {
    let dir = temp_dir("atomic-parent-sync-open-error");
    let parent = dir.join("parent");
    fs::create_dir(&parent).unwrap();
    fs::set_permissions(&parent, fs::Permissions::from_mode(0o300)).unwrap();

    let result = LocalFiles::atomic_write(parent.join("out.txt"), b"data");

    fs::set_permissions(&parent, fs::Permissions::from_mode(0o700)).unwrap();
    if let Err(error) = result {
        assert_eq!(ErrorKind::PermissionDenied, error.kind());
        assert_eq!(LocalAtomicWriteStage::SyncParent, error.stage());
        assert_eq!(
            LocalAtomicDestinationState::Replaced,
            error.destination_state(),
        );
        assert_eq!(
            b"data",
            fs::read(parent.join("out.txt")).unwrap().as_slice()
        );
    }
    fs::remove_dir_all(dir).unwrap();
}
