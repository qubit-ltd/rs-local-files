// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
use std::path::Path;

use qubit_local_files::LocalFileSystem;
use qubit_local_files::options::LocalDirectoryOpenOptions;
use qubit_local_files::options::LocalObservedReadOptions;
use qubit_local_files::options::LocalPathBindingOptions;
use qubit_local_files::options::LocalTempDirectoryOptions;

fn assert_redacted(debug: &str) {
    assert!(
        !debug.contains("customer-secret-root-8f921d"),
        "debug output leaked a sensitive path: {debug}",
    );
}

#[test]
fn test_rooted_directory_and_reader_debug_redact_paths() {
    let scratch = tempfile::tempdir().expect("fixture should exist");
    let root = scratch.path().join("customer-secret-root-8f921d");
    std::fs::create_dir_all(root.join("nested")).expect("directories should exist");
    std::fs::write(root.join("nested/source"), b"source").expect("source should exist");

    let filesystem = LocalFileSystem::rooted(&root).expect("root should open");
    let directory = filesystem
        .open_directory_with_options(
            Path::new("nested"),
            &LocalDirectoryOpenOptions::new().with_path_binding(LocalPathBindingOptions::new()),
        )
        .expect("directory should open");
    let directory_debug = format!("{directory:?}");
    assert_redacted(&directory_debug);
    assert!(directory_debug.contains("LocalDirectoryHandle"));
    assert!(directory_debug.contains("Rooted"));
    assert!(directory_debug.contains("<redacted>"));
    assert!(directory_debug.contains("binding_enabled: true"));

    let unbound = filesystem
        .open_observed_reader(Path::new("nested/source"), &LocalObservedReadOptions::new())
        .expect("reader should open");
    let unbound_debug = format!("{unbound:?}");
    assert_redacted(&unbound_debug);
    assert!(unbound_debug.contains("LocalObservedReader"));
    assert!(unbound_debug.contains("binding_enabled: false"));
    assert!(!unbound_debug.contains("reader:"));

    let bound = filesystem
        .open_observed_reader(
            Path::new("nested/source"),
            &LocalObservedReadOptions::new().with_path_binding(LocalPathBindingOptions::new()),
        )
        .expect("bound reader should open");
    let bound_debug = format!("{bound:?}");
    assert_redacted(&bound_debug);
    assert!(bound_debug.contains("LocalObservedReader"));
    assert!(bound_debug.contains("binding_enabled: true"));
    assert!(bound_debug.contains("PathBinding {"));
    assert!(bound_debug.contains("<redacted>"), "{bound_debug}");
    assert!(!bound_debug.contains("reader:"));
}

#[test]
fn test_nested_directory_debug_redacts_anchor_paths() {
    let scratch = tempfile::tempdir().expect("fixture should exist");
    let root = scratch.path().join("customer-secret-root-8f921d");
    std::fs::create_dir_all(root.join("parent/child")).expect("directories should exist");

    let filesystem = LocalFileSystem::rooted(&root).expect("root should open");
    let parent = filesystem
        .open_directory_with_options(
            Path::new("parent"),
            &LocalDirectoryOpenOptions::new().with_path_binding(LocalPathBindingOptions::new()),
        )
        .expect("parent should open");
    let child = parent
        .open_directory(
            Path::new("child"),
            &LocalDirectoryOpenOptions::new().with_path_binding(LocalPathBindingOptions::new()),
        )
        .expect("child should open");

    let debug = format!("{child:?}");
    assert_redacted(&debug);
    assert!(debug.contains("LocalDirectoryHandle"));
    assert!(debug.contains("binding_enabled: true"));
    assert!(debug.contains("Rooted"));
}

#[test]
fn test_nested_retained_stage_debug_redacts_owner_path() {
    let scratch = tempfile::tempdir().expect("fixture should exist");
    let root = scratch.path().join("customer-secret-root-8f921d");
    std::fs::create_dir_all(root.join("nested")).expect("directory should exist");

    let filesystem = LocalFileSystem::rooted(&root).expect("root should open");
    let parent = filesystem
        .open_directory_with_options(
            Path::new("nested"),
            &LocalDirectoryOpenOptions::new().with_path_binding(LocalPathBindingOptions::new()),
        )
        .expect("nested parent should open");
    let stage = parent
        .create_staged_directory(&LocalTempDirectoryOptions::new())
        .expect("stage should open");

    let debug = format!("{stage:?}");
    assert_redacted(&debug);
    assert!(debug.contains("LocalStagedDirectory"));
    assert!(debug.contains("LocalTreeRetainedResource"));
    assert!(debug.contains("binding_enabled: true"));
    assert!(debug.contains("<redacted>"));
    stage.cleanup().expect("stage should be cleaned by its owner");
}
