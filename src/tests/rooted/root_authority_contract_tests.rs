// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Crate-private authority and native-error contract tests.

use std::fs;
use std::path::Path;

#[cfg(unix)]
use crate::LocalFileOperation;
use crate::local::LocalRelativePath;
#[cfg(unix)]
use crate::rooted::Permissions;
use crate::rooted::Root;

#[test]
fn test_root_from_existing_authority_uses_descriptor_and_reports_diagnostic_path() {
    let fixture = tempfile::tempdir().expect("root fixture should exist");
    fs::write(fixture.path().join("payload"), b"descriptor authority").expect("file should exist");
    let opened = Root::open(fixture.path()).expect("root should open");
    let diagnostic = fixture.path().join("diagnostic-only");
    let root = Root::from_directory_authority(
        opened.try_clone_authority().expect("authority should clone"),
        diagnostic.clone(),
    );
    let path = LocalRelativePath::new(Path::new("payload")).expect("path should be relative");

    let metadata = root
        .symlink_metadata(&path)
        .expect("lookup must use the retained descriptor, even when diagnostic path does not exist");

    assert_eq!(metadata.size(), b"descriptor authority".len() as u64);
    assert_eq!(root.path(), diagnostic);
    assert_eq!(
        root.resolution_contract().limits(),
        crate::options::LocalResolutionLimits::new()
    );
    let debug = format!("{root:?}");
    assert!(debug.contains("Root"));
    assert!(debug.contains("diagnostic-only"));
}

#[cfg(unix)]
#[test]
fn test_root_native_errors_retain_read_link_and_recursive_delete_context() {
    let fixture = tempfile::tempdir().expect("root fixture should exist");
    fs::write(fixture.path().join("ordinary"), b"unchanged").expect("ordinary file should exist");
    let root = Root::open(fixture.path()).expect("root should open");
    let file = LocalRelativePath::new(Path::new("ordinary")).expect("path should be relative");
    root.set_permissions(&file, Permissions::from_unix_mode(0o600))
        .expect("rooted permissions should update through the retained authority");
    assert_eq!(
        Some(0o600),
        root.symlink_metadata(&file)
            .expect("updated metadata should be available")
            .permissions()
            .unix_mode()
    );

    let validations = std::cell::Cell::new(0);
    let validate = || {
        let validations = &validations;
        move |_: &Path| {
            validations.set(validations.get() + 1);
            Ok(())
        }
    };
    let error = root
        .read_link_limited(&file, None, validate())
        .expect_err("an ordinary file cannot be read as a symlink");
    assert_eq!(error.operation(), LocalFileOperation::Metadata);
    assert_eq!(error.path(), Some(file.as_path()));
    assert!(error.io_error().is_some());
    std::os::unix::fs::symlink("ordinary", fixture.path().join("link")).expect("real link should exist");
    let link = LocalRelativePath::new(Path::new("link")).expect("link path should be relative");
    assert_eq!(
        root.read_link_limited(&link, None, validate())
            .expect("link target should be validated"),
        Path::new("ordinary")
    );
    assert_eq!(
        Path::new("ordinary"),
        root.read_link(&link).expect("rooted link target should be readable")
    );
    assert_eq!(
        validations.get(),
        1,
        "only the successful read may validate a raw target"
    );

    let limited = root
        .read_link_limited(&link, Some(1), validate())
        .expect_err("a target beyond the byte limit must be rejected");
    assert_eq!(crate::LocalFileErrorKind::ResourceLimit, limited.kind());
    let facts = limited
        .resource_limit_error()
        .expect("link limit facts should be retained");
    assert_eq!(crate::LocalResourceKind::SymlinkTargetBytes, facts.resource());
    assert_eq!(Some(2), facts.observed());
    assert_eq!(
        1,
        validations.get(),
        "over-limit targets must not reach raw-target validation"
    );

    fs::create_dir(fixture.path().join("nested")).expect("nested directory should exist");
    let nested = LocalRelativePath::new(Path::new("nested")).expect("path should be relative");
    let entries = root
        .read_dir(&nested)
        .expect("rooted directory entries should be listed");
    assert!(entries.is_empty());

    let missing = LocalRelativePath::new(Path::new("missing")).expect("path should be relative");
    let error = root
        .remove_tree_typed(&missing)
        .expect_err("recursive removal must report a missing target");
    assert_eq!(error.operation(), LocalFileOperation::DeleteDirectory);
    assert_eq!(error.path(), Some(missing.as_path()));
    assert_eq!(error.io_error_kind(), std::io::ErrorKind::NotFound);
    assert_eq!(
        fs::read(fixture.path().join("ordinary")).expect("unrelated file should remain"),
        b"unchanged"
    );
}
