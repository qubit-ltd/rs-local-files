// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Fuzzes rooted-authority lifecycle operations and descendant constraints.

#![no_main]

use std::{
    fs,
    io::Write,
    path::{
        Path,
        PathBuf,
    },
};

use libfuzzer_sys::fuzz_target;
use qubit_local_files::{
    LocalCreateDirectoryOptions,
    LocalDeleteOptions,
    LocalFileSystem,
    LocalTempDirectoryOptions,
    LocalTempFileOptions,
    LocalWriteMode,
    LocalWriteOptions,
};

const MAX_FUZZ_INPUT_LEN: usize = 256;
const MAX_OPERATIONS: usize = 16;

fuzz_target!(|data: &[u8]| {
    let data = &data[..data.len().min(MAX_FUZZ_INPUT_LEN)];
    let root = fuzz_root();
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).expect("fuzz root should be creatable");

    let Ok(filesystem) = LocalFileSystem::rooted(&root) else {
        let _ = fs::remove_dir_all(&root);
        return;
    };
    let _ = filesystem.create_directory(
        Path::new("scratch"),
        &LocalCreateDirectoryOptions::new().with_recursive(),
    );
    for operation in data.chunks(2).take(MAX_OPERATIONS) {
        let opcode = operation.first().copied().unwrap_or_default() % 4;
        let selector = operation.get(1).copied().unwrap_or_default();
        match opcode {
            0 => {
                let options = LocalTempFileOptions::new()
                    .with_parent(Path::new("scratch"))
                    .with_create_parent()
                    .with_max_attempts(1 + usize::from(selector % 4));
                if let Ok(mut resource) = filesystem.create_temp_file(&options)
                {
                    let path = resource.path().to_path_buf();
                    let _ = resource.write_all(data);
                    if selector & 1 == 0 {
                        let _ = resource.cleanup();
                    }
                    drop(resource);
                    assert!(!root.join(path).exists());
                }
            }
            1 => {
                let options = LocalTempDirectoryOptions::new()
                    .with_parent(Path::new("scratch"))
                    .with_create_parent()
                    .with_max_attempts(1 + usize::from(selector % 4));
                if let Ok(resource) = filesystem.create_temp_directory(&options)
                {
                    let path = resource.path().to_path_buf();
                    drop(resource);
                    assert!(!root.join(path).exists());
                }
            }
            2 => {
                let target = Path::new("scratch/payload");
                if let Ok(mut writer) = filesystem.open_writer(
                    target,
                    &LocalWriteOptions::new(LocalWriteMode::CreateOrReplace),
                ) {
                    let _ = writer.write_all(data);
                    let _ = writer.commit();
                }
            }
            _ => {
                let _ = filesystem.delete_file(
                    Path::new("scratch/payload"),
                    &LocalDeleteOptions::new().with_missing_ok(),
                );
            }
        }
    }
    let _ = fs::remove_dir_all(root);
});

fn fuzz_root() -> PathBuf {
    std::env::temp_dir().join(format!(
        "qubit-local-files-rooted-fuzz-{}",
        std::process::id()
    ))
}
