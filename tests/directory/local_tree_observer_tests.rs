// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::path::Path;

use qubit_local_files::LocalFileSystem;
use qubit_local_files::directory::LocalTreeEntry;
use qubit_local_files::error::LocalFileErrorKind;
use qubit_local_files::options::LocalDirectoryOpenOptions;
use qubit_local_files::options::LocalListOptions;
use qubit_local_files::options::LocalTempDirectoryOptions;
use qubit_local_files::options::LocalTreeValidationOptions;
use qubit_local_files::outcome::LocalFileKind;

#[test]
fn lf3_tree_observer_streams_created_entry() {
    let fixture = tempfile::tempdir().expect("fixture should exist");
    let fs = LocalFileSystem::host().expect("host filesystem should open");
    let parent = fs
        .open_directory_with_options(fixture.path(), &LocalDirectoryOpenOptions::new())
        .expect("parent should open");
    let mut stage = parent
        .create_staged_directory(&LocalTempDirectoryOptions::new())
        .expect("stage should be created");
    let mut bytes = b"entry".as_slice();
    let observation = stage
        .write_file_new(Path::new("file"), &mut bytes, 5)
        .expect("file should be written");
    assert_eq!(observation.metadata().kind(), LocalFileKind::File);
    assert!(observation.identity().is_some());
    let mut observer = stage
        .observe_tree_with_options(&LocalListOptions::new())
        .expect("observer should open");
    let first = observer
        .next()
        .expect("entry should be yielded")
        .expect("entry should observe");
    assert_eq!(first.relative_path(), Path::new("file"));
    assert_eq!(first.observation().metadata().kind(), LocalFileKind::File);
    assert!(observer.next().is_none());
}

#[test]
fn lf_review_tree_finite_depth_accepts_complete_shallow_tree() {
    let fixture = tempfile::tempdir().expect("fixture");
    let fs = LocalFileSystem::host().expect("Host");
    let parent = fs
        .open_directory_with_options(fixture.path(), &LocalDirectoryOpenOptions::new())
        .expect("parent");
    let mut stage = parent
        .create_staged_directory(&LocalTempDirectoryOptions::new())
        .expect("stage");
    let mut bytes = b"x".as_slice();
    stage.write_file_new(Path::new("file"), &mut bytes, 1).expect("write");
    let expected = [LocalTreeEntry::new(Path::new("file").to_path_buf(), LocalFileKind::File).expect("expected")];
    let validation = LocalTreeValidationOptions::new().with_limits(LocalListOptions::new().with_max_depth(1));
    stage
        .verify_entries(&expected, &validation)
        .expect("a fully observed depth-one tree is within budget");
    stage.cleanup().expect("cleanup");
}

#[test]
fn lf_review_tree_depth_zero_and_boundary() {
    let fixture = tempfile::tempdir().expect("fixture");
    let fs = LocalFileSystem::host().expect("Host");
    let parent = fs
        .open_directory_with_options(fixture.path(), &LocalDirectoryOpenOptions::new())
        .expect("parent");
    let empty = parent
        .create_staged_directory(&LocalTempDirectoryOptions::new())
        .expect("empty stage");
    empty
        .verify_entries(
            &[],
            &LocalTreeValidationOptions::new().with_limits(LocalListOptions::new().with_max_depth(0)),
        )
        .expect("empty tree is within depth zero");
    empty.cleanup().expect("empty cleanup");

    let mut stage = parent
        .create_staged_directory(&LocalTempDirectoryOptions::new())
        .expect("stage");
    let mut bytes = b"x".as_slice();
    stage.write_file_new(Path::new("file"), &mut bytes, 1).expect("write");
    let expected = [LocalTreeEntry::new(Path::new("file").to_path_buf(), LocalFileKind::File).expect("expected")];
    stage
        .verify_entries(
            &expected,
            &LocalTreeValidationOptions::new().with_limits(LocalListOptions::new().with_max_depth(1)),
        )
        .expect("direct child is within depth one");
    let error = stage
        .verify_entries(
            &expected,
            &LocalTreeValidationOptions::new().with_limits(LocalListOptions::new().with_max_depth(0)),
        )
        .expect_err("direct child exceeds depth zero");
    assert_eq!(error.kind(), LocalFileErrorKind::ResourceLimit);
    stage.cleanup().expect("stage cleanup");
}

#[test]
fn lf_review_tree_duplicate_expected_paths_are_invalid_options() {
    let fixture = tempfile::tempdir().expect("fixture");
    let fs = LocalFileSystem::host().expect("Host");
    let parent = fs
        .open_directory_with_options(fixture.path(), &LocalDirectoryOpenOptions::new())
        .expect("parent");
    let stage = parent
        .create_staged_directory(&LocalTempDirectoryOptions::new())
        .expect("stage");
    let duplicate = [
        LocalTreeEntry::new(Path::new("same").to_path_buf(), LocalFileKind::File).expect("entry"),
        LocalTreeEntry::new(Path::new("same").to_path_buf(), LocalFileKind::File).expect("entry"),
    ];
    let error = stage
        .verify_entries(&duplicate, &LocalTreeValidationOptions::new())
        .expect_err("duplicate expected paths are invalid");
    assert_eq!(error.kind(), LocalFileErrorKind::InvalidOptions);
    let different_kinds = [
        LocalTreeEntry::new(Path::new("same").to_path_buf(), LocalFileKind::File).expect("entry"),
        LocalTreeEntry::new(Path::new("same").to_path_buf(), LocalFileKind::Directory).expect("entry"),
    ];
    let error = stage
        .verify_entries(&different_kinds, &LocalTreeValidationOptions::new())
        .expect_err("same path with different kinds is still a duplicate");
    assert_eq!(error.kind(), LocalFileErrorKind::InvalidOptions);
}

#[test]
fn lf_review_tree_zero_deadline_and_entry_limit_fail() {
    let fixture = tempfile::tempdir().expect("fixture");
    let fs = LocalFileSystem::host().expect("Host");
    let parent = fs
        .open_directory_with_options(fixture.path(), &LocalDirectoryOpenOptions::new())
        .expect("parent");
    let mut stage = parent
        .create_staged_directory(&LocalTempDirectoryOptions::new())
        .expect("stage");
    let mut bytes = b"x".as_slice();
    stage.write_file_new(Path::new("file"), &mut bytes, 1).expect("write");
    let expected = [LocalTreeEntry::new(Path::new("file").to_path_buf(), LocalFileKind::File).expect("expected")];
    for validation in [
        LocalTreeValidationOptions::new().with_limits(LocalListOptions::new().with_deadline(std::time::Duration::ZERO)),
        LocalTreeValidationOptions::new().with_limits(LocalListOptions::new().with_max_entries(0)),
    ] {
        let error = stage
            .verify_entries(&expected, &validation)
            .expect_err("zero budget rejects tree");
        assert_eq!(error.kind(), LocalFileErrorKind::ResourceLimit);
    }
    stage.cleanup().expect("cleanup");
}

#[test]
fn lf_review_tree_observer_depth_still_limits_display() {
    let fixture = tempfile::tempdir().expect("fixture");
    let fs = LocalFileSystem::host().expect("Host");
    let parent = fs
        .open_directory_with_options(fixture.path(), &LocalDirectoryOpenOptions::new())
        .expect("parent");
    let mut stage = parent
        .create_staged_directory(&LocalTempDirectoryOptions::new())
        .expect("stage");
    let mut bytes = b"x".as_slice();
    stage.write_file_new(Path::new("file"), &mut bytes, 1).expect("write");
    assert!(
        stage
            .observe_tree_with_options(&LocalListOptions::new().with_max_depth(0))
            .expect("observer")
            .next()
            .is_none()
    );
    assert!(
        stage
            .observe_tree_with_options(&LocalListOptions::new().with_max_depth(1))
            .expect("observer")
            .next()
            .expect("entry")
            .is_ok()
    );
    stage.cleanup().expect("cleanup");
}
