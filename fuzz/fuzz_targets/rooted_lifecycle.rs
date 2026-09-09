// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Fuzzes rooted-authority lifecycle operations and descendant constraints.

#![no_main]

use std::io::Write;
use std::path::Path;

use libfuzzer_sys::fuzz_target;
use qubit_local_files::LocalFileSystem;
use qubit_local_files::options::LocalCreateDirectoryOptions;
use qubit_local_files::options::LocalDeleteOptions;
use qubit_local_files::options::LocalPersistOptions;
use qubit_local_files::options::LocalTempDirectoryOptions;
use qubit_local_files::options::LocalTempFileOptions;
use qubit_local_files::options::LocalWriteMetadataPolicy;
use qubit_local_files::options::LocalWriteMode;
use qubit_local_files::options::LocalWriteOptions;
use qubit_local_files::outcome::LocalPersistStage;
use qubit_local_files::outcome::LocalWriterState;

mod support;

use support::FuzzRoot;

const MAX_FUZZ_INPUT_LEN: usize = 256;
const MAX_OPERATIONS: usize = 16;

fuzz_target!(|data: &[u8]| {
    let data = &data[..data.len().min(MAX_FUZZ_INPUT_LEN)];
    let Some(root) = FuzzRoot::create("rooted-lifecycle-fuzz") else {
        return;
    };

    let Ok(filesystem) = LocalFileSystem::rooted(root.path()) else {
        return;
    };
    let Ok(scratch) = filesystem.create_directory_with_options(
        Path::new("scratch"),
        &LocalCreateDirectoryOptions::new().with_recursive(),
    ) else {
        return;
    };
    assert!(scratch.created());
    for operation in data.chunks(2).take(MAX_OPERATIONS) {
        let opcode = operation.first().copied().unwrap_or_default() % 6;
        let selector = operation.get(1).copied().unwrap_or_default();
        match opcode {
            0 => {
                let options = LocalTempFileOptions::new()
                    .with_parent(Path::new("scratch"))
                    .with_create_parent()
                    .with_max_attempts(1 + usize::from(selector % 4));
                if let Ok(mut resource) = filesystem.create_temp_file_with_options(&options) {
                    let path = resource.path().to_path_buf();
                    if resource.write_all(data).is_err() {
                        let _ = resource.cleanup();
                        continue;
                    }
                    if selector & 1 == 0 {
                        assert!(resource.cleanup().is_ok());
                    }
                    drop(resource);
                    assert!(filesystem.metadata(&path).is_err());
                }
            }
            1 => {
                let options = LocalTempDirectoryOptions::new()
                    .with_parent(Path::new("scratch"))
                    .with_create_parent()
                    .with_max_attempts(1 + usize::from(selector % 4));
                if let Ok(resource) = filesystem.create_temp_directory_with_options(&options) {
                    let path = resource.path().to_path_buf();
                    drop(resource);
                    assert!(filesystem.metadata(&path).is_err());
                }
            }
            2 => {
                let target = Path::new("scratch/payload");
                let policy = if selector & 1 == 0 {
                    LocalWriteMetadataPolicy::PreserveExisting
                } else {
                    LocalWriteMetadataPolicy::UseStaging
                };
                if let Ok(mut writer) = filesystem.open_writer_with_options(
                    target,
                    &LocalWriteOptions::new(LocalWriteMode::CreateOrReplace).with_metadata_policy(policy),
                ) {
                    if writer.write_all(data).is_err() {
                        continue;
                    }
                    let outcome = writer.commit().expect("rooted fuzz writer should commit");
                    assert_eq!(LocalWriterState::Committed, outcome.state());
                }
            }
            3 => {
                let deleted = filesystem
                    .delete_file_with_options(
                        Path::new("scratch/payload"),
                        &LocalDeleteOptions::new().with_missing_ok(),
                    )
                    .expect("rooted fuzz delete should tolerate missing payload");
                assert!(deleted.deleted() || !root.path().join("scratch/payload").exists());
            }
            4 => {
                let (base, target, valid) = persistence_operands(selector);
                let Ok(mut resource) = filesystem.create_temp_file() else {
                    continue;
                };
                if resource.write_all(data).is_err() {
                    let _ = resource.cleanup();
                    continue;
                }
                match resource.persist_at(base, target, LocalPersistOptions::new()) {
                    Ok(outcome) => {
                        assert!(valid);
                        let _ = filesystem
                            .delete_file(outcome.path())
                            .expect("remove published fuzz file");
                    }
                    Err(mut error) => {
                        assert!(!valid, "valid explicit-base file publication failed: {error}");
                        assert_eq!(error.stage(), LocalPersistStage::ResolveTarget);
                        error
                            .resource_mut()
                            .write_all(b"retained")
                            .expect("invalid parameters keep file open");
                        error.resource_mut().cleanup().expect("clean retained fuzz file");
                    }
                }
            }
            _ => {
                let (base, target, valid) = persistence_operands(selector);
                let Ok(resource) = filesystem.create_temp_directory() else {
                    continue;
                };
                match resource.persist_at(base, target, LocalPersistOptions::new()) {
                    Ok(outcome) => {
                        assert!(valid);
                        let _ = filesystem
                            .delete_directory(outcome.path())
                            .expect("remove published fuzz directory");
                    }
                    Err(mut error) => {
                        assert!(!valid, "valid explicit-base directory publication failed: {error}");
                        assert_eq!(error.stage(), LocalPersistStage::ResolveTarget);
                        error.resource_mut().cleanup().expect("clean retained fuzz directory");
                    }
                }
            }
        }
    }
});

/// Keeps valid and rejected operands, including virtual-root escapes, in the
/// corpus space.
fn persistence_operands(selector: u8) -> (&'static Path, &'static Path, bool) {
    let (base, target, valid) = match selector % 8 {
        0 => ("/scratch", "published", true),
        1 => ("/scratch", "../published", true),
        2 => ("/scratch", "../../outside", false),
        3 => ("scratch", "published", false),
        4 => ("/scratch/.", "published", false),
        5 => ("/scratch", "/absolute", false),
        6 => ("/missing", "published", false),
        _ => ("/scratch", "", false),
    };
    (Path::new(base), Path::new(target), valid)
}
