// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
#[cfg(test)]
use std::cell::Cell;
#[cfg(test)]
use std::cell::RefCell;
use std::path::Path;
#[cfg(all(test, unix))]
use std::path::PathBuf;
use std::time::Instant;

use super::LocalStagedDirectory;
use super::internal::tree_sync_budget::TreeSyncBudget;
use super::resource_limit;
use crate::LocalFileError;
use crate::LocalFileErrorKind;
use crate::LocalFileOperation;
use crate::directory::LocalDirectoryHandle;
use crate::error::LocalResult;
#[cfg(test)]
use crate::observation::LocalFileIdentity;
use crate::options::LocalListOptions;
use crate::policy::LocalFileSyncRequirement;
#[cfg(all(test, unix))]
mod sync_order_tests;

#[cfg(test)]
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) enum OwnerPolicySyncEvent {
    PreSync(LocalFileIdentity),
    #[cfg(unix)]
    AccessApplied(LocalFileIdentity),
    FinalSync(LocalFileIdentity),
}

#[cfg(test)]
thread_local! {
    static TREE_SYNC_FAILURE_AFTER: Cell<Option<usize>> = const { Cell::new(None) };
    pub(super) static TREE_SYNC_TEST_EXPIRED: Cell<bool> = const { Cell::new(false) };
    static TREE_SYNC_EXPIRE_AFTER_FILE: Cell<bool> = const { Cell::new(false) };
    static TREE_SYNC_EXPIRE_AFTER_DIRECTORY: Cell<bool> = const { Cell::new(false) };
    static TREE_SYNC_EXPIRE_AFTER_ROOT: Cell<bool> = const { Cell::new(false) };
    static TREE_SYNC_EXPIRE_AFTER_COLLECTION: Cell<bool> = const { Cell::new(false) };
    pub(super) static OWNER_POLICY_SYNC_EVENTS: RefCell<Option<Vec<OwnerPolicySyncEvent>>> = const { RefCell::new(None) };
}

#[cfg(all(test, unix))]
pub(super) fn fail_tree_sync_after(successful_steps: usize) {
    TREE_SYNC_FAILURE_AFTER.with(|slot| slot.set(Some(successful_steps)));
}

#[cfg(test)]
pub(super) fn fail_tree_sync_if_scheduled() -> std::io::Result<()> {
    let fail = TREE_SYNC_FAILURE_AFTER.with(|slot| match slot.get() {
        Some(0) => {
            slot.set(None);
            true
        }
        Some(remaining) => {
            slot.set(Some(remaining - 1));
            false
        }
        None => false,
    });
    if fail {
        Err(std::io::Error::other("injected tree file sync failure"))
    } else {
        Ok(())
    }
}

#[cfg(test)]
pub(in crate::directory) fn record_owner_policy_sync_event(event: OwnerPolicySyncEvent) {
    OWNER_POLICY_SYNC_EVENTS.with(|events| {
        if let Some(events) = events.borrow_mut().as_mut() {
            events.push(event);
        }
    });
}

#[cfg(all(test, unix))]
pub(super) fn test_review_tree_sync_orders_files_directories_root_and_parent() {
    sync_order_tests::test_review_tree_sync_orders_files_directories_root_and_parent();
}

#[cfg(all(test, unix))]
pub(super) fn test_review_tree_sync_stops_after_nested_directory_failure() {
    sync_order_tests::test_review_tree_sync_stops_after_nested_directory_failure();
}

#[cfg(all(test, unix))]
#[test]
fn test_tree_sync_deadline_stops_after_file_sync() {
    sync_order_tests::test_tree_sync_deadline_stops_after_file_sync();
}

#[cfg(all(test, unix))]
#[test]
fn test_tree_sync_deadline_stops_before_parent_sync() {
    sync_order_tests::test_tree_sync_deadline_stops_before_parent_sync();
}

#[cfg(all(test, unix))]
#[test]
fn test_tree_sync_deadline_zero_rejects_at_entry() {
    sync_order_tests::test_tree_sync_deadline_zero_rejects_at_entry();
}

#[cfg(test)]
fn expire_tree_sync_after_collection_if_scheduled() {
    let expire = TREE_SYNC_EXPIRE_AFTER_COLLECTION.with(|slot| {
        if slot.get() {
            slot.set(false);
            true
        } else {
            false
        }
    });
    if expire {
        TREE_SYNC_TEST_EXPIRED.with(|slot| slot.set(true));
    }
}

#[cfg(all(test, unix))]
#[test]
fn test_tree_sync_deadline_expires_after_collection_before_directory_sync() {
    sync_order_tests::test_tree_sync_deadline_expires_after_collection_before_directory_sync();
}

#[cfg(all(test, unix))]
#[test]
fn test_tree_sync_deadline_stops_after_directory_sync() {
    sync_order_tests::test_tree_sync_deadline_stops_after_directory_sync();
}

#[cfg(test)]
#[test]
fn test_tree_sync_rejects_a_stage_without_observed_identity() {
    let fixture = tempfile::tempdir().expect("fixture");
    let filesystem = crate::LocalFileSystem::host().expect("host filesystem");
    let parent = filesystem
        .open_directory_with_options(fixture.path(), &crate::options::LocalDirectoryOpenOptions::new())
        .expect("parent authority");
    let mut stage = parent
        .create_staged_directory(&crate::options::LocalTempDirectoryOptions::new())
        .expect("stage");
    let identity = stage.identity.clone();
    assert!(identity.is_some(), "the fixture stage should have an observed identity");

    stage.identity = None;
    let error = stage
        .sync_tree_with_options(&parent, &LocalListOptions::new())
        .expect_err("tree synchronization requires an observed stage identity");
    assert_eq!(LocalFileErrorKind::Unsupported, error.kind());
    assert_eq!(LocalFileOperation::Metadata, error.operation());

    stage.identity = identity;
    stage.cleanup().expect("cleanup stage");
}

#[cfg(test)]
#[test]
fn test_not_required_file_sync_skips_depth_limit_for_nonempty_stage() {
    let fixture = tempfile::tempdir().expect("fixture");
    let filesystem = crate::LocalFileSystem::host().expect("host filesystem");
    let parent = filesystem
        .open_directory_with_options(fixture.path(), &crate::options::LocalDirectoryOpenOptions::new())
        .expect("parent authority");
    let mut stage = parent
        .create_staged_directory(&crate::options::LocalTempDirectoryOptions::new())
        .expect("stage");
    let mut payload = &b"payload"[..];
    stage
        .write_file_new(Path::new("payload"), &mut payload, 32)
        .expect("write staged payload");

    let limits = LocalListOptions::new().with_max_depth(0);
    let synced = stage
        .sync_files_with_options(&limits, LocalFileSyncRequirement::NotRequired)
        .expect("NotRequired skips traversal even when the stage is nonempty");
    assert!(!synced, "no files are synchronized when the requirement is NotRequired");

    let error = stage
        .sync_files_with_options(&limits, LocalFileSyncRequirement::Required)
        .expect_err("Required enforces the depth limit on the same staged file");
    assert_eq!(LocalFileErrorKind::ResourceLimit, error.kind());
    stage.cleanup().expect("cleanup stage");
}

#[cfg(all(test, target_os = "linux"))]
mod owner_policy_sync_order_tests {
    use std::ffi::OsStr;
    use std::os::unix::fs::PermissionsExt;
    use std::path::Path;

    use super::OWNER_POLICY_SYNC_EVENTS;
    use super::OwnerPolicySyncEvent;
    use crate::LocalFileSystem;
    use crate::options::LocalDirectoryOpenOptions;
    use crate::options::LocalTempDirectoryOptions;
    use crate::options::LocalTreePublishOptions;
    use crate::policy::LocalFileSyncRequirement;
    use crate::policy::LocalOwnerAccessPolicy;

    struct EventCollectionGuard;

    impl Drop for EventCollectionGuard {
        fn drop(&mut self) {
            OWNER_POLICY_SYNC_EVENTS.with(|events| events.borrow_mut().take());
        }
    }

    #[test]
    fn test_owner_policy_publish_syncs_the_applied_identity_after_access_verification() {
        let scratch = tempfile::tempdir().expect("scratch directory");
        let parent = LocalFileSystem::host()
            .expect("host filesystem")
            .open_directory_with_options(scratch.path(), &LocalDirectoryOpenOptions::new())
            .expect("parent authority");
        let mut stage = parent
            .create_staged_directory(&LocalTempDirectoryOptions::new())
            .expect("stage");
        stage
            .write_file_new(Path::new("payload"), &mut &b"payload"[..], 32)
            .expect("payload");
        OWNER_POLICY_SYNC_EVENTS.with(|events| *events.borrow_mut() = Some(Vec::new()));
        let _event_collection = EventCollectionGuard;

        let policy = LocalOwnerAccessPolicy::new().with_unix_modes(0o500, 0o400);
        let outcome = stage
            .publish_new(
                &parent,
                OsStr::new("published"),
                &LocalTreePublishOptions::new()
                    .with_owner_access_policy(policy)
                    .with_file_sync(LocalFileSyncRequirement::Required),
            )
            .expect("publish after final file sync");
        assert!(outcome.file_synced());

        let events = OWNER_POLICY_SYNC_EVENTS.with(|events| events.borrow_mut().take().expect("sync event collection"));
        assert!(matches!(
            events.as_slice(),
            [
                OwnerPolicySyncEvent::PreSync(pre),
                OwnerPolicySyncEvent::AccessApplied(applied),
                OwnerPolicySyncEvent::FinalSync(final_sync),
            ] if pre == applied && applied == final_sync
        ));
        std::fs::set_permissions(scratch.path().join("published"), std::fs::Permissions::from_mode(0o700))
            .expect("restore fixture directory mode");
        std::fs::set_permissions(
            scratch.path().join("published/payload"),
            std::fs::Permissions::from_mode(0o600),
        )
        .expect("restore fixture file mode");
    }
}

impl LocalStagedDirectory {
    /// Synchronizes every regular file currently contained in this staged
    /// directory.
    ///
    /// The stage remains unpublished. `limits` bounds tree traversal, and a
    /// depth limit that would truncate the tree is reported as an error. With
    /// `Required`, a file synchronization failure is an error; with
    /// `Preferred`, the method returns `false` when a file could not be synced.
    /// `NotRequired` performs no file synchronization and returns `false`.
    /// This method does not synchronize the parent namespace.
    /// A deadline is measured from this call's entry. Use
    /// [`Self::sync_tree_with_options`] to synchronize descendant directories,
    /// the stage root, and its creating parent under one shared deadline.
    ///
    /// # Parameters
    /// `limits` bounds the traversal and provides the optional deadline.
    /// `requirement` controls how file synchronization failures are handled.
    ///
    /// # Returns
    /// Returns `false` when syncing is not required, otherwise whether every
    /// file was synced.
    ///
    /// # Errors
    /// Returns an error when the staged tree cannot be completely traversed,
    /// its contents change during observation, a resource limit is exceeded,
    /// or a required file synchronization fails.
    ///
    /// # Panics
    /// Panics if the staged directory handle has already been closed.
    #[inline]
    pub fn sync_files_with_options(
        &self,
        limits: &LocalListOptions,
        requirement: LocalFileSyncRequirement,
    ) -> LocalResult<bool> {
        self.sync_tree_files(limits, requirement)
    }

    /// Synchronizes this stage's files, directories, root, and creating parent.
    ///
    /// Files are synchronized first, followed by descendant directories from
    /// deepest to shallowest, the staged root, and finally `parent`. The stage
    /// remains unpublished. `limits` bounds every tree traversal; a depth
    /// limit that would truncate the tree is an error. This operation requires
    /// all requested handle synchronizations to succeed.
    /// Synchronization is a durability operation only; publication remains a
    /// separate explicit step, so a successfully synchronized stage is still
    /// cleanup-owned and unpublished.
    /// One cooperative deadline, when configured, starts at this call's entry
    /// and covers both tree traversals and every file, directory, root, and
    /// parent synchronization. It cannot cancel an operating-system call
    /// already in progress.
    ///
    /// # Parameters
    /// `parent` is the creating directory authority that must still name this
    /// stage. `limits` bounds both traversals and supplies the optional
    /// shared deadline.
    ///
    /// # Returns
    /// Returns `Ok(())` after every requested file and directory
    /// synchronization succeeds.
    ///
    /// # Errors
    /// Returns an error when the parent no longer names this stage, the staged
    /// tree cannot be completely traversed, an entry changes during the
    /// operation, a resource limit is exceeded, or any file/directory sync
    /// fails.
    ///
    /// # Panics
    /// Panics if the staged directory handle has already been closed.
    pub fn sync_tree_with_options(&self, parent: &LocalDirectoryHandle, limits: &LocalListOptions) -> LocalResult<()> {
        let started_at = Instant::now();
        let budget = TreeSyncBudget::new(started_at, limits)?;
        budget.check(self.directory().diagnostic_path())?;
        let expected_stage_identity = self
            .identity
            .as_ref()
            .ok_or_else(|| LocalFileError::new(LocalFileErrorKind::Unsupported, LocalFileOperation::Metadata))?;
        let stage_name = Path::new(&self.relative_name);
        let parent_entry = parent.observe_entry(stage_name)?;
        if parent_entry.identity() != Some(expected_stage_identity) {
            return Err(
                LocalFileError::new(LocalFileErrorKind::ChangedDuringRead, LocalFileOperation::Metadata)
                    .with_path(parent.diagnostic_path().join(stage_name)),
            );
        }

        budget.check(parent.diagnostic_path())?;

        self.sync_tree_files_started_at(limits, LocalFileSyncRequirement::Required, started_at)?;
        budget.check(self.directory().diagnostic_path())?;

        let directories = self.collect_tree_sync_directories(limits, started_at, &budget)?;

        self.sync_tree_directories(directories, &budget)?;

        self.sync_tree_root_and_parent(parent, &budget, expected_stage_identity)?;
        Ok(())
    }

    /// Synchronizes stage files under the caller's requirement and traversal
    /// limits.
    ///
    /// # Parameters
    /// `limits` bounds traversal and supplies the optional deadline.
    /// `requirement` selects required, preferred, or skipped file syncing.
    ///
    /// # Returns
    /// Returns whether every requested file synchronization completed.
    ///
    /// # Errors
    /// Returns an error when traversal, observation, configured limits, the
    /// deadline, or a required file synchronization fails.
    ///
    /// # Panics
    /// Panics if the staged directory handle has already been closed.
    #[inline]
    pub(in crate::directory) fn sync_tree_files(
        &self,
        limits: &LocalListOptions,
        requirement: LocalFileSyncRequirement,
    ) -> LocalResult<bool> {
        self.sync_tree_files_started_at(limits, requirement, Instant::now())
    }

    /// Synchronizes regular files using a deadline shared with a later tree
    /// pass.
    ///
    /// # Parameters
    /// `limits` bounds traversal and supplies the optional deadline.
    /// `requirement` selects required, preferred, or skipped file syncing.
    /// `started_at` is the instant shared by the file and directory passes.
    ///
    /// # Returns
    /// Returns whether every requested file synchronization completed.
    ///
    /// # Errors
    /// Returns an error when deadline construction, traversal, observation,
    /// configured limits, or a required file synchronization fails.
    ///
    /// # Panics
    /// Panics if the staged directory handle has already been closed.
    fn sync_tree_files_started_at(
        &self,
        limits: &LocalListOptions,
        requirement: LocalFileSyncRequirement,
        started_at: Instant,
    ) -> LocalResult<bool> {
        if requirement == LocalFileSyncRequirement::NotRequired {
            return Ok(false);
        }
        let budget = TreeSyncBudget::new(started_at, limits)?;
        budget.check(self.directory().diagnostic_path())?;
        if requirement == LocalFileSyncRequirement::NotRequired {
            return Ok(false);
        }
        let root = self.directory().root_authority()?;
        let mut synced = true;
        let traversal = limits
            .without_max_depth()
            .with_error_policy(crate::options::LocalWalkErrorPolicy::FailFast);
        for item in self.observe_tree_with_options_started_at(&traversal, started_at)? {
            budget.check(self.directory().diagnostic_path())?;
            let item = item?;
            budget.check(&self.directory().diagnostic_path().join(item.relative_path()))?;
            let depth = item.relative_path().components().count();
            if limits.max_depth().is_some_and(|maximum| depth > maximum) {
                return Err(resource_limit());
            }
            if item.observation().metadata().kind() != crate::LocalFileKind::File {
                continue;
            }
            let expected =
                item.observation().identity().cloned().ok_or_else(|| {
                    LocalFileError::new(LocalFileErrorKind::Unsupported, LocalFileOperation::Metadata)
                })?;
            let relative = crate::LocalRelativePath::new(item.relative_path()).map_err(|source| {
                LocalFileError::from_io(
                    LocalFileOperation::ValidateName,
                    Some(item.relative_path().to_path_buf()),
                    None,
                    source,
                )
            })?;
            #[cfg(unix)]
            let opened = root.open_reader(&relative, &crate::read::OpenOptions::default());
            #[cfg(windows)]
            let opened = root.open_writer(
                &relative,
                &crate::write::OpenOptions::new(crate::write::Mode::OpenExistingAtStart),
            );
            #[cfg(not(any(unix, windows)))]
            let opened = root.open_writer(
                &relative,
                &crate::write::OpenOptions::new(crate::write::Mode::OpenExistingAtStart),
            );
            let opened = opened.map_err(|source| {
                LocalFileError::from_io(
                    LocalFileOperation::Commit,
                    Some(self.directory().diagnostic_path().join(item.relative_path())),
                    None,
                    source,
                )
            })?;
            let observation = crate::observation::query::query_observation(
                &opened,
                crate::observation::LocalObservationRequirement::Required,
            )?;
            if observation.identity() != Some(&expected) || observation.metadata().kind() != crate::LocalFileKind::File
            {
                return Err(
                    LocalFileError::new(LocalFileErrorKind::ChangedDuringRead, LocalFileOperation::Metadata)
                        .with_path(self.directory().diagnostic_path().join(item.relative_path())),
                );
            }
            budget.check(&self.directory().diagnostic_path().join(item.relative_path()))?;
            #[cfg(test)]
            if let Err(source) = fail_tree_sync_if_scheduled() {
                if requirement == LocalFileSyncRequirement::Required {
                    return Err(LocalFileError::from_io(
                        LocalFileOperation::Commit,
                        Some(self.directory().diagnostic_path().join(item.relative_path())),
                        None,
                        source,
                    ));
                }
                synced = false;
                continue;
            }
            #[cfg(feature = "test-support")]
            if crate::local::take_test_support("owner-policy-sync-failure") {
                let source = crate::local::test_fault_error();
                if requirement == LocalFileSyncRequirement::Required {
                    return Err(LocalFileError::from_io(
                        LocalFileOperation::Commit,
                        Some(self.directory().diagnostic_path().join(item.relative_path())),
                        None,
                        source,
                    ));
                }
                synced = false;
                continue;
            }
            #[cfg(test)]
            let sync_result =
                crate::tests::native_io_fault_tests::check(crate::tests::native_io_fault_tests::NativeIoPoint::SyncAll)
                    .and_then(|()| opened.sync_all());
            #[cfg(not(test))]
            let sync_result = opened.sync_all();
            if let Err(source) = sync_result {
                if requirement == LocalFileSyncRequirement::Required {
                    return Err(LocalFileError::from_io(
                        LocalFileOperation::Commit,
                        Some(self.directory().diagnostic_path().join(item.relative_path())),
                        None,
                        source,
                    ));
                }
                synced = false;
            } else {
                #[cfg(all(test, unix))]
                let _ = sync_order_tests::record_sync_event(sync_order_tests::SyncEvent {
                    kind: sync_order_tests::SyncKind::File,
                    path: item.relative_path().to_path_buf(),
                    identity: expected.clone(),
                });
                #[cfg(test)]
                record_owner_policy_sync_event(OwnerPolicySyncEvent::PreSync(expected));
            }
            budget.check(&self.directory().diagnostic_path().join(item.relative_path()))?;
        }
        budget.check(self.directory().diagnostic_path())?;
        Ok(synced)
    }

    /// Collects descendant directory identities while enforcing tree limits.
    ///
    /// # Parameters
    /// `limits` supplies traversal limits for the staged tree.
    /// `started_at` is the shared start instant used to enforce the operation
    /// deadline. `budget` checks the same cooperative deadline during
    /// collection.
    fn collect_tree_sync_directories(
        &self,
        limits: &LocalListOptions,
        started_at: Instant,
        budget: &TreeSyncBudget,
    ) -> LocalResult<Vec<(std::path::PathBuf, crate::observation::LocalFileIdentity)>> {
        let traversal = limits
            .with_recursive()
            .without_max_depth()
            .with_symlink_policy(crate::policy::LocalSymlinkPolicy::Reject)
            .with_error_policy(crate::options::LocalWalkErrorPolicy::FailFast);
        let mut directories = Vec::new();
        let mut seen_bytes = 0_usize;
        for item in self.observe_tree_with_options_started_at(&traversal, started_at)? {
            budget.check(self.directory().diagnostic_path())?;
            let item = item?;
            let relative = item.relative_path();
            let depth = relative.components().count();
            if limits.max_depth().is_some_and(|maximum| depth > maximum) {
                return Err(resource_limit());
            }
            seen_bytes = seen_bytes
                .checked_add(relative.as_os_str().as_encoded_bytes().len())
                .ok_or_else(resource_limit)?;
            if limits.max_seen_name_bytes().is_some_and(|maximum| seen_bytes > maximum) {
                return Err(resource_limit());
            }
            if item.observation().metadata().kind() == crate::LocalFileKind::Directory {
                let identity = item.observation().identity().cloned().ok_or_else(|| {
                    LocalFileError::new(LocalFileErrorKind::Unsupported, LocalFileOperation::Metadata)
                })?;
                directories.try_reserve(1).map_err(|_| resource_limit())?;
                directories.push((relative.to_path_buf(), identity));
            }
        }
        #[cfg(test)]
        expire_tree_sync_after_collection_if_scheduled();
        budget.check(self.directory().diagnostic_path())?;
        directories.sort_by_key(|(path, _)| std::cmp::Reverse(path.components().count()));
        budget.check(self.directory().diagnostic_path())?;
        Ok(directories)
    }

    /// Reopens and synchronizes collected directories deepest-first.
    ///
    /// # Parameters
    /// `directories` contains relative paths and identities captured during
    /// traversal. `budget` checks the shared cooperative deadline before
    /// and after each synchronization.
    fn sync_tree_directories(
        &self,
        directories: Vec<(std::path::PathBuf, crate::observation::LocalFileIdentity)>,
        budget: &TreeSyncBudget,
    ) -> LocalResult<()> {
        for (relative, expected_identity) in directories {
            budget.check(&self.directory().diagnostic_path().join(&relative))?;
            let directory = self
                .directory()
                .open_directory(&relative, &crate::options::LocalDirectoryOpenOptions::new())?;
            if directory.observe()?.identity() != Some(&expected_identity) {
                return Err(
                    LocalFileError::new(LocalFileErrorKind::ChangedDuringRead, LocalFileOperation::Metadata)
                        .with_path(self.directory().diagnostic_path().join(&relative)),
                );
            }
            budget.check(&self.directory().diagnostic_path().join(&relative))?;
            #[cfg(all(test, unix))]
            sync_order_tests::record_sync_event(sync_order_tests::SyncEvent {
                kind: sync_order_tests::SyncKind::Directory,
                path: relative.clone(),
                identity: expected_identity.clone(),
            })
            .map_err(|source| {
                LocalFileError::from_io(
                    LocalFileOperation::Commit,
                    Some(self.directory().diagnostic_path().join(&relative)),
                    None,
                    source,
                )
            })?;
            #[cfg(test)]
            let sync_result = {
                let file = directory.security_authority()?;
                crate::tests::native_io_fault_tests::check(crate::tests::native_io_fault_tests::NativeIoPoint::SyncAll)
                    .and_then(|()| file.sync_all())
            };
            #[cfg(not(test))]
            let sync_result = directory.security_authority()?.sync_all();
            sync_result.map_err(|source| {
                LocalFileError::from_io(
                    LocalFileOperation::Commit,
                    Some(self.directory().diagnostic_path().join(&relative)),
                    None,
                    source,
                )
            })?;
            budget.check(&self.directory().diagnostic_path().join(&relative))?;
        }
        Ok(())
    }

    /// Synchronizes the stage root and then its creating parent.
    ///
    /// # Parameters
    /// `parent` is the creating directory authority to synchronize after the
    /// stage root. `budget` checks the shared cooperative deadline
    /// throughout both synchronizations. `expected_stage_identity` is the
    /// identity previously observed for the stage root.
    fn sync_tree_root_and_parent(
        &self,
        parent: &LocalDirectoryHandle,
        budget: &TreeSyncBudget,
        expected_stage_identity: &crate::observation::LocalFileIdentity,
    ) -> LocalResult<()> {
        #[cfg(not(all(test, unix)))]
        let _ = expected_stage_identity;
        budget.check(self.directory().diagnostic_path())?;
        #[cfg(all(test, unix))]
        sync_order_tests::record_sync_event(sync_order_tests::SyncEvent {
            kind: sync_order_tests::SyncKind::Root,
            path: PathBuf::from("."),
            identity: expected_stage_identity.clone(),
        })
        .map_err(|source| {
            LocalFileError::from_io(
                LocalFileOperation::Commit,
                Some(self.directory().diagnostic_path().to_path_buf()),
                None,
                source,
            )
        })?;
        #[cfg(test)]
        let sync_result = {
            let file = self.directory().security_authority()?;
            crate::tests::native_io_fault_tests::check(crate::tests::native_io_fault_tests::NativeIoPoint::SyncAll)
                .and_then(|()| file.sync_all())
        };
        #[cfg(not(test))]
        let sync_result = self.directory().security_authority()?.sync_all();
        sync_result.map_err(|source| {
            LocalFileError::from_io(
                LocalFileOperation::Commit,
                Some(self.directory().diagnostic_path().to_path_buf()),
                None,
                source,
            )
        })?;
        budget.check(self.directory().diagnostic_path())?;
        budget.check(parent.diagnostic_path())?;
        #[cfg(all(test, unix))]
        sync_order_tests::record_sync_event(sync_order_tests::SyncEvent {
            kind: sync_order_tests::SyncKind::Parent,
            path: PathBuf::from("."),
            identity: parent
                .observe()?
                .identity()
                .cloned()
                .ok_or_else(|| LocalFileError::new(LocalFileErrorKind::Unsupported, LocalFileOperation::Metadata))?,
        })
        .map_err(|source| {
            LocalFileError::from_io(
                LocalFileOperation::Commit,
                Some(parent.diagnostic_path().to_path_buf()),
                None,
                source,
            )
        })?;
        #[cfg(test)]
        let sync_result = {
            let file = parent.security_authority()?;
            crate::tests::native_io_fault_tests::check(crate::tests::native_io_fault_tests::NativeIoPoint::SyncAll)
                .and_then(|()| file.sync_all())
        };
        #[cfg(not(test))]
        let sync_result = parent.security_authority()?.sync_all();
        sync_result.map_err(|source| {
            LocalFileError::from_io(
                LocalFileOperation::Commit,
                Some(parent.diagnostic_path().to_path_buf()),
                None,
                source,
            )
        })?;
        budget.check(parent.diagnostic_path())?;
        Ok(())
    }
}
