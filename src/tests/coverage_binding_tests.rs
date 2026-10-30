// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::path::Path;
use std::time::Duration;

use crate::LocalFileSystem;
use crate::LocalListOptions;
use crate::error::LocalFileErrorKind;
use crate::options::LocalDirectoryOpenOptions;
use crate::options::LocalObservedReadOptions;
use crate::options::LocalPathBindingOptions;

#[test]
fn observed_reader_binding_checks_deadline_and_retains_ancestors() {
    let fixture = tempfile::tempdir().expect("fixture directory should be created");
    let path = fixture.path().join("payload");
    std::fs::write(&path, b"observed").expect("fixture file should be written");
    let filesystem = LocalFileSystem::host().expect("host filesystem should open");

    let expired = filesystem
        .open_observed_reader(
            &path,
            &LocalObservedReadOptions::new()
                .with_path_binding(LocalPathBindingOptions::new().with_deadline(Duration::ZERO)),
        )
        .expect_err("zero-duration binding should expire before resolution");
    assert_eq!(expired.kind(), LocalFileErrorKind::Io);
    assert_eq!(std::io::ErrorKind::TimedOut, expired.io_error_kind());

    let reader = filesystem
        .open_observed_reader(
            &path,
            &LocalObservedReadOptions::new().with_path_binding(
                LocalPathBindingOptions::new()
                    .with_max_ancestors(64)
                    .with_deadline(Duration::from_secs(10)),
            ),
        )
        .expect("unexpired binding should retain and verify its ancestor chain");
    assert_eq!(reader.observation().metadata().len(), 8);
    assert!(reader.verify_path_binding().is_ok());

    let rooted = LocalFileSystem::rooted(fixture.path()).expect("rooted filesystem should open");
    assert_eq!(
        rooted
            .open_observed_reader(
                Path::new("payload"),
                &LocalObservedReadOptions::new().with_path_binding(LocalPathBindingOptions::new()),
            )
            .expect("rooted binding should open")
            .observation()
            .metadata()
            .len(),
        8
    );
}

#[test]
fn directory_binding_retains_chain_and_enforces_ancestor_limits() {
    let fixture = tempfile::tempdir().expect("fixture directory should be created");
    std::fs::create_dir(fixture.path().join("parent")).expect("parent should be created");
    std::fs::create_dir(fixture.path().join("parent/child")).expect("child should be created");
    let filesystem = LocalFileSystem::rooted(fixture.path()).expect("rooted filesystem should open");

    let root = filesystem
        .open_directory_with_options(
            Path::new("."),
            &LocalDirectoryOpenOptions::new().with_path_binding(LocalPathBindingOptions::new()),
        )
        .expect("bound root should open");
    let directory = root
        .open_directory(
            Path::new("parent/child"),
            &LocalDirectoryOpenOptions::new().with_path_binding(LocalPathBindingOptions::new().with_max_ancestors(2)),
        )
        .expect("child should retain its ancestor chain");

    let ancestors = directory
        .ancestor_observations(&LocalListOptions::new())
        .expect("unrestricted ancestor query should succeed");
    assert!(ancestors.len() >= 3);
    assert_eq!(
        directory
            .ancestor_observations(&LocalListOptions::new().with_max_depth(1))
            .expect_err("depth limit should reject the retained chain")
            .kind(),
        LocalFileErrorKind::ResourceLimit,
    );
    assert_eq!(
        root.open_directory(
            Path::new("parent/child"),
            &LocalDirectoryOpenOptions::new().with_path_binding(LocalPathBindingOptions::new().with_max_ancestors(0),),
        )
        .expect_err("zero ancestor budget should reject a nested directory")
        .kind(),
        LocalFileErrorKind::ResourceLimit,
    );
}
