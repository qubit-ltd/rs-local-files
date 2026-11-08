// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
use std::fs;
use std::path::Path;

use qubit_io::Output;
use qubit_local_files::LocalFileSystem;
#[cfg(windows)]
use qubit_local_files::error::LocalFileErrorKind;
use qubit_local_files::options::LocalWriteMode;
use qubit_local_files::options::LocalWriteOptions;
use qubit_local_files::policy::LocalDurabilityRequirement;
use qubit_local_files::policy::LocalFileSyncRequirement;
use tempfile::tempdir;

#[test]
fn test_file_sync_requirement_is_independent() {
    let fixture = tempdir().expect("temporary directory should be created");
    let fs = LocalFileSystem::host().expect("host filesystem should open");
    let target = fixture.path().join("sync");
    let options = LocalWriteOptions::new(LocalWriteMode::CreateNew)
        .with_file_sync(LocalFileSyncRequirement::Required)
        .with_durability(LocalDurabilityRequirement::NotRequired);
    let mut writer = fs
        .open_writer_with_options(&target, &options)
        .expect("writer should open");
    writer.write_fully(b"synced").expect("bytes should be written");
    let outcome = writer.commit().expect("writer should commit");
    assert!(outcome.file_synced());
    assert!(!outcome.durable());
    assert_eq!(fs::read(target).expect("target should be readable"), b"synced");
}

#[test]
fn test_file_sync_requirement_reports_sync_for_all_host_modes() {
    for mode in [
        LocalWriteMode::CreateNew,
        LocalWriteMode::CreateOrReplace,
        LocalWriteMode::Append,
    ] {
        let fixture = tempdir().expect("temporary directory should be created");
        let fs = LocalFileSystem::host().expect("host filesystem should open");
        let target = fixture.path().join("sync");
        if mode != LocalWriteMode::CreateNew {
            fs::write(&target, b"prefix").expect("existing destination should be created");
        }
        let options = LocalWriteOptions::new(mode)
            .with_file_sync(LocalFileSyncRequirement::Required)
            .with_durability(LocalDurabilityRequirement::NotRequired);
        let mut writer = fs
            .open_writer_with_options(&target, &options)
            .expect("writer should open");
        writer.write_fully(b"data").expect("bytes should be written");
        let outcome = writer.commit().expect("writer should commit");
        assert!(outcome.file_synced(), "{mode:?} should report file synchronization");
        assert!(
            !outcome.durable(),
            "independent file sync must not imply namespace durability"
        );
    }
}

#[test]
fn test_file_sync_requirement_reports_sync_for_all_rooted_modes() {
    for mode in [
        LocalWriteMode::CreateNew,
        LocalWriteMode::CreateOrReplace,
        LocalWriteMode::Append,
    ] {
        let fixture = tempdir().expect("temporary directory should be created");
        let fs = LocalFileSystem::rooted(fixture.path()).expect("rooted filesystem should open");
        let target = Path::new("sync");
        if mode != LocalWriteMode::CreateNew {
            fs::write(fixture.path().join(target), b"prefix").expect("existing destination should be created");
        }
        let options = LocalWriteOptions::new(mode)
            .with_file_sync(LocalFileSyncRequirement::Required)
            .with_durability(LocalDurabilityRequirement::NotRequired);
        let mut writer = fs
            .open_writer_with_options(target, &options)
            .expect("writer should open");
        writer.write_fully(b"data").expect("bytes should be written");
        let outcome = writer.commit().expect("writer should commit");
        assert!(outcome.file_synced(), "{mode:?} should report file synchronization");
        assert!(
            !outcome.durable(),
            "independent file sync must not imply namespace durability"
        );
    }
}

#[cfg(unix)]
#[test]
fn test_required_durability_reports_file_sync() {
    let fixture = tempdir().expect("temporary directory should be created");
    let fs = LocalFileSystem::host().expect("host filesystem should open");
    let target = fixture.path().join("sync");
    let options =
        LocalWriteOptions::new(LocalWriteMode::CreateNew).with_durability(LocalDurabilityRequirement::Required);
    let mut writer = fs
        .open_writer_with_options(&target, &options)
        .expect("writer should open");
    writer.write_fully(b"data").expect("bytes should be written");
    let outcome = writer.commit().expect("writer should commit");
    assert!(outcome.file_synced());
    assert!(outcome.durable());
}

#[cfg(windows)]
#[test]
fn test_required_durability_is_rejected_before_open() {
    let fixture = tempdir().expect("temporary directory should be created");
    let fs = LocalFileSystem::host().expect("host filesystem should open");
    let target = fixture.path().join("sync");
    let options =
        LocalWriteOptions::new(LocalWriteMode::CreateNew).with_durability(LocalDurabilityRequirement::Required);
    let error = fs
        .open_writer_with_options(&target, &options)
        .expect_err("unsupported required durability should be rejected before opening");

    assert_eq!(error.kind(), LocalFileErrorKind::RequirementNotMet);
    assert!(!target.exists(), "preflight rejection must not create the target");
}

#[cfg(unix)]
#[test]
fn test_rooted_required_durability_reports_file_sync() {
    let fixture = tempdir().expect("temporary directory should be created");
    let fs = LocalFileSystem::rooted(fixture.path()).expect("rooted filesystem should open");
    let target = Path::new("sync");
    let options =
        LocalWriteOptions::new(LocalWriteMode::CreateNew).with_durability(LocalDurabilityRequirement::Required);
    let mut writer = fs
        .open_writer_with_options(target, &options)
        .expect("writer should open");
    writer.write_fully(b"data").expect("bytes should be written");
    let outcome = writer.commit().expect("writer should commit");
    assert!(outcome.file_synced());
    assert!(outcome.durable());
}
