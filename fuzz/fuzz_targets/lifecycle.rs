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

use libfuzzer_sys::fuzz_target;
use qubit_local_files::LocalFileSystem;
use qubit_local_files::options::LocalListOptions;
use qubit_local_files::options::LocalTempDirectoryOptions;
use qubit_local_files::options::LocalTempFileOptions;

mod support;

use support::FuzzRoot;

const MAX_FUZZ_INPUT_LEN: usize = 256;
const MAX_OPERATIONS: usize = 16;

fuzz_target!(|data: &[u8]| {
    let data = &data[..data.len().min(MAX_FUZZ_INPUT_LEN)];
    let Some(root) = FuzzRoot::create("lifecycle-fuzz") else {
        return;
    };
    let root_path = root.path();

    let Ok(native) = LocalFileSystem::host() else {
        return;
    };
    for operation in data.chunks(2).take(MAX_OPERATIONS) {
        let opcode = operation.first().copied().unwrap_or_default() % 5;
        let selector = operation.get(1).copied().unwrap_or_default();
        match opcode {
            0 => {
                let options = LocalTempFileOptions::new()
                    .with_parent(root_path)
                    .with_prefix("fuzz-")
                    .with_max_attempts(1 + usize::from(selector % 4));
                if let Ok(mut resource) = native.create_temp_file_with_options(&options) {
                    let path = resource.path().to_path_buf();
                    if std::io::Write::write_all(&mut resource, data).is_err() {
                        let _ = resource.cleanup();
                        continue;
                    }
                    if selector & 1 == 0 {
                        assert!(resource.cleanup().is_ok());
                    }
                    drop(resource);
                    assert!(!path.exists());
                }
            }
            1 => {
                let options = LocalTempDirectoryOptions::new()
                    .with_parent(root_path)
                    .with_prefix("fuzz-")
                    .with_max_attempts(1 + usize::from(selector % 4));
                if let Ok(resource) = native.create_temp_directory_with_options(&options) {
                    let path = resource.path().to_path_buf();
                    if selector & 1 == 0 {
                        if fs::write(path.join("payload"), data).is_err() {
                            continue;
                        }
                    }
                    drop(resource);
                    assert!(!path.exists());
                }
            }
            2 => {
                let target = root_path.join("persisted");
                let options = LocalTempFileOptions::new()
                    .with_parent(root_path)
                    .with_max_attempts(1 + usize::from(selector % 4));
                if let Ok(mut resource) = native.create_temp_file_with_options(&options) {
                    if std::io::Write::write_all(&mut resource, data).is_err() {
                        let _ = resource.cleanup();
                        continue;
                    }
                    let _outcome = resource
                        .persist(&target)
                        .expect("temporary file should persist to an absent target");
                    assert_eq!(data, fs::read(&target).expect("persisted bytes should be readable"));
                    let _ = fs::remove_file(target);
                }
            }
            3 => {
                let options = LocalTempDirectoryOptions::new()
                    .with_parent(root_path)
                    .with_max_attempts(1 + usize::from(selector % 4));
                if let Ok(resource) = native.create_temp_directory_with_options(&options) {
                    let child = resource.path().join("nested");
                    if fs::create_dir(&child).is_err() || fs::write(child.join("payload"), data).is_err() {
                        continue;
                    }
                    let list_options = LocalListOptions::new().with_recursive();
                    let entries = native
                        .list_with_options(resource.path(), &list_options)
                        .and_then(|walker| walker.collect::<Result<Vec<_>, _>>());
                    assert!(entries.is_ok(), "temporary directory listing should succeed");
                }
            }
            _ => {
                let entries = native
                    .list_with_options(root_path, &LocalListOptions::new())
                    .and_then(|walker| walker.collect::<Result<Vec<_>, _>>());
                assert!(entries.is_ok(), "fuzz root listing should succeed");
            }
        }
    }
});
