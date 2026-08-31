// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Fuzzes rooted-authority lifecycle operations and descendant constraints.

#![no_main]

use std::fs;
use std::io::Write;
use std::path::Path;
use std::path::PathBuf;

use libfuzzer_sys::fuzz_target;
use qubit_local_files::LocalFileSystem;
use qubit_local_files::options::LocalCreateDirectoryOptions;
use qubit_local_files::options::LocalDeleteOptions;
use qubit_local_files::options::LocalTempDirectoryOptions;
use qubit_local_files::options::LocalTempFileOptions;
use qubit_local_files::options::LocalWriteMode;
use qubit_local_files::options::LocalWriteOptions;
use qubit_local_files::outcome::LocalWriterState;

const MAX_FUZZ_INPUT_LEN: usize = 256;
const MAX_OPERATIONS: usize = 16;

fuzz_target!(|data: &[u8]| {
    let data = &data[..data.len().min(MAX_FUZZ_INPUT_LEN)];
    let root = fuzz_root();
    if root.exists() {
        fs::remove_dir_all(&root).expect("stale rooted fuzz root should be removable");
    }
    fs::create_dir_all(&root).expect("fuzz root should be creatable");

    let Ok(filesystem) = LocalFileSystem::rooted(&root) else {
        fs::remove_dir_all(&root).expect("unopenable rooted fuzz root should be removable");
        return;
    };
    let scratch = filesystem
        .create_directory_with_options(
            Path::new("scratch"),
            &LocalCreateDirectoryOptions::new().with_recursive(),
        )
        .expect("rooted fuzz scratch directory should be created");
    assert!(scratch.created());
    for operation in data.chunks(2).take(MAX_OPERATIONS) {
        let opcode = operation.first().copied().unwrap_or_default() % 4;
        let selector = operation.get(1).copied().unwrap_or_default();
        match opcode {
            0 => {
                let options = LocalTempFileOptions::new()
                    .with_parent(Path::new("scratch"))
                    .with_create_parent()
                    .with_max_attempts(1 + usize::from(selector % 4));
                if let Ok(mut resource) = filesystem.create_temp_file_with_options(&options) {
                    let path = resource.path().to_path_buf();
                    resource
                        .write_all(data)
                        .expect("rooted temporary fuzz file should accept bytes");
                    if selector & 1 == 0 {
                        resource.cleanup().expect("rooted temporary cleanup should succeed");
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
                if let Ok(mut writer) = filesystem
                    .open_writer_with_options(target, &LocalWriteOptions::new(LocalWriteMode::CreateOrReplace))
                {
                    writer.write_all(data).expect("rooted fuzz writer should accept bytes");
                    let outcome = writer.commit().expect("rooted fuzz writer should commit");
                    assert_eq!(LocalWriterState::Committed, outcome.state());
                }
            }
            _ => {
                let deleted = filesystem
                    .delete_file_with_options(
                        Path::new("scratch/payload"),
                        &LocalDeleteOptions::new().with_missing_ok(),
                    )
                    .expect("rooted fuzz delete should tolerate missing payload");
                assert!(deleted.deleted() || !root.join("scratch/payload").exists());
            }
        }
    }
    fs::remove_dir_all(root).expect("rooted fuzz root should be removable");
});

fn fuzz_root() -> PathBuf {
    std::env::temp_dir().join(format!("qubit-local-files-rooted-fuzz-{}", std::process::id()))
}
