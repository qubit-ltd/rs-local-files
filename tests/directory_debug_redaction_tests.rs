// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
use std::path::Path;

use qubit_io::Output;
use qubit_local_files::LocalFileSystem;
use qubit_local_files::options::LocalDirectoryOpenOptions;
use qubit_local_files::options::LocalObservedReadOptions;
use qubit_local_files::options::LocalPathBindingOptions;
use qubit_local_files::options::LocalSealOptions;
use qubit_local_files::options::LocalTempDirectoryOptions;
use qubit_local_files::options::LocalWriteMode;
use qubit_local_files::options::LocalWriteOptions;

fn assert_resource_debug_redacted(resource: &impl std::fmt::Debug, expected_type: &str) {
    for output in [format!("{resource:?}"), format!("{resource:#?}")] {
        assert!(!output.contains("customer-secret-root-8f921d"), "{output}");
        assert!(!output.contains("private-leaf-65b3a0"), "{output}");
        assert!(output.contains(expected_type), "{output}");
    }
}

fn assert_debug_hides_marker(resource: &impl std::fmt::Debug, expected_type: &str, marker: &str) {
    for output in [format!("{resource:?}"), format!("{resource:#?}")] {
        assert!(!output.contains(marker), "{output}");
        assert!(output.contains(expected_type), "{output}");
    }
}

fn assert_debug_contains(resource: &impl std::fmt::Debug, expected: &str) {
    for output in [format!("{resource:?}"), format!("{resource:#?}")] {
        assert!(output.contains(expected), "{output}");
    }
}

#[test]
fn test_file_resource_debug_redacts_paths_without_formatting_backends() {
    for rooted in [false, true] {
        let scratch = tempfile::tempdir().expect("fixture should exist");
        let root = scratch.path().join("customer-secret-root-8f921d");
        std::fs::create_dir_all(&root).expect("root should exist");
        let leaf = root.join("private-leaf-65b3a0");
        std::fs::write(&leaf, b"existing").expect("leaf should exist");
        let filesystem = if rooted {
            LocalFileSystem::rooted(&root)
        } else {
            LocalFileSystem::host()
        }
        .expect("filesystem should open");
        let target = if rooted {
            Path::new("private-leaf-65b3a0")
        } else {
            leaf.as_path()
        };
        let staged_target = if rooted {
            std::path::PathBuf::from("private-leaf-65b3a0-staged")
        } else {
            root.join("private-leaf-65b3a0-staged")
        };

        let reader = filesystem.open_reader(target).expect("reader should open");
        assert_resource_debug_redacted(&reader, "LocalFileReader");

        let staged = filesystem
            .open_writer(&staged_target)
            .expect("staged writer should open");
        assert_resource_debug_redacted(&staged, "LocalFileWriter");
        assert_debug_contains(&staged, "state: Open");
        drop(staged);

        let append = filesystem
            .open_writer_with_options(target, &LocalWriteOptions::new(LocalWriteMode::Append))
            .expect("append writer should open");
        assert_resource_debug_redacted(&append, "LocalFileWriter");
        assert_debug_contains(&append, "state: Open");
        drop(append);

        let mut temporary = filesystem.create_temp_file().expect("temporary file should open");
        assert_resource_debug_redacted(&temporary, "LocalTempFile");
        assert_debug_contains(&temporary, "handle_open: true");
        let temporary_name = temporary.path().file_name().expect("generated name").to_string_lossy();
        assert_debug_hides_marker(&temporary, "LocalTempFile", &temporary_name);

        let mut temporary_directory = filesystem
            .create_temp_directory()
            .expect("temporary directory should open");
        assert_resource_debug_redacted(&temporary_directory, "LocalTempDirectory");
        let directory_name = temporary_directory
            .path()
            .file_name()
            .expect("generated name")
            .to_string_lossy();
        assert_debug_hides_marker(&temporary_directory, "LocalTempDirectory", &directory_name);

        let mut seal_source = filesystem.create_temp_file().expect("seal source should open");
        Output::write_fully(&mut seal_source, b"sealed").expect("seal payload should write");
        let mut sealed = seal_source
            .seal_with_options(&LocalSealOptions::default())
            .expect("temporary file should seal");
        assert_resource_debug_redacted(&sealed, "LocalSealedTempFile");

        temporary.cleanup().expect("temporary file should clean up");
        temporary_directory
            .cleanup()
            .expect("temporary directory should clean up");
        sealed.cleanup().expect("sealed file should clean up");
    }
}

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
