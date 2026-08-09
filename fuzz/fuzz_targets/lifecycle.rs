// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Fuzzes temporary-resource lifecycle transitions and cleanup invariants.

#![no_main]

use std::fs;
use std::path::PathBuf;

use libfuzzer_sys::fuzz_target;
use qubit_local_files::LocalFileSystem;
use qubit_local_files::LocalListOptions;
use qubit_local_files::LocalTempDirectoryOptions;
use qubit_local_files::LocalTempFileOptions;

const MAX_FUZZ_INPUT_LEN: usize = 256;
const MAX_OPERATIONS: usize = 16;

fuzz_target!(|data: &[u8]| {
    let data = &data[..data.len().min(MAX_FUZZ_INPUT_LEN)];
    let root = fuzz_root();
    if root.exists() {
        fs::remove_dir_all(&root).expect("stale fuzz root should be removable");
    }
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
                    std::io::Write::write_all(&mut resource, data)
                        .expect("temporary fuzz file should accept bytes");
                    if selector & 1 == 0 {
                        let _outcome = resource
                            .cleanup()
                            .expect("temporary file cleanup should succeed");
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
                        fs::write(path.join("payload"), data).expect(
                            "temporary directory payload should be writable",
                        );
                    }
                    drop(resource);
                    assert!(!path.exists());
                }
            }
            2 => {
                let target = root.join("persisted");
                let options = LocalTempFileOptions::new().with_parent(&root);
                if let Ok(mut resource) = native.create_temp_file(&options) {
                    std::io::Write::write_all(&mut resource, data)
                        .expect("persist source should accept bytes");
                    let _outcome = resource.persist(&target).expect(
                        "temporary file should persist to an absent target",
                    );
                    assert_eq!(
                        data,
                        fs::read(&target)
                            .expect("persisted bytes should be readable")
                    );
                    fs::remove_file(target)
                        .expect("persisted target should be removable");
                }
            }
            3 => {
                let options =
                    LocalTempDirectoryOptions::new().with_parent(&root);
                if let Ok(resource) = native.create_temp_directory(&options) {
                    let child = resource.path().join("nested");
                    fs::create_dir(&child)
                        .expect("nested fuzz directory should be created");
                    fs::write(child.join("payload"), data)
                        .expect("nested fuzz payload should be writable");
                    let list_options = LocalListOptions::new().with_recursive();
                    let entries = native
                        .list(resource.path(), &list_options)
                        .map(|walker| walker.collect::<Vec<_>>());
                    assert!(
                        entries.is_ok(),
                        "temporary directory listing should succeed"
                    );
                }
            }
            _ => {
                let entries = native.list(&root, &LocalListOptions::new());
                assert!(entries.is_ok(), "fuzz root listing should open");
            }
        }
    }
    fs::remove_dir_all(root).expect("fuzz root should be removable");
});

fn fuzz_root() -> PathBuf {
    std::env::temp_dir()
        .join(format!("qubit-local-files-fuzz-{}", std::process::id()))
}
