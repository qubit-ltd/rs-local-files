// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

#![cfg(unix)]

use std::fs;
use std::fs::File;
use std::mem;

use qubit_local_files::LocalFileSystem;
use qubit_local_files::options::LocalPersistOptions;
use qubit_local_files::options::LocalTempFileOptions;
use qubit_local_files::outcome::LocalPersistFailureState;
use qubit_local_files::outcome::LocalPersistStage;
use qubit_local_files::outcome::LocalTempSourceState;
use qubit_local_files::policy::LocalDurabilityRequirement;
use tempfile::tempdir;

#[cfg(unix)]
#[test]
fn test_required_persist_rejects_replaced_open_handle_and_retains_source() {
    for rooted in [false, true] {
        for durability in [
            LocalDurabilityRequirement::Required,
            LocalDurabilityRequirement::Preferred,
        ] {
            let directory = tempdir().expect("private parent should be created");
            let filesystem = if rooted {
                LocalFileSystem::rooted(directory.path()).expect("root authority should open")
            } else {
                LocalFileSystem::host().expect("host authority should open")
            };
            let parent = if rooted {
                std::path::Path::new("/")
            } else {
                directory.path()
            };
            let mut temporary = filesystem
                .create_temp_file_with_options(&LocalTempFileOptions::new().with_parent(parent))
                .expect("temporary source should be created");
            let outside = directory.path().join("other");
            fs::write(&outside, b"other file").expect("other file should be created");
            let replaced = File::open(&outside).expect("other file should open");
            let original = mem::replace(temporary.as_file_mut().expect("source handle should be open"), replaced);
            let target = if rooted {
                std::path::PathBuf::from("/published")
            } else {
                directory.path().join("published")
            };
            let host_target = directory.path().join("published");
            let options = LocalPersistOptions::new().with_durability(durability);

            let error = temporary
                .persist_with(&target, options)
                .expect_err("a handle for another file must not be synchronized and published");
            assert_eq!(LocalPersistStage::SynchronizeSource, error.stage());
            assert_eq!(LocalPersistFailureState::NotPublished, error.state());
            assert_eq!(LocalTempSourceState::Owned, error.source_state());
            assert!(!host_target.exists());
            assert_eq!(
                b"other file",
                fs::read(&outside).expect("other file must remain").as_slice()
            );
            let mut temporary = error.into_parts().resource;
            *temporary.as_file_mut().expect("replacement handle remains open") = original;
            let outcome = temporary
                .persist(&target)
                .expect("restored source handle should persist");
            assert_eq!(target, outcome.path());
            assert!(host_target.exists());
        }
    }
}

#[cfg(unix)]
#[test]
fn test_required_persist_accepts_cloned_and_reopened_source_handles() {
    for reopen in [false, true] {
        let directory = tempdir().expect("private parent should be created");
        let mut temporary = LocalFileSystem::host()
            .expect("host authority should open")
            .create_temp_file_with_options(&LocalTempFileOptions::new().with_parent(directory.path()))
            .expect("temporary source should be created");
        if reopen {
            temporary.close();
        } else {
            let clone = temporary
                .as_file_mut()
                .expect("source handle should be open")
                .try_clone()
                .expect("same-entity handle should clone");
            *temporary.as_file_mut().expect("source handle should be open") = clone;
        }
        let target = directory.path().join("published");
        let outcome = temporary
            .persist_with(
                &target,
                LocalPersistOptions::new().with_durability(LocalDurabilityRequirement::Required),
            )
            .expect("valid source handle should persist durably");
        assert!(outcome.durable());
    }
}

#[cfg(unix)]
#[test]
fn test_closed_persist_rejects_reopened_replacement_path_for_each_durability() {
    for rooted in [false, true] {
        for durability in [
            LocalDurabilityRequirement::Required,
            LocalDurabilityRequirement::Preferred,
        ] {
            let directory = tempdir().expect("private parent should be created");
            let filesystem = if rooted {
                LocalFileSystem::rooted(directory.path()).expect("root authority should open")
            } else {
                LocalFileSystem::host().expect("host authority should open")
            };
            let parent = if rooted {
                std::path::Path::new("/")
            } else {
                directory.path()
            };
            let mut temporary = filesystem
                .create_temp_file_with_options(&LocalTempFileOptions::new().with_parent(parent))
                .expect("temporary source should be created");
            let replacement = directory.path().join("replacement");
            fs::write(&replacement, b"replacement entity")
                .expect("replacement entity should be created while original exists");
            let namespace_source = temporary.path().to_path_buf();
            let source = if rooted {
                directory.path().join(
                    namespace_source
                        .strip_prefix(std::path::Path::new("/"))
                        .expect("rooted source should be virtual-absolute"),
                )
            } else {
                namespace_source
            };
            temporary.close();
            fs::remove_file(&source).expect("original path entity should be removed");
            fs::rename(&replacement, &source).expect("replacement entity should occupy the source path");

            let target = if rooted {
                std::path::PathBuf::from("/published")
            } else {
                directory.path().join("published")
            };
            let host_target = directory.path().join("published");
            let error = temporary
                .persist_with(&target, LocalPersistOptions::new().with_durability(durability))
                .expect_err("a reopened replacement entity must not be persisted");

            assert_eq!(LocalPersistStage::InstallDestination, error.stage());
            assert_eq!(LocalPersistFailureState::NotPublished, error.state());
            assert_eq!(LocalTempSourceState::Indeterminate, error.source_state());
            assert!(!host_target.exists(), "replacement must not be published");
            assert_eq!(
                b"replacement entity",
                fs::read(&source)
                    .expect("replacement must remain at the source path")
                    .as_slice(),
            );
        }
    }
}
