// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Bounded source-mode copies preserve payloads and preflight state.

#![no_main]

use std::fs;
use std::path::PathBuf;

use libfuzzer_sys::fuzz_target;
use qubit_local_files::LocalFileSystem;
use qubit_local_files::error::LocalFileErrorKind;
use qubit_local_files::options::LocalCopyOptions;
use qubit_local_files::options::LocalCopySourceMode;
use qubit_local_files::options::LocalDeleteOptions;
use qubit_local_files::outcome::LocalCopyFailureState;

mod support;

use support::FuzzRoot;

fuzz_target!(|input: &[u8]| {
    let data = &input[..input.len().min(256)];
    let Some(root) = FuzzRoot::create("copy-lifecycle") else {
        return;
    };
    let directory_source = data.first().is_some_and(|byte| byte & 1 != 0);
    let source = root.path().join("source");
    let payload = if directory_source {
        if fs::create_dir(&source).is_err() {
            return;
        }
        source.join("payload")
    } else {
        source.clone()
    };
    if fs::write(payload, data).is_err() {
        return;
    }
    for (index, operation) in data.iter().take(16).enumerate() {
        let rooted = operation & 4 != 0;
        let Ok(filesystem) = (if rooted {
            LocalFileSystem::rooted(root.path())
        } else {
            LocalFileSystem::host()
        }) else {
            return;
        };
        let mode = match operation % 3 {
            0 => LocalCopySourceMode::Entry,
            1 => LocalCopySourceMode::Tree,
            _ => LocalCopySourceMode::Auto,
        };
        let target_name = format!("target-{index}");
        let native_target = root.path().join(&target_name);
        let source_path = if rooted {
            PathBuf::from("source")
        } else {
            source.clone()
        };
        let target = if rooted {
            PathBuf::from(target_name)
        } else {
            native_target.clone()
        };
        let options = LocalCopyOptions::new()
            .with_source_mode(mode)
            .with_max_depth(2)
            .with_max_entries(4)
            .with_max_bytes(512)
            .with_max_open_directories(3);
        let mismatch = (directory_source && mode == LocalCopySourceMode::Entry)
            || (!directory_source && mode == LocalCopySourceMode::Tree);
        match filesystem.copy_with_options(&source_path, &target, &options) {
            Ok(outcome) => {
                assert!(!mismatch, "source mode must reject incompatible kinds");
                assert_eq!(1, outcome.stats().files());
                let copied = if directory_source {
                    native_target.join("payload")
                } else {
                    native_target.clone()
                };
                if let Ok(bytes) = fs::read(copied) {
                    assert_eq!(data, bytes);
                }
                let deletion = if directory_source {
                    filesystem.delete_directory_with_options(
                        &target,
                        &LocalDeleteOptions::new()
                            .with_recursive()
                            .with_max_depth(2)
                            .with_max_entries(4)
                            .with_max_pending_path_bytes(8192),
                    )
                } else {
                    filesystem.delete_file(&target)
                };
                if deletion.is_ok() {
                    assert!(!native_target.exists());
                }
            }
            Err(failure) => {
                if mismatch {
                    assert_eq!(LocalFileErrorKind::RequirementNotMet, failure.error().kind());
                }
                if failure.state() == LocalCopyFailureState::Unchanged {
                    assert!(!native_target.exists());
                }
            }
        }
        let source_payload = if directory_source {
            source.join("payload")
        } else {
            source.clone()
        };
        if let Ok(bytes) = fs::read(source_payload) {
            assert_eq!(data, bytes, "copy must never mutate source content");
        }
    }
});
