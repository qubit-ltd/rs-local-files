// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

//! Static options must be rejected transactionally before filesystem work.

use std::path::Path;
use std::time::Duration;

use qubit_local_files::LocalFileSystem;
use qubit_local_files::error::LocalFileErrorKind;
use qubit_local_files::error::LocalFileOperation;
use qubit_local_files::options::LocalCopyOptions;
use qubit_local_files::options::LocalListOptions;
use qubit_local_files::options::LocalTempDirectoryOptions;
use qubit_local_files::options::LocalTempFileOptions;
use qubit_local_files::options::LocalWriteMode;
use qubit_local_files::options::LocalWriteOptions;
use qubit_local_files::policy::LocalAtomicityRequirement;
use qubit_local_files::policy::LocalDurabilityRequirement;
use tempfile::tempdir;

/// Exercises both scopes without changing process-wide state.
#[test]
fn test_configuration_rejects_static_requirements_transactionally() {
    let directory = tempdir().expect("fixture should exist");
    for mut filesystem in [
        LocalFileSystem::host().expect("Host should open"),
        LocalFileSystem::rooted(directory.path()).expect("Rooted should open"),
    ] {
        let previous = *filesystem.default_write_options();
        let invalid =
            LocalWriteOptions::new(LocalWriteMode::Append).with_atomicity(LocalAtomicityRequirement::Required);
        let error = filesystem
            .set_default_write_options(invalid)
            .expect_err("append cannot be atomic");
        assert_eq!(error.kind(), LocalFileErrorKind::RequirementNotMet);
        assert_eq!(error.operation(), LocalFileOperation::Configure);
        assert_eq!(*filesystem.default_write_options(), previous);
        let error = filesystem
            .open_writer_with_options(Path::new("missing/entry"), &invalid)
            .expect_err("static requirement must win before native lookup");
        assert_eq!(error.kind(), LocalFileErrorKind::RequirementNotMet);
        assert_eq!(error.operation(), LocalFileOperation::OpenWriter);

        let previous = *filesystem.default_copy_options();
        for invalid in [
            LocalCopyOptions::new()
                .with_tree_source()
                .with_atomicity(LocalAtomicityRequirement::Required),
            LocalCopyOptions::new()
                .with_tree_source()
                .with_durability(LocalDurabilityRequirement::Required),
        ] {
            let error = filesystem
                .set_default_copy_options(invalid)
                .expect_err("tree guarantee is unavailable");
            assert_eq!(error.kind(), LocalFileErrorKind::RequirementNotMet);
            assert_eq!(error.operation(), LocalFileOperation::Configure);
            assert_eq!(*filesystem.default_copy_options(), previous);
            let failure = filesystem
                .copy_with_options(Path::new("missing/source"), Path::new("target"), &invalid)
                .expect_err("static tree guarantee must win before lookup");
            assert_eq!(failure.error().kind(), LocalFileErrorKind::RequirementNotMet);
        }
    }
}

/// Invalid affixes must not replace defaults or create a parent directory.
#[test]
fn test_configuration_rejects_temporary_affixes_before_io() {
    let directory = tempdir().expect("fixture should exist");
    for mut filesystem in [
        LocalFileSystem::host().expect("Host should open"),
        LocalFileSystem::rooted(directory.path()).expect("Rooted should open"),
    ] {
        for affix in ["bad/name", "bad\\name", "bad\0name"] {
            let previous = filesystem.default_temp_file_options().clone();
            let options = LocalTempFileOptions::new().with_prefix(affix);
            let error = filesystem
                .set_default_temp_file_options(options.clone())
                .expect_err("invalid prefix");
            assert_eq!(error.kind(), LocalFileErrorKind::InvalidOptions);
            assert_eq!(error.operation(), LocalFileOperation::Configure);
            assert_eq!(filesystem.default_temp_file_options(), &previous);
            let error = filesystem
                .create_temp_file_with_options(&options)
                .expect_err("invalid prefix");
            assert_eq!(error.kind(), LocalFileErrorKind::InvalidOptions);
            let previous = filesystem.default_temp_directory_options().clone();
            let options = LocalTempDirectoryOptions::new().with_suffix(affix);
            let error = filesystem
                .set_default_temp_directory_options(options.clone())
                .expect_err("invalid suffix");
            assert_eq!(error.kind(), LocalFileErrorKind::InvalidOptions);
            assert_eq!(error.operation(), LocalFileOperation::Configure);
            assert_eq!(filesystem.default_temp_directory_options(), &previous);
            let error = filesystem
                .create_temp_directory_with_options(&options)
                .expect_err("invalid suffix");
            assert_eq!(error.kind(), LocalFileErrorKind::InvalidOptions);
        }
    }
}

/// Distinguishes an invalid listing configuration from a valid exhausted copy
/// budget.
#[test]
fn test_configuration_budget_boundaries() {
    let mut filesystem = LocalFileSystem::host().expect("Host should open");
    let previous = *filesystem.default_list_options();
    let error = filesystem
        .set_default_list_options(LocalListOptions::new().with_max_open_directories(0))
        .expect_err("listing requires a nonzero open-directory bound");
    assert_eq!(error.kind(), LocalFileErrorKind::InvalidOptions);
    assert_eq!(*filesystem.default_list_options(), previous);
    let error = filesystem
        .set_default_list_options(LocalListOptions::new().with_deadline(Duration::MAX))
        .expect_err("listing deadline must fit the monotonic clock");
    assert_eq!(error.kind(), LocalFileErrorKind::InvalidOptions);
    assert_eq!(error.operation(), LocalFileOperation::Configure);
    assert_eq!(*filesystem.default_list_options(), previous);
    filesystem
        .set_default_copy_options(LocalCopyOptions::new().with_max_open_directories(0))
        .expect("zero copy budget is a valid runtime limit");
    let previous = *filesystem.default_copy_options();
    let error = filesystem
        .set_default_copy_options(LocalCopyOptions::new().with_deadline(Duration::MAX))
        .expect_err("unrepresentable deadline should be rejected");
    assert_eq!(error.kind(), LocalFileErrorKind::InvalidOptions);
    assert_eq!(*filesystem.default_copy_options(), previous);
}
