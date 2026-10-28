// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
#![cfg(all(feature = "test-support", unix))]

use std::error::Error;
use std::fs;
use std::io::ErrorKind;
use std::path::Path;

use qubit_local_files::LocalFileSystem;
use qubit_local_files::error::LocalFileErrorKind;
use qubit_local_files::options::LocalCopyConflictPolicy;
use qubit_local_files::options::LocalCopyOptions;
use qubit_local_files::options::LocalCopyTypeConflictPolicy;
use qubit_local_files::outcome::LocalCopyFailure;
use qubit_local_files::outcome::LocalCopyFailureState;
use qubit_local_files::policy::LocalDurabilityRequirement;
use qubit_local_files::test_support::install_test_fault;
use tempfile::tempdir;

fn io_cause(failure: &LocalCopyFailure) -> &std::io::Error {
    let mut source = failure.error().source();
    let mut io_cause = None;
    while let Some(error) = source {
        if let Some(error) = error.downcast_ref::<std::io::Error>() {
            io_cause = Some(error);
        }
        source = error.source();
    }
    io_cause.expect("native I/O cause should be retained")
}

fn raw_os_error(error: &std::io::Error) -> Option<i32> {
    error
        .raw_os_error()
        .or_else(|| error.get_ref()?.downcast_ref::<std::io::Error>().and_then(raw_os_error))
}

#[test]
fn test_rooted_file_copy_retains_installed_state_after_parent_sync_failure() {
    let directory = tempdir().expect("temporary root should be created");
    fs::write(directory.path().join("source"), b"payload").expect("source should be written");
    fs::write(directory.path().join("target"), b"old").expect("target should be written");
    let filesystem = LocalFileSystem::rooted(directory.path()).expect("rooted filesystem should open");
    let _fault = install_test_fault("rooted-copy-writer-parent-sync-after-install")
        .expect("writer parent-sync fault should install");

    let failure = filesystem
        .copy_with_options(
            Path::new("source"),
            Path::new("target"),
            &LocalCopyOptions::default()
                .with_conflict(LocalCopyConflictPolicy::Overwrite)
                .with_durability(LocalDurabilityRequirement::Required),
        )
        .expect_err("the installed file must retain the namespace sync failure");

    assert_eq!(LocalCopyFailureState::Published, failure.state());
    assert_eq!(1, failure.partial_stats().files());
    assert_eq!(7, failure.partial_stats().bytes());
    assert_eq!(1, failure.partial_stats().overwritten());
    assert_eq!(
        b"payload",
        fs::read(directory.path().join("target"))
            .expect("target should be readable")
            .as_slice()
    );
    assert_eq!(Some(Path::new("/source")), failure.failed_source_path());
    assert_eq!(Some(Path::new("/target")), failure.failed_target_path());
    assert_eq!(LocalFileErrorKind::Io, failure.error().kind());
    assert_eq!(Some(libc::EIO), raw_os_error(io_cause(&failure)));
}

#[test]
fn test_rooted_removed_conflict_keeps_published_fact_after_sync_failure() {
    let directory = tempdir().expect("temporary root should be created");
    fs::write(directory.path().join("source"), b"payload").expect("source should be written");
    fs::create_dir_all(directory.path().join("target/child")).expect("conflicting tree should be created");
    fs::write(directory.path().join("target/child/old"), b"old").expect("conflicting target child should be written");
    let filesystem = LocalFileSystem::rooted(directory.path()).expect("rooted filesystem should open");
    let _fault = install_test_fault("rooted-copy-writer-parent-sync-after-install")
        .expect("writer parent-sync fault should install");

    let failure = filesystem
        .copy_with_options(
            Path::new("source"),
            Path::new("target"),
            &LocalCopyOptions::default()
                .with_conflict(LocalCopyConflictPolicy::Overwrite)
                .with_type_conflict(LocalCopyTypeConflictPolicy::Replace)
                .with_durability(LocalDurabilityRequirement::Required),
        )
        .expect_err("sync failure after replacement should be reported");

    assert_eq!(LocalCopyFailureState::Published, failure.state());
    assert_eq!(1, failure.partial_stats().files());
    assert_eq!(1, failure.partial_stats().overwritten());
    assert_eq!(
        b"payload",
        fs::read(directory.path().join("target"))
            .expect("target should be readable")
            .as_slice()
    );
}

#[test]
fn test_rooted_preinstall_and_install_failures_do_not_claim_publication() {
    for (fault, expected_state, expected_error_kind, expected_cause_kind, expected_raw_os_error) in [
        (
            "rooted-copy-writer-open",
            LocalCopyFailureState::Unchanged,
            LocalFileErrorKind::PermissionDenied,
            Some(ErrorKind::PermissionDenied),
            None,
        ),
        (
            "rooted-copy-file-commit-native",
            LocalCopyFailureState::Indeterminate,
            LocalFileErrorKind::Io,
            None,
            Some(libc::EIO),
        ),
    ] {
        let directory = tempdir().expect("temporary root should be created");
        fs::write(directory.path().join("source"), b"payload").expect("source should be written");
        fs::write(directory.path().join("target"), b"old").expect("target should be written");
        let filesystem = LocalFileSystem::rooted(directory.path()).expect("rooted filesystem should open");
        let _fault = install_test_fault(fault).expect("writer fault should install");

        let failure = filesystem
            .copy_with_options(
                Path::new("source"),
                Path::new("target"),
                &LocalCopyOptions::default()
                    .with_conflict(LocalCopyConflictPolicy::Overwrite)
                    .with_durability(LocalDurabilityRequirement::Required),
            )
            .expect_err("injected writer failure should be reported");

        assert_eq!(expected_state, failure.state());
        assert_eq!(expected_error_kind, failure.error().kind());
        assert_eq!(0, failure.partial_stats().files());
        assert_eq!(0, failure.partial_stats().bytes());
        if let Some(expected_cause_kind) = expected_cause_kind {
            assert_eq!(expected_cause_kind, io_cause(&failure).kind());
        }
        assert_eq!(expected_raw_os_error, raw_os_error(io_cause(&failure)));
        assert_eq!(
            b"old",
            fs::read(directory.path().join("target"))
                .expect("target should remain readable")
                .as_slice()
        );
    }
}
