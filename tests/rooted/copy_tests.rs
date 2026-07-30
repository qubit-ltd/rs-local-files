// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use qubit_local_files::{
    copy,
    rooted,
};

/// Runs one coverage-only rooted-copy fault case in an isolated child process.
#[cfg(coverage)]
fn run_in_coverage_fault_process<F>(test_name: &str, fault: &str, action: F)
where
    F: FnOnce(),
{
    const COVERAGE_FAULT_ENV: &str = "QUBIT_LOCAL_FILES_COVERAGE_FAULT";
    if std::env::var_os(COVERAGE_FAULT_ENV).is_some() {
        action();
        return;
    }
    let executable =
        std::env::current_exe().expect("test executable should be available");
    let status = std::process::Command::new(executable)
        .arg("--exact")
        .arg(test_name)
        .arg("--nocapture")
        .env(COVERAGE_FAULT_ENV, fault)
        .status()
        .expect("coverage fault child should launch");
    assert!(status.success(), "coverage fault child should pass");
}

/// Verifies that rooted copy stages and installs one regular file.
#[cfg(any(unix, windows))]
#[test]
fn test_copy_file_installs_complete_contents() {
    let temp = tempfile::tempdir().expect("a temporary root should exist");
    std::fs::write(temp.path().join("source"), b"complete payload")
        .expect("the source should be written");
    let root = rooted::Root::open(temp.path()).expect("the root should open");
    let source =
        rooted::Path::new("source").expect("the source should validate");
    let destination = rooted::Path::new("destination")
        .expect("the destination should validate");

    let statistics = root
        .copy(&source, &destination, copy::Options::new())
        .expect("the rooted file should be copied");

    assert_eq!(1, statistics.files());
    assert_eq!(16, statistics.bytes());
    assert_eq!(
        b"complete payload",
        std::fs::read(temp.path().join("destination"))
            .expect("the destination should be readable")
            .as_slice(),
    );
}

/// Verifies that rooted copy traverses directory trees without following links.
#[cfg(any(unix, windows))]
#[test]
fn test_copy_directory_copies_regular_descendants() {
    let temp = tempfile::tempdir().expect("a temporary root should exist");
    std::fs::create_dir_all(temp.path().join("source/nested"))
        .expect("the source tree should exist");
    std::fs::write(temp.path().join("source/nested/value"), b"value")
        .expect("the source should be written");
    let root = rooted::Root::open(temp.path()).expect("the root should open");
    let source =
        rooted::Path::new("source").expect("the source should validate");
    let destination = rooted::Path::new("destination")
        .expect("the destination should validate");

    let statistics = root
        .copy(&source, &destination, copy::Options::new())
        .expect("the rooted tree should be copied");

    assert_eq!(1, statistics.files());
    assert_eq!(2, statistics.directories());
    assert_eq!(
        b"value",
        std::fs::read(temp.path().join("destination/nested/value"))
            .expect("the destination should be readable")
            .as_slice(),
    );
}

/// Verifies that conservative rooted copy preserves an existing destination.
#[cfg(any(unix, windows))]
#[test]
fn test_copy_file_rejects_existing_destination() {
    let temp = tempfile::tempdir().expect("a temporary root should exist");
    std::fs::write(temp.path().join("source"), b"source")
        .expect("the source should be written");
    std::fs::write(temp.path().join("destination"), b"destination")
        .expect("the destination should be written");
    let root = rooted::Root::open(temp.path()).expect("the root should open");
    let source =
        rooted::Path::new("source").expect("the source should validate");
    let destination = rooted::Path::new("destination")
        .expect("the destination should validate");

    let error = root
        .copy(&source, &destination, copy::Options::new())
        .expect_err("the destination conflict should be rejected");

    assert_eq!(std::io::ErrorKind::AlreadyExists, error.kind());
    assert_eq!(
        b"destination",
        std::fs::read(temp.path().join("destination"))
            .expect("the destination should remain readable")
            .as_slice(),
    );
}

/// Verifies overwrite and skip policies preserve their distinct contracts.
#[cfg(any(unix, windows))]
#[test]
fn test_copy_file_applies_explicit_conflict_policies() {
    let temp = tempfile::tempdir().expect("a temporary root should exist");
    std::fs::write(temp.path().join("source"), b"new")
        .expect("the source should be written");
    std::fs::write(temp.path().join("destination"), b"old")
        .expect("the destination should be written");
    let root = rooted::Root::open(temp.path()).expect("the root should open");
    let source =
        rooted::Path::new("source").expect("the source should validate");
    let destination = rooted::Path::new("destination")
        .expect("the destination should validate");

    let skipped = root
        .copy(
            &source,
            &destination,
            copy::Options::new().with_conflict(copy::ConflictPolicy::Skip),
        )
        .expect("skip should keep the destination");
    assert_eq!(1, skipped.skipped());
    assert_eq!(
        b"old",
        std::fs::read(temp.path().join("destination"))
            .unwrap()
            .as_slice()
    );

    let overwritten = root
        .copy(
            &source,
            &destination,
            copy::Options::new().with_conflict(copy::ConflictPolicy::Overwrite),
        )
        .expect("overwrite should replace the destination");
    assert_eq!(1, overwritten.files());
    assert_eq!(1, overwritten.overwritten());
    assert_eq!(
        b"new",
        std::fs::read(temp.path().join("destination"))
            .unwrap()
            .as_slice()
    );
}

/// Verifies type replacement removes the old tree before installing a file.
#[cfg(any(unix, windows))]
#[test]
fn test_copy_file_replaces_directory_type_conflict() {
    let temp = tempfile::tempdir().expect("a temporary root should exist");
    std::fs::write(temp.path().join("source"), b"file")
        .expect("the source should be written");
    std::fs::create_dir_all(temp.path().join("destination/nested"))
        .expect("the destination tree should exist");
    std::fs::write(temp.path().join("destination/nested/value"), b"old")
        .expect("the old child should exist");
    let root = rooted::Root::open(temp.path()).expect("the root should open");
    let source =
        rooted::Path::new("source").expect("the source should validate");
    let destination = rooted::Path::new("destination")
        .expect("the destination should validate");

    let _ = root
        .copy(
            &source,
            &destination,
            copy::Options::new()
                .with_type_conflict(copy::TypeConflictPolicy::Replace),
        )
        .expect("the directory should be replaced by a file");

    assert_eq!(
        b"file",
        std::fs::read(temp.path().join("destination"))
            .unwrap()
            .as_slice()
    );
}

/// Verifies invalid self and nested-tree destinations are rejected.
#[cfg(any(unix, windows))]
#[test]
fn test_copy_rejects_self_and_nested_tree_destinations() {
    let temp = tempfile::tempdir().expect("a temporary root should exist");
    std::fs::create_dir(temp.path().join("source"))
        .expect("the source should exist");
    let root = rooted::Root::open(temp.path()).expect("the root should open");
    let source =
        rooted::Path::new("source").expect("the source should validate");
    let nested = rooted::Path::new("source/nested")
        .expect("the nested path should validate");

    assert!(root.copy(&source, &source, copy::Options::new()).is_err());
    assert!(root.copy(&source, &nested, copy::Options::new()).is_err());
}

/// Verifies rooted directory copy uses an explicit stack for deep trees.
#[cfg(any(unix, windows))]
#[test]
fn test_copy_deep_tree_without_recursive_call_stack() {
    let temp = tempfile::tempdir().expect("a temporary root should exist");
    let mut source_path = temp.path().join("source");
    for _ in 0..128 {
        source_path.push("d");
    }
    std::fs::create_dir_all(&source_path)
        .expect("the deep source should exist");
    std::fs::write(source_path.join("value"), b"deep")
        .expect("the leaf should be written");
    let root = rooted::Root::open(temp.path()).expect("the root should open");
    let source =
        rooted::Path::new("source").expect("the source should validate");
    let destination = rooted::Path::new("destination")
        .expect("the destination should validate");

    let statistics = root
        .copy(&source, &destination, copy::Options::new())
        .expect("the deep tree should copy");

    assert_eq!(1, statistics.files());
    assert_eq!(129, statistics.directories());
}

/// Verifies rooted copy rejects links instead of following them.
#[cfg(unix)]
#[test]
fn test_copy_rejects_symbolic_links() {
    use std::os::unix::fs::symlink;

    let temp = tempfile::tempdir().expect("a temporary root should exist");
    std::fs::write(temp.path().join("target"), b"value")
        .expect("the target should exist");
    symlink("target", temp.path().join("source"))
        .expect("the link should exist");
    let root = rooted::Root::open(temp.path()).expect("the root should open");
    let source =
        rooted::Path::new("source").expect("the source should validate");
    let destination = rooted::Path::new("destination")
        .expect("the destination should validate");

    assert!(
        root.copy(&source, &destination, copy::Options::new())
            .is_err()
    );
    assert!(
        root.copy(
            &source,
            &destination,
            copy::Options::new().follow_symlinks(),
        )
        .is_err()
    );
}

/// Verifies permission preservation uses the metadata from the opened source.
#[cfg(unix)]
#[test]
fn test_copy_preserves_permissions_when_requested() {
    use std::os::unix::fs::{
        MetadataExt,
        PermissionsExt,
    };

    let temp = tempfile::tempdir().expect("a temporary root should exist");
    std::fs::write(temp.path().join("source"), b"value")
        .expect("the source should be written");
    std::fs::set_permissions(
        temp.path().join("source"),
        std::fs::Permissions::from_mode(0o640),
    )
    .expect("the source mode should be set");
    let root = rooted::Root::open(temp.path()).expect("the root should open");
    let source =
        rooted::Path::new("source").expect("the source should validate");
    let destination = rooted::Path::new("destination")
        .expect("the destination should validate");

    let _ = root
        .copy(
            &source,
            &destination,
            copy::Options::new().preserve_permissions(),
        )
        .expect("the file should copy with permissions");

    assert_eq!(
        0o640,
        std::fs::metadata(temp.path().join("destination"))
            .unwrap()
            .mode()
            & 0o777
    );
}

/// Verifies preserving permissions applies after a directory's descendants
/// have been installed, not only to regular-file destinations.
#[cfg(unix)]
#[test]
fn test_copy_directory_preserves_source_directory_permissions() {
    use std::os::unix::fs::{
        MetadataExt,
        PermissionsExt,
    };

    let temp = tempfile::tempdir().expect("a temporary root should exist");
    std::fs::create_dir_all(temp.path().join("source/nested"))
        .expect("the source tree should exist");
    std::fs::write(temp.path().join("source/nested/value"), b"value")
        .expect("the source value should be written");
    std::fs::set_permissions(
        temp.path().join("source"),
        std::fs::Permissions::from_mode(0o750),
    )
    .expect("the source directory mode should be set");
    std::fs::set_permissions(
        temp.path().join("source/nested"),
        std::fs::Permissions::from_mode(0o710),
    )
    .expect("the nested source directory mode should be set");
    let root = rooted::Root::open(temp.path()).expect("the root should open");
    let source =
        rooted::Path::new("source").expect("the source should validate");
    let destination = rooted::Path::new("destination")
        .expect("the destination should validate");

    let _ = root
        .copy(
            &source,
            &destination,
            copy::Options::new().preserve_permissions(),
        )
        .expect("the directory tree should copy with permissions");

    assert_eq!(
        0o750,
        std::fs::metadata(temp.path().join("destination"))
            .expect("the copied root directory should have metadata")
            .mode()
            & 0o777,
    );
    assert_eq!(
        0o710,
        std::fs::metadata(temp.path().join("destination/nested"))
            .expect("the copied nested directory should have metadata")
            .mode()
            & 0o777,
    );
}

/// Verifies directory-copy conflict policies either retain, skip, or merge the
/// destination tree, and that explicit type replacement can replace a file.
#[cfg(any(unix, windows))]
#[test]
fn test_copy_directory_applies_conflict_and_type_replacement_policies() {
    let temp = tempfile::tempdir().expect("a temporary root should exist");
    std::fs::create_dir_all(temp.path().join("source/nested"))
        .expect("the source tree should exist");
    std::fs::write(temp.path().join("source/nested/value"), b"new")
        .expect("the source value should be written");
    std::fs::create_dir_all(temp.path().join("destination"))
        .expect("the destination tree should exist");
    std::fs::write(temp.path().join("destination/old"), b"old")
        .expect("the destination value should be written");
    let root = rooted::Root::open(temp.path()).expect("the root should open");
    let source =
        rooted::Path::new("source").expect("the source should validate");
    let destination = rooted::Path::new("destination")
        .expect("the destination should validate");

    let conflict = root
        .copy(&source, &destination, copy::Options::new())
        .expect_err("the default directory conflict should fail");
    assert_eq!(std::io::ErrorKind::AlreadyExists, conflict.kind());

    let skipped = root
        .copy(
            &source,
            &destination,
            copy::Options::new().with_conflict(copy::ConflictPolicy::Skip),
        )
        .expect("skip should retain the destination tree");
    assert_eq!(1, skipped.skipped());
    assert!(!temp.path().join("destination/nested/value").exists());

    let overwritten = root
        .copy(
            &source,
            &destination,
            copy::Options::new().with_conflict(copy::ConflictPolicy::Overwrite),
        )
        .expect("overwrite should merge the destination tree");
    assert_eq!(1, overwritten.overwritten());
    assert_eq!(
        b"new",
        std::fs::read(temp.path().join("destination/nested/value"))
            .expect("the merged value should be readable")
            .as_slice(),
    );

    std::fs::write(temp.path().join("file-target"), b"file")
        .expect("the conflicting file should be written");
    let file_target = rooted::Path::new("file-target")
        .expect("the conflicting target should validate");
    let replaced = root
        .copy(
            &source,
            &file_target,
            copy::Options::new()
                .with_type_conflict(copy::TypeConflictPolicy::Replace),
        )
        .expect("type replacement should install the source tree");
    assert_eq!(2, replaced.directories());
    assert_eq!(1, replaced.overwritten());
    assert_eq!(
        b"new",
        std::fs::read(temp.path().join("file-target/nested/value"))
            .expect("the replacement tree value should be readable")
            .as_slice(),
    );
}

/// Verifies rooted file copy distinguishes type conflicts, hard-link aliases,
/// missing sources, and unsupported descendants.
#[cfg(unix)]
#[test]
fn test_copy_rejects_file_aliases_and_unsupported_entries() {
    use std::os::unix::fs::symlink;

    let temp = tempfile::tempdir().expect("a temporary root should exist");
    std::fs::write(temp.path().join("source"), b"source")
        .expect("the source should be written");
    std::fs::create_dir(temp.path().join("directory-target"))
        .expect("the conflicting directory should be created");
    std::fs::hard_link(temp.path().join("source"), temp.path().join("alias"))
        .expect("the hard-link alias should be created");
    std::fs::create_dir(temp.path().join("tree"))
        .expect("the source tree should be created");
    symlink("../source", temp.path().join("tree/link"))
        .expect("the unsupported descendant should be created");
    let root = rooted::Root::open(temp.path()).expect("the root should open");
    let source =
        rooted::Path::new("source").expect("the source should validate");

    let type_conflict = root
        .copy(
            &source,
            &rooted::Path::new("directory-target")
                .expect("the directory target should validate"),
            copy::Options::new(),
        )
        .expect_err("a file must not replace a directory by default");
    assert_eq!(std::io::ErrorKind::AlreadyExists, type_conflict.kind());

    let alias = root
        .copy(
            &source,
            &rooted::Path::new("alias").expect("the alias should validate"),
            copy::Options::new(),
        )
        .expect_err("a hard-link alias must not be copied onto itself");
    assert_eq!(std::io::ErrorKind::InvalidInput, alias.kind());

    let missing = root
        .copy(
            &rooted::Path::new("missing")
                .expect("the missing source should validate"),
            &rooted::Path::new("unwritten")
                .expect("the missing destination should validate"),
            copy::Options::new(),
        )
        .expect_err("a missing source must fail before destination creation");
    assert_eq!(std::io::ErrorKind::NotFound, missing.kind());

    let unsupported = root
        .copy(
            &rooted::Path::new("tree")
                .expect("the source tree should validate"),
            &rooted::Path::new("tree-copy")
                .expect("the destination tree should validate"),
            copy::Options::new(),
        )
        .expect_err("a symbolic-link descendant must be rejected");
    assert_eq!(std::io::ErrorKind::Unsupported, unsupported.kind());
}

/// Verifies rooted copy handles symbolic-link destinations as type conflicts
/// and never follows them while replacing the final entry.
#[cfg(unix)]
#[test]
fn test_copy_replaces_symbolic_link_destination_without_following_target() {
    use std::os::unix::fs::symlink;

    let temp = tempfile::tempdir().expect("a temporary root should exist");
    std::fs::write(temp.path().join("source"), b"replacement")
        .expect("the source should be written");
    std::fs::write(temp.path().join("outside-target"), b"unchanged")
        .expect("the linked target should be written");
    symlink("outside-target", temp.path().join("destination"))
        .expect("the destination link should be created");
    let root = rooted::Root::open(temp.path()).expect("the root should open");
    let source =
        rooted::Path::new("source").expect("the source should validate");
    let destination = rooted::Path::new("destination")
        .expect("the destination should validate");

    let rejected = root
        .copy(&source, &destination, copy::Options::new())
        .expect_err("the conservative policy should reject a link destination");
    assert_eq!(std::io::ErrorKind::AlreadyExists, rejected.kind());

    let statistics = root
        .copy(
            &source,
            &destination,
            copy::Options::new()
                .with_type_conflict(copy::TypeConflictPolicy::Replace),
        )
        .expect("replacement should remove the link itself");

    assert_eq!(1, statistics.files());
    assert_eq!(1, statistics.overwritten());
    assert!(
        !std::fs::symlink_metadata(temp.path().join("destination"))
            .expect("the replacement should exist")
            .file_type()
            .is_symlink(),
    );
    assert_eq!(
        b"replacement",
        std::fs::read(temp.path().join("destination"))
            .expect("the replacement should be readable")
            .as_slice(),
    );
    assert_eq!(
        b"unchanged",
        std::fs::read(temp.path().join("outside-target"))
            .expect("the link target should remain unchanged")
            .as_slice(),
    );
}

/// Verifies rooted copying rejects Unix socket sources as unsupported entries.
#[cfg(unix)]
#[test]
fn test_copy_rejects_unix_socket_source() {
    use std::os::unix::net::UnixListener;

    let temp = tempfile::tempdir().expect("a temporary root should exist");
    let socket_path = temp.path().join("socket");
    let _listener = UnixListener::bind(&socket_path)
        .expect("the Unix socket source should be created");
    let root = rooted::Root::open(temp.path()).expect("the root should open");
    let source =
        rooted::Path::new("socket").expect("the source should validate");
    let destination = rooted::Path::new("destination")
        .expect("the destination should validate");

    let error = root
        .copy(&source, &destination, copy::Options::new())
        .expect_err("socket sources must be rejected");

    assert_eq!(std::io::ErrorKind::Unsupported, error.kind());
    assert!(!temp.path().join("destination").exists());
}

/// Verifies a native source-open failure is retained as source-inspection
/// context before a destination can be published.
#[cfg(all(coverage, any(unix, windows)))]
#[test]
fn test_copy_reports_source_open_fault_before_publication() {
    const TEST_NAME: &str = "rooted::copy_tests::test_copy_reports_source_open_fault_before_publication";
    run_in_coverage_fault_process(TEST_NAME, "rooted-copy-source-open", || {
        let temp = tempfile::tempdir().expect("a temporary root should exist");
        std::fs::write(temp.path().join("source"), b"payload")
            .expect("the source should be written");
        let root =
            rooted::Root::open(temp.path()).expect("the root should open");
        let source =
            rooted::Path::new("source").expect("the source should validate");
        let destination = rooted::Path::new("destination")
            .expect("the destination should validate");

        let error = root
            .copy(&source, &destination, copy::Options::new())
            .expect_err("the injected source open should fail the copy");

        assert_eq!(std::io::ErrorKind::PermissionDenied, error.kind());
        assert_eq!(copy::Stage::InspectSourceEntry, error.stage());
        assert!(!temp.path().join("destination").exists());
    });
}

/// Verifies a destination metadata failure is classified before staging a
/// replacement file.
#[cfg(all(coverage, any(unix, windows)))]
#[test]
fn test_copy_reports_destination_metadata_fault_before_publication() {
    const TEST_NAME: &str = "rooted::copy_tests::test_copy_reports_destination_metadata_fault_before_publication";
    run_in_coverage_fault_process(
        TEST_NAME,
        "rooted-copy-destination-metadata",
        || {
            let temp =
                tempfile::tempdir().expect("a temporary root should exist");
            std::fs::write(temp.path().join("source"), b"payload")
                .expect("the source should be written");
            let root =
                rooted::Root::open(temp.path()).expect("the root should open");
            let source = rooted::Path::new("source")
                .expect("the source should validate");
            let destination = rooted::Path::new("destination")
                .expect("the destination should validate");

            let error = root
                .copy(&source, &destination, copy::Options::new())
                .expect_err("the injected destination inspection should fail");

            assert_eq!(std::io::ErrorKind::PermissionDenied, error.kind());
            assert_eq!(copy::Stage::PrepareDestination, error.stage());
            assert!(!temp.path().join("destination").exists());
        },
    );
}

/// Verifies a directory enumeration failure retains read-source-directory
/// context without publishing a descendant.
#[cfg(all(coverage, any(unix, windows)))]
#[test]
fn test_copy_reports_directory_read_fault_before_descendant_publication() {
    const TEST_NAME: &str = "rooted::copy_tests::test_copy_reports_directory_read_fault_before_descendant_publication";
    run_in_coverage_fault_process(
        TEST_NAME,
        "rooted-copy-directory-read",
        || {
            let temp =
                tempfile::tempdir().expect("a temporary root should exist");
            std::fs::create_dir(temp.path().join("source"))
                .expect("the source directory should exist");
            std::fs::write(temp.path().join("source/value"), b"payload")
                .expect("the source child should be written");
            let root =
                rooted::Root::open(temp.path()).expect("the root should open");
            let source = rooted::Path::new("source")
                .expect("the source should validate");
            let destination = rooted::Path::new("destination")
                .expect("the destination should validate");

            let error = root
                .copy(&source, &destination, copy::Options::new())
                .expect_err("the injected directory read should fail");

            assert_eq!(std::io::ErrorKind::PermissionDenied, error.kind());
            assert_eq!(copy::Stage::ReadSourceDirectory, error.stage());
            assert!(temp.path().join("destination").is_dir());
            assert!(!temp.path().join("destination/value").exists());
        },
    );
}

/// Verifies a permission-preservation failure is reported after contents have
/// been atomically published.
#[cfg(all(coverage, unix))]
#[test]
fn test_copy_reports_permission_preservation_fault_after_publication() {
    const TEST_NAME: &str = "rooted::copy_tests::test_copy_reports_permission_preservation_fault_after_publication";
    run_in_coverage_fault_process(
        TEST_NAME,
        "rooted-copy-set-permissions",
        || {
            let temp =
                tempfile::tempdir().expect("a temporary root should exist");
            std::fs::write(temp.path().join("source"), b"payload")
                .expect("the source should be written");
            let root =
                rooted::Root::open(temp.path()).expect("the root should open");
            let source = rooted::Path::new("source")
                .expect("the source should validate");
            let destination = rooted::Path::new("destination")
                .expect("the destination should validate");

            let error = root
                .copy(
                    &source,
                    &destination,
                    copy::Options::new().preserve_permissions(),
                )
                .expect_err("the injected permission update should fail");

            assert_eq!(std::io::ErrorKind::PermissionDenied, error.kind());
            assert_eq!(copy::Stage::PreservePermissions, error.stage());
            assert_eq!(
                b"payload",
                std::fs::read(temp.path().join("destination"))
                    .expect("the completed destination should remain readable")
                    .as_slice(),
            );
        },
    );
}

/// Verifies destination-directory creation failures retain preparation
/// context and do not begin traversal.
#[cfg(all(coverage, any(unix, windows)))]
#[test]
fn test_copy_reports_destination_directory_creation_fault() {
    const TEST_NAME: &str = "rooted::copy_tests::test_copy_reports_destination_directory_creation_fault";
    run_in_coverage_fault_process(
        TEST_NAME,
        "rooted-copy-directory-create",
        || {
            let temp =
                tempfile::tempdir().expect("a temporary root should exist");
            std::fs::create_dir(temp.path().join("source"))
                .expect("the source directory should exist");
            let root =
                rooted::Root::open(temp.path()).expect("the root should open");
            let source = rooted::Path::new("source")
                .expect("the source should validate");
            let destination = rooted::Path::new("destination")
                .expect("the destination should validate");

            let error = root
                .copy(&source, &destination, copy::Options::new())
                .expect_err("the injected directory create should fail");

            assert_eq!(std::io::ErrorKind::PermissionDenied, error.kind());
            assert_eq!(copy::Stage::PrepareDestination, error.stage());
            assert!(!temp.path().join("destination").exists());
        },
    );
}

/// Verifies staging-writer creation failures are classified as destination
/// preparation failures before content copying starts.
#[cfg(all(coverage, any(unix, windows)))]
#[test]
fn test_copy_reports_staging_writer_creation_fault() {
    const TEST_NAME: &str =
        "rooted::copy_tests::test_copy_reports_staging_writer_creation_fault";
    run_in_coverage_fault_process(TEST_NAME, "rooted-copy-writer-open", || {
        let temp = tempfile::tempdir().expect("a temporary root should exist");
        std::fs::write(temp.path().join("source"), b"payload")
            .expect("the source should be written");
        let root =
            rooted::Root::open(temp.path()).expect("the root should open");
        let source =
            rooted::Path::new("source").expect("the source should validate");
        let destination = rooted::Path::new("destination")
            .expect("the destination should validate");

        let error = root
            .copy(&source, &destination, copy::Options::new())
            .expect_err("the injected staging writer creation should fail");

        assert_eq!(std::io::ErrorKind::PermissionDenied, error.kind());
        assert_eq!(copy::Stage::PrepareDestination, error.stage());
        assert!(!temp.path().join("destination").exists());
    });
}

/// Verifies a later directory child-copy failure preserves one successfully
/// published child and reports the file-content stage.
#[cfg(all(coverage, any(unix, windows)))]
#[test]
fn test_copy_reports_second_file_copy_fault_after_partial_publication() {
    const TEST_NAME: &str = "rooted::copy_tests::test_copy_reports_second_file_copy_fault_after_partial_publication";
    run_in_coverage_fault_process(TEST_NAME, "rooted-copy-file-second", || {
        let temp = tempfile::tempdir().expect("a temporary root should exist");
        std::fs::create_dir(temp.path().join("source"))
            .expect("the source directory should exist");
        std::fs::write(temp.path().join("source/first"), b"first")
            .expect("the first source file should be written");
        std::fs::write(temp.path().join("source/second"), b"second")
            .expect("the second source file should be written");
        let root =
            rooted::Root::open(temp.path()).expect("the root should open");
        let source =
            rooted::Path::new("source").expect("the source should validate");
        let destination = rooted::Path::new("destination")
            .expect("the destination should validate");

        let error = root
            .copy(&source, &destination, copy::Options::new())
            .expect_err("the injected second file copy should fail");

        assert_eq!(Some(libc::EIO), error.error().raw_os_error());
        assert_eq!(copy::Stage::CopyFileContents, error.stage());
        let published_children =
            std::fs::read_dir(temp.path().join("destination"))
                .expect("the destination directory should remain published")
                .collect::<Result<Vec<_>, _>>()
                .expect("the published destination should remain readable");
        assert_eq!(1, published_children.len());
    });
}

/// Verifies a descriptor metadata failure while opening a source file retains
/// source-entry inspection context before destination publication.
#[cfg(all(coverage, any(unix, windows)))]
#[test]
fn test_copy_reports_opened_file_metadata_fault_before_publication() {
    const TEST_NAME: &str = "rooted::copy_tests::test_copy_reports_opened_file_metadata_fault_before_publication";
    run_in_coverage_fault_process(TEST_NAME, "rooted-file-metadata", || {
        let temp = tempfile::tempdir().expect("a temporary root should exist");
        std::fs::write(temp.path().join("source"), b"payload")
            .expect("the source should be written");
        let root =
            rooted::Root::open(temp.path()).expect("the root should open");
        let source =
            rooted::Path::new("source").expect("the source should validate");
        let destination = rooted::Path::new("destination")
            .expect("the destination should validate");

        let error = root
            .copy(&source, &destination, copy::Options::new())
            .expect_err("the injected source metadata inspection should fail");

        assert_eq!(copy::Stage::InspectSourceEntry, error.stage());
        assert!(!temp.path().join("destination").exists());
    });
}

/// Verifies a native source-open failure reaches the copy operation's source
/// error mapping boundary.
#[cfg(all(coverage, any(unix, windows)))]
#[test]
fn test_copy_reports_native_source_open_fault_before_publication() {
    const TEST_NAME: &str = "rooted::copy_tests::test_copy_reports_native_source_open_fault_before_publication";
    run_in_coverage_fault_process(
        TEST_NAME,
        "rooted-copy-source-open-native",
        || {
            let temp =
                tempfile::tempdir().expect("a temporary root should exist");
            std::fs::write(temp.path().join("source"), b"payload")
                .expect("the source should be written");
            let root =
                rooted::Root::open(temp.path()).expect("the root should open");
            let source = rooted::Path::new("source")
                .expect("the source should validate");
            let destination = rooted::Path::new("destination")
                .expect("the destination should validate");

            let error = root
                .copy(&source, &destination, copy::Options::new())
                .expect_err("the injected native source open should fail");

            assert_eq!(copy::Stage::InspectSourceEntry, error.stage());
            assert!(!temp.path().join("destination").exists());
        },
    );
}

/// Verifies an I/O failure while inspecting the final destination entry is
/// mapped to the destination-preparation stage.
#[cfg(all(coverage, any(unix, windows)))]
#[test]
fn test_copy_reports_native_destination_metadata_fault_before_publication() {
    const TEST_NAME: &str = "rooted::copy_tests::test_copy_reports_native_destination_metadata_fault_before_publication";
    run_in_coverage_fault_process(
        TEST_NAME,
        "rooted-copy-destination-metadata-native",
        || {
            let temp =
                tempfile::tempdir().expect("a temporary root should exist");
            std::fs::write(temp.path().join("source"), b"payload")
                .expect("the source should be written");
            let root =
                rooted::Root::open(temp.path()).expect("the root should open");
            let source = rooted::Path::new("source")
                .expect("the source should validate");
            let destination = rooted::Path::new("destination")
                .expect("the destination should validate");

            let error = root
                .copy(&source, &destination, copy::Options::new())
                .expect_err("the injected destination metadata should fail");

            assert_eq!(copy::Stage::PrepareDestination, error.stage());
            assert!(!temp.path().join("destination").exists());
        },
    );
}

/// Verifies a native source-directory enumeration failure retains the
/// directory-read stage after destination creation.
#[cfg(all(coverage, any(unix, windows)))]
#[test]
fn test_copy_reports_native_directory_read_fault_after_creation() {
    const TEST_NAME: &str = "rooted::copy_tests::test_copy_reports_native_directory_read_fault_after_creation";
    run_in_coverage_fault_process(
        TEST_NAME,
        "rooted-copy-directory-read-native",
        || {
            let temp =
                tempfile::tempdir().expect("a temporary root should exist");
            std::fs::create_dir(temp.path().join("source"))
                .expect("the source directory should exist");
            let root =
                rooted::Root::open(temp.path()).expect("the root should open");
            let source = rooted::Path::new("source")
                .expect("the source should validate");
            let destination = rooted::Path::new("destination")
                .expect("the destination should validate");

            let error = root
                .copy(&source, &destination, copy::Options::new())
                .expect_err("the injected directory read should fail");

            assert_eq!(copy::Stage::ReadSourceDirectory, error.stage());
            assert!(temp.path().join("destination").is_dir());
        },
    );
}

/// Verifies a native destination-directory creation failure is mapped at the
/// preparation boundary without publishing a target.
#[cfg(all(coverage, any(unix, windows)))]
#[test]
fn test_copy_reports_native_directory_create_fault_before_publication() {
    const TEST_NAME: &str = "rooted::copy_tests::test_copy_reports_native_directory_create_fault_before_publication";
    run_in_coverage_fault_process(
        TEST_NAME,
        "rooted-copy-directory-create-native",
        || {
            let temp =
                tempfile::tempdir().expect("a temporary root should exist");
            std::fs::create_dir(temp.path().join("source"))
                .expect("the source directory should exist");
            let root =
                rooted::Root::open(temp.path()).expect("the root should open");
            let source = rooted::Path::new("source")
                .expect("the source should validate");
            let destination = rooted::Path::new("destination")
                .expect("the destination should validate");

            let error = root
                .copy(&source, &destination, copy::Options::new())
                .expect_err("the injected directory creation should fail");

            assert_eq!(copy::Stage::PrepareDestination, error.stage());
            assert!(!temp.path().join("destination").exists());
        },
    );
}

/// Verifies replacement of a non-directory destination propagates a native
/// removal failure with destination-preparation context.
#[cfg(all(coverage, unix))]
#[test]
fn test_copy_reports_native_file_removal_fault_during_replacement() {
    use std::os::unix::fs::symlink;

    const TEST_NAME: &str = "rooted::copy_tests::test_copy_reports_native_file_removal_fault_during_replacement";
    run_in_coverage_fault_process(
        TEST_NAME,
        "rooted-copy-remove-file-native",
        || {
            let temp =
                tempfile::tempdir().expect("a temporary root should exist");
            std::fs::write(temp.path().join("source"), b"payload")
                .expect("the source should be written");
            symlink("source", temp.path().join("destination"))
                .expect("the destination link should be created");
            let root =
                rooted::Root::open(temp.path()).expect("the root should open");
            let source = rooted::Path::new("source")
                .expect("the source should validate");
            let destination = rooted::Path::new("destination")
                .expect("the destination should validate");

            let error = root
                .copy(
                    &source,
                    &destination,
                    copy::Options::new()
                        .with_type_conflict(copy::TypeConflictPolicy::Replace),
                )
                .expect_err("the injected destination removal should fail");

            assert_eq!(copy::Stage::PrepareDestination, error.stage());
            assert!(
                std::fs::symlink_metadata(temp.path().join("destination"))
                    .expect("the destination link should remain")
                    .file_type()
                    .is_symlink()
            );
        },
    );
}

/// Verifies replacement of a destination directory propagates a native tree
/// removal failure with destination-preparation context.
#[cfg(all(coverage, any(unix, windows)))]
#[test]
fn test_copy_reports_native_tree_removal_fault_during_replacement() {
    const TEST_NAME: &str = "rooted::copy_tests::test_copy_reports_native_tree_removal_fault_during_replacement";
    run_in_coverage_fault_process(
        TEST_NAME,
        "rooted-copy-remove-tree-native",
        || {
            let temp =
                tempfile::tempdir().expect("a temporary root should exist");
            std::fs::write(temp.path().join("source"), b"payload")
                .expect("the source should be written");
            std::fs::create_dir(temp.path().join("destination"))
                .expect("the destination directory should exist");
            let root =
                rooted::Root::open(temp.path()).expect("the root should open");
            let source = rooted::Path::new("source")
                .expect("the source should validate");
            let destination = rooted::Path::new("destination")
                .expect("the destination should validate");

            let error = root
                .copy(
                    &source,
                    &destination,
                    copy::Options::new()
                        .with_type_conflict(copy::TypeConflictPolicy::Replace),
                )
                .expect_err(
                    "the injected destination tree removal should fail",
                );

            assert_eq!(copy::Stage::PrepareDestination, error.stage());
            assert!(temp.path().join("destination").is_dir());
        },
    );
}

/// Verifies a native permission-update failure is mapped after a file has been
/// installed and before the copy reports success.
#[cfg(all(coverage, unix))]
#[test]
fn test_copy_reports_native_permission_update_fault_after_publication() {
    const TEST_NAME: &str = "rooted::copy_tests::test_copy_reports_native_permission_update_fault_after_publication";
    run_in_coverage_fault_process(
        TEST_NAME,
        "rooted-copy-set-permissions-native",
        || {
            let temp =
                tempfile::tempdir().expect("a temporary root should exist");
            std::fs::write(temp.path().join("source"), b"payload")
                .expect("the source should be written");
            let root =
                rooted::Root::open(temp.path()).expect("the root should open");
            let source = rooted::Path::new("source")
                .expect("the source should validate");
            let destination = rooted::Path::new("destination")
                .expect("the destination should validate");

            let error = root
                .copy(
                    &source,
                    &destination,
                    copy::Options::new().preserve_permissions(),
                )
                .expect_err("the injected permission update should fail");

            assert_eq!(copy::Stage::PreservePermissions, error.stage());
            assert_eq!(
                b"payload",
                std::fs::read(temp.path().join("destination"))
                    .expect("the destination should remain published")
                    .as_slice(),
            );
        },
    );
}

/// Verifies source metadata extraction errors are reported before destination
/// publication.
#[cfg(all(coverage, any(unix, windows)))]
#[test]
fn test_copy_reports_source_metadata_extraction_fault() {
    const TEST_NAME: &str = "rooted::copy_tests::test_copy_reports_source_metadata_extraction_fault";
    run_in_coverage_fault_process(
        TEST_NAME,
        "rooted-copy-source-metadata-native",
        || {
            let temp =
                tempfile::tempdir().expect("a temporary root should exist");
            std::fs::write(temp.path().join("source"), b"payload")
                .expect("the source should be written");
            let root =
                rooted::Root::open(temp.path()).expect("the root should open");
            let source = rooted::Path::new("source")
                .expect("the source should validate");
            let destination = rooted::Path::new("destination")
                .expect("the destination should validate");

            let error = root
                .copy(&source, &destination, copy::Options::new())
                .expect_err(
                    "the injected source metadata extraction should fail",
                );

            assert_eq!(copy::Stage::InspectSourceEntry, error.stage());
            assert!(!temp.path().join("destination").exists());
        },
    );
}

/// Verifies a native read-to-writer failure is reported at the file-content
/// stage without publishing a destination.
#[cfg(all(coverage, any(unix, windows)))]
#[test]
fn test_copy_reports_file_contents_transfer_fault() {
    const TEST_NAME: &str =
        "rooted::copy_tests::test_copy_reports_file_contents_transfer_fault";
    run_in_coverage_fault_process(
        TEST_NAME,
        "rooted-copy-file-contents-native",
        || {
            let temp =
                tempfile::tempdir().expect("a temporary root should exist");
            std::fs::write(temp.path().join("source"), b"payload")
                .expect("the source should be written");
            let root =
                rooted::Root::open(temp.path()).expect("the root should open");
            let source = rooted::Path::new("source")
                .expect("the source should validate");
            let destination = rooted::Path::new("destination")
                .expect("the destination should validate");

            let error = root
                .copy(&source, &destination, copy::Options::new())
                .expect_err("the injected content transfer should fail");

            assert_eq!(copy::Stage::CopyFileContents, error.stage());
            assert!(!temp.path().join("destination").exists());
        },
    );
}

/// Verifies atomic commit errors retain the copy commit stage and do not
/// publish a destination.
#[cfg(all(coverage, any(unix, windows)))]
#[test]
fn test_copy_reports_atomic_commit_fault_before_publication() {
    const TEST_NAME: &str = "rooted::copy_tests::test_copy_reports_atomic_commit_fault_before_publication";
    run_in_coverage_fault_process(
        TEST_NAME,
        "rooted-copy-file-commit-native",
        || {
            let temp =
                tempfile::tempdir().expect("a temporary root should exist");
            std::fs::write(temp.path().join("source"), b"payload")
                .expect("the source should be written");
            let root =
                rooted::Root::open(temp.path()).expect("the root should open");
            let source = rooted::Path::new("source")
                .expect("the source should validate");
            let destination = rooted::Path::new("destination")
                .expect("the destination should validate");

            let error = root
                .copy(&source, &destination, copy::Options::new())
                .expect_err("the injected atomic commit should fail");

            assert_eq!(copy::Stage::CommitFile, error.stage());
            assert!(!temp.path().join("destination").exists());
        },
    );
}

/// Verifies statistics overflow protection returns its dedicated update stage
/// before a completed copy can be reported.
#[cfg(all(coverage, any(unix, windows)))]
#[test]
fn test_copy_reports_statistics_overflow_fault() {
    const TEST_NAME: &str =
        "rooted::copy_tests::test_copy_reports_statistics_overflow_fault";
    run_in_coverage_fault_process(
        TEST_NAME,
        "rooted-copy-statistics-overflow",
        || {
            let temp =
                tempfile::tempdir().expect("a temporary root should exist");
            std::fs::write(temp.path().join("source"), b"payload")
                .expect("the source should be written");
            let root =
                rooted::Root::open(temp.path()).expect("the root should open");
            let source = rooted::Path::new("source")
                .expect("the source should validate");
            let destination = rooted::Path::new("destination")
                .expect("the destination should validate");

            let error = root
                .copy(&source, &destination, copy::Options::new())
                .expect_err("the injected statistics overflow should fail");

            assert_eq!(copy::Stage::UpdateStatistics, error.stage());
            assert!(temp.path().join("destination").exists());
        },
    );
}

/// Verifies directory-copy type conflicts retain destination-preparation
/// context when replacement was not requested.
#[cfg(all(coverage, any(unix, windows)))]
#[test]
fn test_copy_directory_rejects_file_destination_without_replacement() {
    let temp = tempfile::tempdir().expect("a temporary root should exist");
    std::fs::create_dir(temp.path().join("source"))
        .expect("the source directory should exist");
    std::fs::write(temp.path().join("destination"), b"old")
        .expect("the destination file should exist");
    let root = rooted::Root::open(temp.path()).expect("the root should open");
    let source =
        rooted::Path::new("source").expect("the source should validate");
    let destination = rooted::Path::new("destination")
        .expect("the destination should validate");

    let error = root
        .copy(&source, &destination, copy::Options::new())
        .expect_err("a directory must not replace a file by default");

    assert_eq!(std::io::ErrorKind::AlreadyExists, error.kind());
    assert_eq!(copy::Stage::PrepareDestination, error.stage());
}

/// Verifies directory replacement maps native removal failures before the
/// replacement directory is created.
#[cfg(all(coverage, any(unix, windows)))]
#[test]
fn test_copy_directory_reports_native_file_removal_fault_during_replacement() {
    const TEST_NAME: &str = "rooted::copy_tests::test_copy_directory_reports_native_file_removal_fault_during_replacement";
    run_in_coverage_fault_process(
        TEST_NAME,
        "rooted-copy-remove-file-native",
        || {
            let temp =
                tempfile::tempdir().expect("a temporary root should exist");
            std::fs::create_dir(temp.path().join("source"))
                .expect("the source directory should exist");
            std::fs::write(temp.path().join("destination"), b"old")
                .expect("the destination file should exist");
            let root =
                rooted::Root::open(temp.path()).expect("the root should open");
            let source = rooted::Path::new("source")
                .expect("the source should validate");
            let destination = rooted::Path::new("destination")
                .expect("the destination should validate");

            let error = root
                .copy(
                    &source,
                    &destination,
                    copy::Options::new()
                        .with_type_conflict(copy::TypeConflictPolicy::Replace),
                )
                .expect_err("the injected destination removal should fail");

            assert_eq!(copy::Stage::PrepareDestination, error.stage());
            assert_eq!(
                b"old",
                std::fs::read(temp.path().join("destination"))
                    .expect("the destination file should remain")
                    .as_slice(),
            );
        },
    );
}

/// Verifies directory replacement maps native directory-creation failures
/// after removal of the conflicting file.
#[cfg(all(coverage, any(unix, windows)))]
#[test]
fn test_copy_directory_reports_native_create_fault_after_replacement_removal() {
    const TEST_NAME: &str = "rooted::copy_tests::test_copy_directory_reports_native_create_fault_after_replacement_removal";
    run_in_coverage_fault_process(
        TEST_NAME,
        "rooted-copy-directory-create-native",
        || {
            let temp =
                tempfile::tempdir().expect("a temporary root should exist");
            std::fs::create_dir(temp.path().join("source"))
                .expect("the source directory should exist");
            std::fs::write(temp.path().join("destination"), b"old")
                .expect("the destination file should exist");
            let root =
                rooted::Root::open(temp.path()).expect("the root should open");
            let source = rooted::Path::new("source")
                .expect("the source should validate");
            let destination = rooted::Path::new("destination")
                .expect("the destination should validate");

            let error = root
                .copy(
                    &source,
                    &destination,
                    copy::Options::new()
                        .with_type_conflict(copy::TypeConflictPolicy::Replace),
                )
                .expect_err(
                    "the injected replacement directory creation should fail",
                );

            assert_eq!(copy::Stage::PrepareDestination, error.stage());
            assert!(!temp.path().join("destination").exists());
        },
    );
}

/// Verifies atomic-writer creation errors retain destination-preparation
/// context when a requested parent directory does not exist.
#[cfg(all(coverage, any(unix, windows)))]
#[test]
fn test_copy_reports_missing_destination_parent_before_staging() {
    let temp = tempfile::tempdir().expect("a temporary root should exist");
    std::fs::write(temp.path().join("source"), b"payload")
        .expect("the source should be written");
    let root = rooted::Root::open(temp.path()).expect("the root should open");
    let source =
        rooted::Path::new("source").expect("the source should validate");
    let destination = rooted::Path::new("missing/destination")
        .expect("the destination should validate");

    let error = root
        .copy(&source, &destination, copy::Options::new())
        .expect_err("a missing destination parent should reject staging");

    assert_eq!(copy::Stage::PrepareDestination, error.stage());
    assert!(!temp.path().join("missing/destination").exists());
}

/// Verifies a directory-finalization permission failure retains the completed
/// destination directory and its preservation-stage context.
#[cfg(all(coverage, unix))]
#[test]
fn test_copy_reports_directory_permission_preservation_fault_after_creation() {
    const TEST_NAME: &str = "rooted::copy_tests::test_copy_reports_directory_permission_preservation_fault_after_creation";
    run_in_coverage_fault_process(
        TEST_NAME,
        "rooted-copy-set-permissions",
        || {
            let temp =
                tempfile::tempdir().expect("a temporary root should exist");
            std::fs::create_dir(temp.path().join("source"))
                .expect("the empty source directory should exist");
            let root =
                rooted::Root::open(temp.path()).expect("the root should open");
            let source = rooted::Path::new("source")
                .expect("the source should validate");
            let destination = rooted::Path::new("destination")
                .expect("the destination should validate");

            let error = root
                .copy(
                    &source,
                    &destination,
                    copy::Options::new().preserve_permissions(),
                )
                .expect_err(
                    "the injected directory permission update should fail",
                );

            assert_eq!(std::io::ErrorKind::PermissionDenied, error.kind());
            assert_eq!(copy::Stage::PreservePermissions, error.stage());
            assert!(temp.path().join("destination").is_dir());
        },
    );
}
