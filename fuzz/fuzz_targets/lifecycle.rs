// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
// =============================================================================
//! Fuzzes temporary-resource lifecycle transitions and cleanup invariants.

#![no_main]

use std::{
    fs,
    path::PathBuf,
};

use libfuzzer_sys::fuzz_target;
use qubit_local_files::{
    LocalFileSystem,
    LocalListOptions,
    LocalTempDirectoryOptions,
    LocalTempFileOptions,
};

const MAX_FUZZ_INPUT_LEN: usize = 256;
const MAX_OPERATIONS: usize = 16;

fuzz_target!(|data: &[u8]| {
    let data = &data[..data.len().min(MAX_FUZZ_INPUT_LEN)];
    let root = fuzz_root();
    let _ = fs::remove_dir_all(&root);
    fs::create_dir_all(&root).expect("fuzz root should be creatable");

    let native = LocalFileSystem::host();
    for operation in data.chunks(2).take(MAX_OPERATIONS) {
        let opcode = operation.first().copied().unwrap_or_default() % 5;
        let selector = operation.get(1).copied().unwrap_or_default();
        match opcode {
            0 => {
                let options = LocalTempFileOptions::new()
                    .with_parent(&root)
                    .with_prefix("fuzz-")
                    .with_max_attempts(1 + usize::from(selector % 4));
                if let Ok(mut resource) = native.create_temp_file(&options) {
                    let path = resource.path().to_path_buf();
                    let _ = std::io::Write::write_all(&mut resource, data);
                    if selector & 1 == 0 {
                        let _ = resource.cleanup();
                    }
                    drop(resource);
                    assert!(!path.exists());
                }
            }
            1 => {
                let options = LocalTempDirectoryOptions::new()
                    .with_parent(&root)
                    .with_prefix("fuzz-")
                    .with_max_attempts(1 + usize::from(selector % 4));
                if let Ok(resource) = native.create_temp_directory(&options) {
                    let path = resource.path().to_path_buf();
                    if selector & 1 == 0 {
                        let _ = fs::write(path.join("payload"), data);
                    }
                    drop(resource);
                    assert!(!path.exists());
                }
            }
            2 => {
                let target = root.join("persisted");
                let options = LocalTempFileOptions::new().with_parent(&root);
                if let Ok(mut resource) = native.create_temp_file(&options) {
                    let _ = std::io::Write::write_all(&mut resource, data);
                    let _ = resource.persist(&target);
                    let _ = fs::remove_file(target);
                }
            }
            3 => {
                let options =
                    LocalTempDirectoryOptions::new().with_parent(&root);
                if let Ok(resource) = native.create_temp_directory(&options) {
                    let child = resource.path().join("nested");
                    let _ = fs::create_dir(&child);
                    let _ = fs::write(child.join("payload"), data);
                    let list_options = LocalListOptions::new().with_recursive();
                    let _ = native
                        .list(resource.path(), &list_options)
                        .map(|walker| walker.collect::<Vec<_>>());
                }
            }
            _ => {
                let _ = native.list(&root, &LocalListOptions::new());
            }
        }
    }
    let _ = fs::remove_dir_all(root);
});

fn fuzz_root() -> PathBuf {
    std::env::temp_dir()
        .join(format!("qubit-local-files-fuzz-{}", std::process::id()))
}
