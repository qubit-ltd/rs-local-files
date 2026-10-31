// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
#![cfg(all(unix, feature = "test-support"))]

//! A fully skipped copy has no publication whose parent needs synchronization.

use std::fs;
use std::os::unix::fs::{PermissionsExt, symlink};
use std::path::Path;

use qubit_local_files::LocalFileSystem;
use qubit_local_files::options::LocalCopyConflictPolicy;
use qubit_local_files::options::LocalCopyOptions;
use qubit_local_files::options::LocalMetadataPreservePolicy;
use qubit_local_files::policy::LocalDurabilityRequirement;
use qubit_local_files::test_support::install_test_fault;
use tempfile::tempdir;

#[derive(Clone, Copy)]
enum Facade {
    Host,
    Rooted,
}

#[derive(Clone, Copy)]
enum Entry {
    File,
    Symlink,
}

fn test_skipped_copy_does_not_sync_parent(
    facade: Facade,
    entry: Entry,
    durability: LocalDurabilityRequirement,
) {
    let fixture = tempdir().expect("fixture should exist");
    let source = fixture.path().join("source");
    let target = fixture.path().join("target");
    let referent = fixture.path().join("referent");
    let target_referent = fixture.path().join("target-referent");
    fs::write(&source, b"source").expect("source should exist");
    fs::write(&target, b"existing").expect("target should exist");
    fs::set_permissions(&target, fs::Permissions::from_mode(0o640))
        .expect("target mode should be set");
    if matches!(entry, Entry::Symlink) {
        fs::write(&referent, b"referent").expect("link referent should exist");
        fs::remove_file(&source).expect("regular source should be removed");
        symlink(&referent, &source).expect("source should be a final link");
        fs::write(&target_referent, b"existing").expect("target link referent should exist");
        fs::set_permissions(&target_referent, fs::Permissions::from_mode(0o640))
            .expect("target link referent mode should be set");
        fs::remove_file(&target).expect("regular target should be removed");
        symlink(&target_referent, &target).expect("target should be a final link");
    }
    let (logical_source, logical_target) = match facade {
        Facade::Host => (source.as_path(), target.as_path()),
        Facade::Rooted => (Path::new("source"), Path::new("target")),
    };
    let fault = match facade {
        Facade::Host => "copy-parent-sync",
        Facade::Rooted => "rooted-copy-parent-sync",
    };
    let _guard = install_test_fault(fault).expect("parent-sync fault should install");
    let fs = match facade {
        Facade::Host => LocalFileSystem::host().expect("host filesystem should open"),
        Facade::Rooted => {
            LocalFileSystem::rooted(fixture.path()).expect("rooted filesystem should open")
        }
    };
    let options = LocalCopyOptions::new()
        .with_conflict(LocalCopyConflictPolicy::Skip)
        .with_durability(durability)
        .with_metadata_preservation(LocalMetadataPreservePolicy::Permissions);
    let outcome = fs
        .copy_with_options(logical_source, logical_target, &options)
        .expect("a fully skipped copy should not synchronize the parent");

    assert_eq!(1, outcome.stats().skipped());
    assert_eq!(0, outcome.stats().files());
    assert!(!outcome.atomic());
    assert!(!outcome.durable());
    assert_eq!(
        LocalMetadataPreservePolicy::None,
        outcome.metadata_preservation()
    );
    assert_eq!(
        b"existing",
        fs::read(&target).expect("target should remain").as_slice()
    );
    assert_eq!(
        0o640,
        fs::metadata(if matches!(entry, Entry::Symlink) {
            &target_referent
        } else {
            &target
        })
        .expect("target metadata should remain")
        .permissions()
        .mode()
            & 0o777
    );
    if matches!(entry, Entry::Symlink) {
        assert_eq!(referent, fs::read_link(&source).expect("link remains"));
        assert_eq!(
            target_referent,
            fs::read_link(&target).expect("target link remains")
        );
    }
}

#[test]
fn test_fully_skipped_copy_does_not_sync_parent() {
    for facade in [Facade::Host, Facade::Rooted] {
        for entry in [Entry::File, Entry::Symlink] {
            for durability in [
                LocalDurabilityRequirement::Required,
                LocalDurabilityRequirement::Preferred,
            ] {
                test_skipped_copy_does_not_sync_parent(facade, entry, durability);
            }
        }
    }
}

#[test]
fn test_empty_file_copy_is_not_fully_skipped() {
    let fixture = tempdir().expect("fixture should exist");
    let source = fixture.path().join("source");
    let target = fixture.path().join("target");
    fs::write(&source, b"").expect("empty source should exist");

    let outcome = LocalFileSystem::host()
        .expect("host filesystem should open")
        .copy_with_options(&source, &target, &LocalCopyOptions::new())
        .expect("empty file should copy");

    assert_eq!(1, outcome.stats().files());
    assert_eq!(0, outcome.stats().skipped());
    assert!(target.is_file());
}

#[test]
fn test_empty_directory_creation_is_not_fully_skipped() {
    let fixture = tempdir().expect("fixture should exist");
    let source = fixture.path().join("source");
    let target = fixture.path().join("target");
    fs::create_dir(&source).expect("empty source directory should exist");

    let outcome = LocalFileSystem::host()
        .expect("host filesystem should open")
        .copy_with_options(&source, &target, &LocalCopyOptions::new())
        .expect("empty directory should copy");

    assert!(outcome.stats().directories() > 0);
    assert_eq!(0, outcome.stats().skipped());
    assert!(target.is_dir());
}
