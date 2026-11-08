// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
#![cfg(unix)]

use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::path::PathBuf;

use qubit_local_files::LocalFileSystem;
use qubit_local_files::error::LocalFileErrorKind;
use qubit_local_files::options::LocalCopyConflictPolicy;
use qubit_local_files::options::LocalCopyOptions;
use qubit_local_files::options::LocalCopyTypeConflictPolicy;
use qubit_local_files::options::LocalMetadataPreservePolicy;
use qubit_local_files::outcome::LocalCopyFailureState;
use tempfile::tempdir;

fn set_mode(path: &Path, mode: u32) {
    fs::set_permissions(path, fs::Permissions::from_mode(mode)).expect("set permissions");
}

fn mode(path: &Path) -> u32 {
    fs::metadata(path).expect("metadata").permissions().mode() & 0o777
}

fn tree_permissions_options() -> LocalCopyOptions {
    LocalCopyOptions::new()
        .with_tree_source()
        .with_conflict(LocalCopyConflictPolicy::Overwrite)
        .with_type_conflict(LocalCopyTypeConflictPolicy::Fail)
        .with_metadata_preservation(LocalMetadataPreservePolicy::Permissions)
}

fn rooted_path(rooted: bool, absolute: &Path, relative: &str) -> PathBuf {
    if rooted {
        PathBuf::from(relative)
    } else {
        absolute.to_path_buf()
    }
}

#[test]
fn test_empty_directory_merge_reports_applied_permissions_for_both_backends() {
    for rooted in [false, true] {
        let fixture = tempdir().expect("fixture");
        let source = fixture.path().join("source");
        let target = fixture.path().join("target");
        fs::create_dir(&source).expect("source directory");
        fs::create_dir(&target).expect("target directory");
        set_mode(&source, 0o500);
        set_mode(&target, 0o700);
        let filesystem = if rooted {
            LocalFileSystem::rooted(fixture.path()).expect("rooted filesystem")
        } else {
            LocalFileSystem::host().expect("host filesystem")
        };
        let source_path = rooted_path(rooted, &source, "source");
        let target_path = rooted_path(rooted, &target, "target");

        let outcome = filesystem
            .copy_with_options(&source_path, &target_path, &tree_permissions_options())
            .expect("empty directory merge");

        assert_eq!(0, outcome.stats().files());
        assert_eq!(0, outcome.stats().directories());
        assert_eq!(1, outcome.stats().overwritten());
        assert_eq!(0o500, mode(&target));
        assert_eq!(
            LocalMetadataPreservePolicy::Permissions,
            outcome.metadata_preservation()
        );
    }
}

#[test]
fn test_all_leaf_conflicts_skipped_still_report_directory_permissions() {
    for rooted in [false, true] {
        let fixture = tempdir().expect("fixture");
        let source = fixture.path().join("source");
        let target = fixture.path().join("target");
        fs::create_dir(&source).expect("source directory");
        fs::create_dir(&target).expect("target directory");
        fs::write(source.join("leaf"), b"source").expect("source leaf");
        fs::write(target.join("leaf"), b"existing").expect("target leaf");
        set_mode(&source, 0o500);
        set_mode(&target, 0o700);
        let filesystem = if rooted {
            LocalFileSystem::rooted(fixture.path()).expect("rooted filesystem")
        } else {
            LocalFileSystem::host().expect("host filesystem")
        };
        let options = tree_permissions_options()
            .with_conflict(LocalCopyConflictPolicy::Skip)
            .with_type_conflict(LocalCopyTypeConflictPolicy::Skip);
        let source_path = rooted_path(rooted, &source, "source");
        let target_path = rooted_path(rooted, &target, "target");

        let outcome = filesystem
            .copy_with_options(&source_path, &target_path, &options)
            .expect("leaf conflict should be skipped");

        assert_eq!(1, outcome.stats().skipped());
        assert_eq!(0, outcome.stats().files());
        assert_eq!(0, outcome.stats().directories());
        assert_eq!(0, outcome.stats().overwritten());
        assert_eq!(b"existing", fs::read(target.join("leaf")).unwrap().as_slice());
        assert_eq!(0o500, mode(&target));
        assert_eq!(
            LocalMetadataPreservePolicy::Permissions,
            outcome.metadata_preservation()
        );
    }
}

#[test]
fn test_directory_permissions_remain_unchanged_when_not_requested() {
    for rooted in [false, true] {
        let fixture = tempdir().expect("fixture");
        let source = fixture.path().join("source");
        let target = fixture.path().join("target");
        fs::create_dir(&source).expect("source directory");
        fs::create_dir(&target).expect("target directory");
        set_mode(&source, 0o500);
        set_mode(&target, 0o700);
        let filesystem = if rooted {
            LocalFileSystem::rooted(fixture.path()).expect("rooted filesystem")
        } else {
            LocalFileSystem::host().expect("host filesystem")
        };
        let options = tree_permissions_options().with_metadata_preservation(LocalMetadataPreservePolicy::None);

        let outcome = filesystem
            .copy_with_options(
                &rooted_path(rooted, &source, "source"),
                &rooted_path(rooted, &target, "target"),
                &options,
            )
            .expect("empty directory merge");

        assert_eq!(0o700, mode(&target));
        assert_eq!(LocalMetadataPreservePolicy::None, outcome.metadata_preservation());
    }
}

#[test]
fn test_later_depth_failure_preserves_earlier_directory_permission_effect() {
    for rooted in [false, true] {
        let fixture = tempdir().expect("fixture");
        let source = fixture.path().join("source");
        let target = fixture.path().join("target");
        fs::create_dir_all(source.join("first")).expect("first source directory");
        fs::create_dir_all(source.join("second")).expect("second source directory");
        fs::create_dir_all(target.join("first")).expect("first target directory");
        fs::create_dir_all(target.join("second")).expect("second target directory");
        let names = fs::read_dir(&source)
            .expect("read source order")
            .map(|entry| entry.expect("directory entry").file_name())
            .collect::<Vec<_>>();
        assert_eq!(2, names.len());
        let empty_name = names[0].clone();
        let deep_name = names[1].clone();
        fs::create_dir(source.join(&deep_name).join("deep")).expect("deep child");
        fs::set_permissions(source.join("first"), fs::Permissions::from_mode(0o500)).expect("first source mode");
        fs::set_permissions(source.join("second"), fs::Permissions::from_mode(0o500)).expect("second source mode");
        fs::set_permissions(target.join("first"), fs::Permissions::from_mode(0o700)).expect("first target mode");
        fs::set_permissions(target.join("second"), fs::Permissions::from_mode(0o700)).expect("second target mode");
        assert_eq!(
            names,
            fs::read_dir(&source)
                .unwrap()
                .map(|entry| entry.unwrap().file_name())
                .collect::<Vec<_>>()
        );

        // Keep the empty directory first in the actual native enumeration order.
        assert_eq!(
            empty_name,
            fs::read_dir(&source).unwrap().next().unwrap().unwrap().file_name()
        );
        assert_eq!(
            deep_name,
            fs::read_dir(&source).unwrap().nth(1).unwrap().unwrap().file_name()
        );

        let filesystem = if rooted {
            LocalFileSystem::rooted(fixture.path()).expect("rooted filesystem")
        } else {
            LocalFileSystem::host().expect("host filesystem")
        };
        let source_path = rooted_path(rooted, &source, "source");
        let target_path = rooted_path(rooted, &target, "target");
        let options = tree_permissions_options()
            .with_conflict(LocalCopyConflictPolicy::Fail)
            .with_max_depth(1);
        let failure = filesystem
            .copy_with_options(&source_path, &target_path, &options)
            .expect_err("deep descendant exceeds the configured depth");

        assert_eq!(LocalFileErrorKind::ResourceLimit, failure.error().kind());
        assert_eq!(LocalCopyFailureState::PartiallyPublished, failure.state());
        assert_eq!(0, failure.partial_stats().files());
        assert_eq!(0, failure.partial_stats().directories());
        assert_eq!(0, failure.partial_stats().overwritten());
        assert_eq!(0o500, mode(&target.join(empty_name)));
    }
}

#[cfg(feature = "test-support")]
#[test]
fn test_rooted_permission_failure_keeps_metadata_effect_indeterminate() {
    let fixture = tempdir().expect("fixture");
    let source = fixture.path().join("source");
    let target = fixture.path().join("target");
    fs::create_dir(&source).expect("source directory");
    fs::create_dir(&target).expect("target directory");
    set_mode(&source, 0o500);
    set_mode(&target, 0o700);
    let filesystem = LocalFileSystem::rooted(fixture.path()).expect("rooted filesystem");
    let _fault =
        qubit_local_files::test_support::install_test_fault("rooted-copy-set-permissions").expect("permission fault");

    let failure = filesystem
        .copy_with_options(
            Path::new("source"),
            Path::new("target"),
            &tree_permissions_options().with_conflict(LocalCopyConflictPolicy::Fail),
        )
        .expect_err("injected permission failure");

    assert_eq!(LocalFileErrorKind::PermissionDenied, failure.error().kind());
    assert_eq!(LocalCopyFailureState::Indeterminate, failure.state());
    assert_eq!(0, failure.partial_stats().files());
    assert_eq!(0, failure.partial_stats().directories());
    assert_eq!(0, failure.partial_stats().overwritten());
    assert_eq!(0o700, mode(&target));
}
