// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
use std::cell::RefCell;
use std::path::Path;
use std::path::PathBuf;

use super::super::LocalStagedDirectory;
use crate::LocalFileSystem;
use crate::observation::LocalFileIdentity;
use crate::options::LocalDirectoryOpenOptions;
use crate::options::LocalListOptions;
use crate::options::LocalTempDirectoryOptions;
#[cfg(target_os = "linux")]
use crate::policy::LocalOwnerAccessPolicy;
#[cfg(not(target_os = "linux"))]
use crate::policy::LocalPrivacyRequirement;

fn sync_test_temp_options() -> LocalTempDirectoryOptions {
    #[cfg(target_os = "linux")]
    {
        LocalTempDirectoryOptions::new()
            .with_owner_access_policy(LocalOwnerAccessPolicy::new().with_unix_modes(0o700, 0o600))
    }
    #[cfg(not(target_os = "linux"))]
    {
        LocalTempDirectoryOptions::new().with_privacy(LocalPrivacyRequirement::Required)
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum SyncKind {
    File,
    Directory,
    Root,
    Parent,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct SyncEvent {
    pub(super) kind: SyncKind,
    pub(super) path: PathBuf,
    pub(super) identity: LocalFileIdentity,
}

thread_local! {
    static SYNC_EVENTS: RefCell<Option<Vec<SyncEvent>>> = const { RefCell::new(None) };
    static SYNC_FAILURE_PATH: RefCell<Option<PathBuf>> = const { RefCell::new(None) };
}

pub(super) struct SyncOrderObserverGuard;

impl SyncOrderObserverGuard {
    fn install(failure_path: Option<&Path>) -> Self {
        SYNC_EVENTS.with(|events| {
            assert!(events.borrow().is_none(), "sync observer already installed");
            *events.borrow_mut() = Some(Vec::new());
        });
        SYNC_FAILURE_PATH.with(|slot| *slot.borrow_mut() = failure_path.map(Path::to_path_buf));
        Self
    }
}

impl Drop for SyncOrderObserverGuard {
    fn drop(&mut self) {
        SYNC_EVENTS.with(|events| events.borrow_mut().take());
        SYNC_FAILURE_PATH.with(|slot| slot.borrow_mut().take());
    }
}

pub(super) fn record_sync_event(event: SyncEvent) -> std::io::Result<()> {
    SYNC_EVENTS.with(|events| {
        if let Some(events) = events.borrow_mut().as_mut() {
            events.push(event.clone());
        }
    });
    let expire_after_file = super::TREE_SYNC_EXPIRE_AFTER_FILE.with(|expire| {
        if event.kind == SyncKind::File && expire.get() {
            expire.set(false);
            true
        } else {
            false
        }
    });
    if expire_after_file {
        super::TREE_SYNC_TEST_EXPIRED.with(|expired| expired.set(true));
    }
    let expire_after_root = super::TREE_SYNC_EXPIRE_AFTER_ROOT.with(|expire| {
        if event.kind == SyncKind::Root && expire.get() {
            expire.set(false);
            true
        } else {
            false
        }
    });
    if expire_after_root {
        super::TREE_SYNC_TEST_EXPIRED.with(|expired| expired.set(true));
    }
    let expire_after_directory = super::TREE_SYNC_EXPIRE_AFTER_DIRECTORY.with(|expire| {
        if event.kind == SyncKind::Directory && expire.get() {
            expire.set(false);
            true
        } else {
            false
        }
    });
    if expire_after_directory {
        super::TREE_SYNC_TEST_EXPIRED.with(|expired| expired.set(true));
    }
    let should_fail = SYNC_FAILURE_PATH.with(|slot| slot.borrow().as_deref() == Some(event.path.as_path()));
    if should_fail {
        SYNC_FAILURE_PATH.with(|slot| slot.borrow_mut().take());
        Err(std::io::Error::other("injected directory sync failure"))
    } else {
        Ok(())
    }
}

pub(super) fn test_review_tree_sync_orders_files_directories_root_and_parent() {
    let fixture = tempfile::tempdir().expect("fixture should be created");
    let filesystem = LocalFileSystem::host().expect("host filesystem should open");
    let parent = filesystem
        .open_directory_with_options(fixture.path(), &LocalDirectoryOpenOptions::new())
        .expect("parent authority should open");
    let mut stage = parent
        .create_staged_directory(&sync_test_temp_options())
        .expect("stage should be created");
    stage
        .write_file_new(Path::new("a/b/file"), &mut &b"nested"[..], 16)
        .expect("nested file should be written");
    stage
        .write_file_new(Path::new("c/file"), &mut &b"sibling"[..], 16)
        .expect("sibling file should be written");

    let _observer = SyncOrderObserverGuard::install(None);
    stage
        .sync_tree_with_options(&parent, &LocalListOptions::new())
        .expect("the staged tree and parent should synchronize");
    let events = SYNC_EVENTS.with(|events| events.borrow().clone().expect("observer should be installed"));

    assert_sync_order(&events, &stage, &parent);
    let mut swapped = events.clone();
    let file_position = swapped
        .iter()
        .position(|event| event.kind == SyncKind::File)
        .expect("file event");
    let directory_position = swapped
        .iter()
        .position(|event| event.kind == SyncKind::Directory)
        .expect("directory event");
    swapped.swap(file_position, directory_position);
    assert!(
        std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| assert_sync_order(
            &swapped, &stage, &parent
        )))
        .is_err(),
        "the ordering assertions should reject a file/directory event swap"
    );
}

pub(super) fn test_review_tree_sync_stops_after_nested_directory_failure() {
    let fixture = tempfile::tempdir().expect("fixture should be created");
    let filesystem = LocalFileSystem::host().expect("host filesystem should open");
    let parent = filesystem
        .open_directory_with_options(fixture.path(), &LocalDirectoryOpenOptions::new())
        .expect("parent authority should open");
    let mut stage = parent
        .create_staged_directory(&sync_test_temp_options())
        .expect("stage should be created");
    stage
        .write_file_new(Path::new("a/b/file"), &mut &b"nested"[..], 16)
        .expect("nested file should be written");
    stage
        .write_file_new(Path::new("c/file"), &mut &b"sibling"[..], 16)
        .expect("sibling file should be written");

    let _observer = SyncOrderObserverGuard::install(Some(Path::new("a/b")));
    let error = stage
        .sync_tree_with_options(&parent, &LocalListOptions::new())
        .expect_err("the injected a/b sync failure should be returned");
    assert_eq!(error.io_error_kind(), std::io::ErrorKind::Other);
    assert_eq!(
        error
            .io_error()
            .expect("injected I/O error should be preserved")
            .to_string(),
        "injected directory sync failure"
    );
    let events = SYNC_EVENTS.with(|events| events.borrow().clone().expect("observer should be installed"));
    assert!(events.iter().any(|event| event.path == Path::new("a/b")));
    assert!(
        !events
            .iter()
            .any(|event| matches!(event.kind, SyncKind::Root | SyncKind::Parent))
    );
}

pub(super) fn test_tree_sync_deadline_stops_after_file_sync() {
    let fixture = tempfile::tempdir().expect("fixture");
    let filesystem = LocalFileSystem::host().expect("host filesystem");
    let parent = filesystem
        .open_directory_with_options(fixture.path(), &LocalDirectoryOpenOptions::new())
        .expect("parent authority");
    let mut stage = parent
        .create_staged_directory(&sync_test_temp_options())
        .expect("stage");
    stage
        .write_file_new(Path::new("nested/payload"), &mut &b"payload"[..], 32)
        .expect("payload");

    super::TREE_SYNC_TEST_EXPIRED.with(|expired| expired.set(false));
    super::TREE_SYNC_EXPIRE_AFTER_FILE.with(|expire| expire.set(true));
    let _observer = SyncOrderObserverGuard::install(None);
    let error = stage
        .sync_tree_with_options(&parent, &LocalListOptions::new())
        .expect_err("the deterministic file-phase timeout must stop the tree sync");
    assert_eq!(error.io_error_kind(), std::io::ErrorKind::TimedOut);
    let events = SYNC_EVENTS.with(|events| events.borrow().clone().expect("observer"));
    assert!(events.iter().any(|event| event.kind == SyncKind::File));
    assert!(
        !events
            .iter()
            .any(|event| matches!(event.kind, SyncKind::Directory | SyncKind::Root | SyncKind::Parent))
    );
    drop(_observer);
    super::TREE_SYNC_TEST_EXPIRED.with(|expired| expired.set(false));
    super::TREE_SYNC_EXPIRE_AFTER_FILE.with(|expire| expire.set(false));
    stage
        .sync_tree_with_options(&parent, &LocalListOptions::new())
        .expect("a new invocation receives a fresh budget");
    stage.cleanup().expect("cleanup retry stage");
}

pub(super) fn test_tree_sync_deadline_zero_rejects_at_entry() {
    let fixture = tempfile::tempdir().expect("fixture");
    let filesystem = LocalFileSystem::host().expect("host filesystem");
    let parent = filesystem
        .open_directory_with_options(fixture.path(), &LocalDirectoryOpenOptions::new())
        .expect("parent authority");
    let mut stage = parent
        .create_staged_directory(&LocalTempDirectoryOptions::new())
        .expect("stage");
    stage
        .write_file_new(Path::new("payload"), &mut &b"payload"[..], 32)
        .expect("payload");

    let _observer = SyncOrderObserverGuard::install(None);
    let error = stage
        .sync_tree_with_options(
            &parent,
            &LocalListOptions::new().with_deadline(std::time::Duration::ZERO),
        )
        .expect_err("zero deadline must expire before synchronization begins");
    assert_eq!(error.io_error_kind(), std::io::ErrorKind::TimedOut);
    let events = SYNC_EVENTS.with(|events| events.borrow().clone().expect("observer"));
    assert!(events.is_empty(), "no sync event may follow an expired entry budget");
    drop(_observer);
    stage.cleanup().expect("cleanup retained stage");
}

pub(super) fn test_tree_sync_deadline_stops_before_parent_sync() {
    let fixture = tempfile::tempdir().expect("fixture");
    let filesystem = LocalFileSystem::host().expect("host filesystem");
    let parent = filesystem
        .open_directory_with_options(fixture.path(), &LocalDirectoryOpenOptions::new())
        .expect("parent authority");
    let mut stage = parent
        .create_staged_directory(&LocalTempDirectoryOptions::new())
        .expect("stage");
    stage
        .write_file_new(Path::new("payload"), &mut &b"payload"[..], 32)
        .expect("payload");

    super::TREE_SYNC_TEST_EXPIRED.with(|expired| expired.set(false));
    super::TREE_SYNC_EXPIRE_AFTER_ROOT.with(|expire| expire.set(true));
    let _observer = SyncOrderObserverGuard::install(None);
    let error = stage
        .sync_tree_with_options(&parent, &LocalListOptions::new())
        .expect_err("expiry after root sync must stop before parent sync");
    assert_eq!(error.io_error_kind(), std::io::ErrorKind::TimedOut);
    let events = SYNC_EVENTS.with(|events| events.borrow().clone().expect("observer"));
    assert!(events.iter().any(|event| event.kind == SyncKind::Root));
    assert!(!events.iter().any(|event| event.kind == SyncKind::Parent));
    drop(_observer);
    super::TREE_SYNC_TEST_EXPIRED.with(|expired| expired.set(false));
    super::TREE_SYNC_EXPIRE_AFTER_ROOT.with(|expire| expire.set(false));
    stage
        .sync_tree_with_options(&parent, &LocalListOptions::new())
        .expect("a new invocation receives a fresh budget");
    stage.cleanup().expect("cleanup retry stage");
}

pub(super) fn test_tree_sync_deadline_expires_after_collection_before_directory_sync() {
    let fixture = tempfile::tempdir().expect("fixture");
    let filesystem = LocalFileSystem::host().expect("host filesystem");
    let parent = filesystem
        .open_directory_with_options(fixture.path(), &LocalDirectoryOpenOptions::new())
        .expect("parent authority");
    let mut stage = parent
        .create_staged_directory(&sync_test_temp_options())
        .expect("stage");
    stage
        .write_file_new(Path::new("nested/payload"), &mut &b"payload"[..], 32)
        .expect("payload");

    super::TREE_SYNC_TEST_EXPIRED.with(|expired| expired.set(false));
    super::TREE_SYNC_EXPIRE_AFTER_COLLECTION.with(|expire| expire.set(true));
    let _observer = SyncOrderObserverGuard::install(None);
    let error = stage
        .sync_tree_with_options(&parent, &LocalListOptions::new())
        .expect_err("expiry after collection must stop before directory sync");
    assert_eq!(error.io_error_kind(), std::io::ErrorKind::TimedOut);
    let events = SYNC_EVENTS.with(|events| events.borrow().clone().expect("observer"));
    assert!(events.iter().any(|event| event.kind == SyncKind::File));
    assert!(
        !events
            .iter()
            .any(|event| matches!(event.kind, SyncKind::Directory | SyncKind::Root | SyncKind::Parent))
    );
    drop(_observer);
    super::TREE_SYNC_TEST_EXPIRED.with(|expired| expired.set(false));
    super::TREE_SYNC_EXPIRE_AFTER_COLLECTION.with(|expire| expire.set(false));
    stage
        .sync_tree_with_options(&parent, &LocalListOptions::new())
        .expect("a new invocation receives a fresh budget");
    stage.cleanup().expect("cleanup retry stage");
}

pub(super) fn test_tree_sync_deadline_stops_after_directory_sync() {
    let fixture = tempfile::tempdir().expect("fixture");
    let filesystem = LocalFileSystem::host().expect("host filesystem");
    let parent = filesystem
        .open_directory_with_options(fixture.path(), &LocalDirectoryOpenOptions::new())
        .expect("parent authority");
    let mut stage = parent
        .create_staged_directory(&sync_test_temp_options())
        .expect("stage");
    stage
        .write_file_new(Path::new("nested/payload"), &mut &b"payload"[..], 32)
        .expect("payload");

    super::TREE_SYNC_TEST_EXPIRED.with(|expired| expired.set(false));
    super::TREE_SYNC_EXPIRE_AFTER_DIRECTORY.with(|expire| expire.set(true));
    let _observer = SyncOrderObserverGuard::install(None);
    let error = stage
        .sync_tree_with_options(&parent, &LocalListOptions::new())
        .expect_err("expiry after directory sync must stop before root sync");
    assert_eq!(error.io_error_kind(), std::io::ErrorKind::TimedOut);
    let events = SYNC_EVENTS.with(|events| events.borrow().clone().expect("observer"));
    assert!(events.iter().any(|event| event.kind == SyncKind::Directory));
    assert!(
        !events
            .iter()
            .any(|event| matches!(event.kind, SyncKind::Root | SyncKind::Parent))
    );
    drop(_observer);
    super::TREE_SYNC_TEST_EXPIRED.with(|expired| expired.set(false));
    super::TREE_SYNC_EXPIRE_AFTER_DIRECTORY.with(|expire| expire.set(false));
    stage
        .sync_tree_with_options(&parent, &LocalListOptions::new())
        .expect("a new invocation receives a fresh budget");
    stage.cleanup().expect("cleanup retry stage");
}

fn assert_sync_order(
    events: &[SyncEvent],
    stage: &LocalStagedDirectory,
    parent: &crate::directory::LocalDirectoryHandle,
) {
    let position = |kind, path: &Path| {
        events
            .iter()
            .position(|event| event.kind == kind && event.path == path)
            .expect("sync event should exist")
    };
    for file in [Path::new("a/b/file"), Path::new("c/file")] {
        let file_position = position(SyncKind::File, file);
        for directory in [Path::new("a/b"), Path::new("a"), Path::new("c")] {
            assert!(
                file_position < position(SyncKind::Directory, directory),
                "{file:?} must sync before {directory:?}"
            );
        }
    }
    assert!(position(SyncKind::Directory, Path::new("a/b")) < position(SyncKind::Directory, Path::new("a")));
    let root_position = position(SyncKind::Root, Path::new("."));
    for directory in [Path::new("a/b"), Path::new("a"), Path::new("c")] {
        assert!(position(SyncKind::Directory, directory) < root_position);
    }
    assert!(root_position < position(SyncKind::Parent, Path::new(".")));
    for event in events {
        if matches!(event.kind, SyncKind::File | SyncKind::Directory) {
            assert_eq!(
                stage
                    .directory()
                    .observe_entry(&event.path)
                    .expect("entry observation")
                    .identity(),
                Some(&event.identity)
            );
        }
        if event.kind == SyncKind::Root {
            assert_eq!(stage.identity(), Some(&event.identity));
        }
        if event.kind == SyncKind::Parent {
            assert_eq!(
                parent.observe().expect("parent observation").identity(),
                Some(&event.identity)
            );
        }
    }
}
