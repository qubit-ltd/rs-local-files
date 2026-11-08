// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::path::PathBuf;

use qubit_local_files::LocalFileSystem;
use qubit_local_files::error::LocalFileErrorKind;
use qubit_local_files::options::LocalCopyConflictPolicy;
use qubit_local_files::options::LocalCopyOptions;
use qubit_local_files::options::LocalCopyTypeConflictPolicy;
use qubit_local_files::outcome::LocalCopyFailureState;
use tempfile::tempdir;

fn overlap_options() -> LocalCopyOptions {
    LocalCopyOptions::new()
        .with_tree_source()
        .with_type_conflict(LocalCopyTypeConflictPolicy::Replace)
        .with_conflict(LocalCopyConflictPolicy::Overwrite)
}

#[test]
fn test_ancestor_target_preserves_source_for_host_and_rooted() {
    for rooted in [false, true] {
        let fixture = tempdir().expect("fixture");
        let source = fixture.path().join("container/source");
        std::fs::create_dir_all(&source).expect("source");
        std::fs::write(source.join("source"), b"candidate").expect("source child");
        std::fs::write(source.join("unrelated"), b"must survive").expect("unrelated child");
        let filesystem = if rooted {
            LocalFileSystem::rooted(fixture.path()).expect("rooted filesystem")
        } else {
            LocalFileSystem::host().expect("host filesystem")
        };
        let input = if rooted {
            PathBuf::from("container/source")
        } else {
            source.clone()
        };
        let target = if rooted {
            PathBuf::from("container")
        } else {
            fixture.path().join("container")
        };
        let error = filesystem
            .copy_with_options(&input, &target, &overlap_options())
            .expect_err("ancestor target must be rejected");
        assert_eq!(error.state(), LocalCopyFailureState::Unchanged);
        assert_eq!(error.error().kind(), LocalFileErrorKind::InvalidPath);
        assert!(source.is_dir());
        assert_eq!(std::fs::read(source.join("source")).unwrap(), b"candidate");
        assert_eq!(std::fs::read(source.join("unrelated")).unwrap(), b"must survive");
    }
}

#[test]
fn test_empty_directory_cannot_merge_into_its_ancestor() {
    for rooted in [false, true] {
        let fixture = tempdir().expect("fixture");
        let source = fixture.path().join("container/source");
        std::fs::create_dir_all(&source).expect("empty source directory");
        std::fs::write(fixture.path().join("container/sentinel"), b"keep").expect("target sentinel");
        let filesystem = if rooted {
            LocalFileSystem::rooted(fixture.path()).expect("rooted filesystem")
        } else {
            LocalFileSystem::host().expect("host filesystem")
        };
        let input = if rooted {
            PathBuf::from("container/source")
        } else {
            source.clone()
        };
        let target = if rooted {
            PathBuf::from("container")
        } else {
            fixture.path().join("container")
        };

        let failure = filesystem
            .copy_with_options(&input, &target, &overlap_options())
            .expect_err("empty directory overlap must be rejected");

        assert_eq!(LocalCopyFailureState::Unchanged, failure.state());
        assert_eq!(LocalFileErrorKind::InvalidPath, failure.error().kind());
        assert!(source.is_dir());
        assert_eq!(
            b"keep",
            std::fs::read(fixture.path().join("container/sentinel"))
                .unwrap()
                .as_slice()
        );
    }
}

#[test]
fn test_equal_and_descendant_targets_are_rejected_for_host_and_rooted() {
    for rooted in [false, true] {
        for descendant in [false, true] {
            let fixture = tempdir().expect("fixture");
            let source = fixture.path().join("source");
            std::fs::create_dir(&source).expect("source");
            std::fs::write(source.join("payload"), b"keep").expect("payload");
            let filesystem = if rooted {
                LocalFileSystem::rooted(fixture.path()).expect("rooted filesystem")
            } else {
                LocalFileSystem::host().expect("host filesystem")
            };
            let input = if rooted {
                PathBuf::from("source")
            } else {
                source.clone()
            };
            let target = if rooted {
                if descendant {
                    PathBuf::from("source/child")
                } else {
                    PathBuf::from("source")
                }
            } else if descendant {
                source.join("child")
            } else {
                source.clone()
            };
            let failure = filesystem
                .copy_with_options(&input, &target, &overlap_options())
                .expect_err("overlap must be rejected");
            assert_eq!(failure.state(), LocalCopyFailureState::Unchanged);
            assert_eq!(failure.error().kind(), LocalFileErrorKind::InvalidPath);
            assert_eq!(std::fs::read(source.join("payload")).unwrap(), b"keep");
            assert!(!source.join("child").exists());
        }
    }
}

#[test]
fn test_entry_mode_directory_source_keeps_its_source_mode_error() {
    for rooted in [false, true] {
        let fixture = tempdir().expect("fixture");
        let source = fixture.path().join("container/source");
        std::fs::create_dir_all(&source).expect("source");
        std::fs::write(source.join("payload"), b"keep").expect("payload");
        let filesystem = if rooted {
            LocalFileSystem::rooted(fixture.path()).expect("rooted filesystem")
        } else {
            LocalFileSystem::host().expect("host filesystem")
        };
        let input = if rooted {
            PathBuf::from("container/source")
        } else {
            source.clone()
        };
        let target = if rooted {
            PathBuf::from("container")
        } else {
            fixture.path().join("container")
        };
        let error = filesystem
            .copy_with_options(&input, &target, &overlap_options().with_entry_source())
            .expect_err("entry mode rejects a directory source before tree overlap checks");
        assert_eq!(LocalCopyFailureState::Unchanged, error.state());
        assert_eq!(LocalFileErrorKind::RequirementNotMet, error.error().kind());
        assert_eq!(b"keep", std::fs::read(source.join("payload")).unwrap().as_slice());
    }
}

#[test]
fn test_sibling_tree_copy_still_succeeds_for_host_and_rooted() {
    for rooted in [false, true] {
        let fixture = tempdir().expect("fixture");
        let source = fixture.path().join("source");
        std::fs::create_dir(&source).expect("source");
        std::fs::write(source.join("payload"), b"copy").expect("payload");
        let filesystem = if rooted {
            LocalFileSystem::rooted(fixture.path()).expect("rooted filesystem")
        } else {
            LocalFileSystem::host().expect("host filesystem")
        };
        let input = if rooted { PathBuf::from("source") } else { source };
        let target = if rooted {
            PathBuf::from("sibling")
        } else {
            fixture.path().join("sibling")
        };
        let _outcome = filesystem
            .copy_with_options(&input, &target, &overlap_options())
            .expect("unrelated sibling copy");
        assert_eq!(std::fs::read(fixture.path().join("sibling/payload")).unwrap(), b"copy");
    }
}

#[cfg(unix)]
#[test]
fn test_host_ancestor_symlink_alias_is_rejected_before_mutation() {
    use std::os::unix::fs::symlink;

    let fixture = tempdir().expect("fixture");
    let source = fixture.path().join("container/source");
    std::fs::create_dir_all(&source).expect("source");
    std::fs::write(source.join("payload"), b"keep").expect("payload");
    symlink(fixture.path().join("container"), fixture.path().join("alias")).expect("ancestor alias");
    let filesystem = LocalFileSystem::host().expect("host filesystem");
    let failure = filesystem
        .copy_with_options(&source, &fixture.path().join("alias"), &overlap_options())
        .expect_err("canonical ancestor alias must be rejected");
    assert_eq!(failure.state(), LocalCopyFailureState::Unchanged);
    assert_eq!(failure.error().kind(), LocalFileErrorKind::InvalidPath);
    assert_eq!(std::fs::read(source.join("payload")).unwrap(), b"keep");
}
