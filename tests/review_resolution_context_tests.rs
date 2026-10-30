// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::path::Path;
use std::path::PathBuf;

use qubit_local_files::LocalFileSystem;
use qubit_local_files::error::LocalFileErrorKind;
use qubit_local_files::error::LocalFileOperation;
use qubit_local_files::error::LocalResourceKind;
use qubit_local_files::options::LocalReadOptions;
use qubit_local_files::options::LocalResolutionLimits;
use qubit_local_files::options::LocalRootOpenOptions;

#[test]
fn test_review_initial_resolution_budget_keeps_open_reader_context() {
    let fixture = tempfile::tempdir().expect("fixture should exist");
    let mut path = PathBuf::new();
    for _ in 0..=256 {
        path.push("a");
    }
    let filesystem = LocalFileSystem::rooted_with_options(
        fixture.path(),
        &LocalRootOpenOptions::new()
            .with_resolution_limits(LocalResolutionLimits::new().with_max_actual_components(256)),
    )
    .expect("root authority should open");

    let error = filesystem
        .open_reader_with_options(&path, &LocalReadOptions::new())
        .expect_err("path exceeding the pending-component budget must be rejected");

    assert_eq!(LocalFileErrorKind::ResourceLimit, error.kind());
    assert_eq!(LocalFileOperation::OpenReader, error.operation());
    assert_eq!(Some(Path::new("/").join(&path).as_path()), error.path());
    assert_eq!(
        LocalResourceKind::PendingResolutionComponents,
        error.resource_limit_error().expect("typed resource facts").resource()
    );
}

#[cfg(unix)]
#[test]
fn test_review_symlink_target_budget_keeps_open_reader_context() {
    use std::os::unix::fs::symlink;

    let fixture = tempfile::Builder::new()
        .prefix("r")
        .tempdir_in("/tmp")
        .expect("fixture should exist in a short temporary path");
    let root = fixture.path().canonicalize().expect("fixture root should resolve");
    std::fs::create_dir_all(fixture.path().join("a/b/c/d")).expect("target directories should exist");
    std::fs::write(fixture.path().join("a/b/c/d/file"), b"payload").expect("target file should exist");
    symlink("a/b/c/d/file", fixture.path().join("link")).expect("link should exist");
    let filesystem = LocalFileSystem::rooted_with_options(
        &root,
        &LocalRootOpenOptions::new().with_resolution_limits(LocalResolutionLimits::new().with_max_actual_components(2)),
    )
    .expect("root authority should open");

    let error = filesystem
        .open_reader_with_options(Path::new("link"), &LocalReadOptions::new())
        .expect_err("expanded target must exceed the component budget");

    assert_eq!(LocalFileErrorKind::ResourceLimit, error.kind());
    assert_eq!(LocalFileOperation::OpenReader, error.operation());
    assert_eq!(Some(Path::new("/link")), error.path());
    assert_eq!(
        LocalResourceKind::PendingResolutionComponents,
        error.resource_limit_error().expect("typed resource facts").resource()
    );
}
