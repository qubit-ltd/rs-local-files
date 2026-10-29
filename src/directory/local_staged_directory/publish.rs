// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
use std::ffi::OsStr;
use std::path::Path;
use std::path::PathBuf;

use super::super::tree_validation::LocalTreeValidationOptions;
use super::LocalStagedDirectory;
use super::validate_owner_policy;
use crate::LocalFileError;
use crate::LocalFileErrorKind;
use crate::LocalFileOperation;
use crate::directory::LocalDirectoryHandle;
use crate::options::LocalListOptions;

impl LocalStagedDirectory {
    /// Publishes this stage under a single native name without replacing an
    /// entry.
    ///
    /// On Apple platforms, a read-only stage root is temporarily writable for
    /// the rename and is made read-only through its retained handle afterward.
    /// The published name can therefore briefly expose a writable root. If the
    /// final permission change fails, the error reports that publication
    /// occurred.
    pub fn publish_new(
        mut self,
        parent: &LocalDirectoryHandle,
        name: &OsStr,
        options: &crate::options::LocalTreePublishOptions,
    ) -> Result<crate::outcome::LocalTreePublishOutcome, super::super::tree_recovery::LocalTreePublishError> {
        use crate::outcome::LocalTreeAccessState as Access;
        use crate::outcome::LocalTreeMutationState as Mutation;
        use crate::outcome::LocalTreePublication as Publication;
        let invalid_name = || LocalFileError::new(LocalFileErrorKind::InvalidPath, LocalFileOperation::ValidateName);
        let name_path = Path::new(name);
        if let Err(cause) = parent.validate_direct_input(name_path) {
            return Err(super::super::tree_recovery::LocalTreePublishError::new(
                cause,
                Some(self),
                Mutation::NotStarted,
                Publication::NotPublished,
                Access::Preserved,
            ));
        }
        if options.owner_access_policy().is_some() && options.access() != crate::policy::LocalTreeFinalAccess::Preserve
        {
            return Err(super::super::tree_recovery::LocalTreePublishError::new(
                LocalFileError::new(LocalFileErrorKind::InvalidOptions, LocalFileOperation::ValidateName),
                Some(self),
                Mutation::NotStarted,
                Publication::NotPublished,
                Access::Preserved,
            ));
        }
        if let Some(policy) = options.owner_access_policy()
            && let Err(cause) = validate_owner_policy(policy)
        {
            return Err(super::super::tree_recovery::LocalTreePublishError::new(
                cause,
                Some(self),
                Mutation::NotStarted,
                Publication::NotPublished,
                Access::Preserved,
            ));
        }
        if let Some(policy) = options.owner_access_policy() {
            self.owner_access_policy = Some(*policy);
        }
        if let (Some(policy), Some(owner)) = (options.owner_access_policy(), self.owner.as_mut()) {
            owner.set_owner_access_policy(Some(*policy));
        }
        if name.is_empty()
            || name == OsStr::new(".")
            || name == OsStr::new("..")
            || name_path.components().count() != 1
            || !matches!(name_path.components().next(), Some(std::path::Component::Normal(_)))
        {
            return Err(super::super::tree_recovery::LocalTreePublishError::new(
                invalid_name(),
                Some(self),
                Mutation::NotStarted,
                Publication::NotPublished,
                Access::Preserved,
            ));
        }
        let parent_matches = match self
            .owner
            .as_ref()
            .expect("stage cleanup owner exists")
            .parent_matches(parent)
        {
            Ok(matches) => matches,
            Err(cause) => {
                return Err(super::super::tree_recovery::LocalTreePublishError::new(
                    cause,
                    Some(self),
                    Mutation::NotStarted,
                    Publication::NotPublished,
                    Access::Preserved,
                ));
            }
        };
        if !parent_matches {
            return Err(super::super::tree_recovery::LocalTreePublishError::new(
                LocalFileError::new(LocalFileErrorKind::InvalidState, LocalFileOperation::Rename),
                Some(self),
                Mutation::NotStarted,
                Publication::NotPublished,
                Access::Preserved,
            ));
        }
        let source = PathBuf::from(self.relative_name());
        let target = PathBuf::from(name);
        let observed = match parent.observe_entry(&source) {
            Ok(observed) => observed,
            Err(cause) => {
                return Err(super::super::tree_recovery::LocalTreePublishError::new(
                    cause,
                    Some(self),
                    Mutation::NotStarted,
                    Publication::NotPublished,
                    Access::Preserved,
                ));
            }
        };
        if observed.identity() != self.identity.as_ref() || self.identity.is_none() {
            return Err(super::super::tree_recovery::LocalTreePublishError::new(
                LocalFileError::new(LocalFileErrorKind::ChangedDuringRead, LocalFileOperation::Metadata),
                Some(self),
                Mutation::NotStarted,
                Publication::NotPublished,
                Access::Preserved,
            ));
        }
        if let Some(validation) = options.validation()
            && let Err(cause) =
                super::super::internal::tree_verification::verify_tree(&self, validation.exact_entries(), validation)
        {
            return Err(super::super::tree_recovery::LocalTreePublishError::new(
                cause,
                Some(self),
                Mutation::NotStarted,
                Publication::NotPublished,
                Access::Preserved,
            ));
        }
        let default_limits = LocalListOptions::new();
        let access_limits = options
            .validation()
            .map_or(&default_limits, LocalTreeValidationOptions::limits);
        let (access_state, file_synced) = if let Some(policy) = options.owner_access_policy() {
            let synced = match self.sync_tree_files(access_limits, options.file_sync()) {
                Ok(synced) => synced,
                Err(cause) => {
                    return Err(super::super::tree_recovery::LocalTreePublishError::new(
                        cause,
                        Some(self),
                        Mutation::NotStarted,
                        Publication::NotPublished,
                        Access::Preserved,
                    ));
                }
            };
            match self.apply_owner_access_policy_with_sync(policy, access_limits, options.file_sync(), synced) {
                Ok((outcome, file_synced)) => (outcome.access_state(), file_synced),
                Err(error) => {
                    let (cause, access_state, _) = error.into_parts();
                    return Err(super::super::tree_recovery::LocalTreePublishError::new(
                        cause,
                        Some(self),
                        if access_state == Access::Indeterminate {
                            Mutation::Indeterminate
                        } else if access_state == Access::Preserved {
                            Mutation::NotStarted
                        } else {
                            Mutation::Started
                        },
                        Publication::NotPublished,
                        access_state,
                    ));
                }
            }
        } else if options.access() == crate::policy::LocalTreeFinalAccess::Preserve {
            let file_synced = match self.sync_tree_files(access_limits, options.file_sync()) {
                Ok(synced) => synced,
                Err(cause) => {
                    return Err(super::super::tree_recovery::LocalTreePublishError::new(
                        cause,
                        Some(self),
                        Mutation::NotStarted,
                        Publication::NotPublished,
                        Access::Preserved,
                    ));
                }
            };
            (Access::Preserved, file_synced)
        } else {
            match self.apply_access_policy_with_sync(options.access(), access_limits, options.file_sync()) {
                Ok((outcome, synced)) => (outcome.access_state(), synced),
                Err(error) => {
                    let (cause, access_state, _) = error.into_parts();
                    return Err(super::super::tree_recovery::LocalTreePublishError::new(
                        cause,
                        Some(self),
                        match access_state {
                            Access::Preserved => Mutation::NotStarted,
                            Access::CurrentUserOnly | Access::ReadOnlyCurrentUser | Access::Mixed => Mutation::Started,
                            Access::OwnerPolicyApplied => Mutation::Started,
                            Access::Indeterminate => Mutation::Indeterminate,
                        },
                        Publication::NotPublished,
                        access_state,
                    ));
                }
            }
        };
        let access_mutation = if access_state == Access::Preserved {
            Mutation::NotStarted
        } else {
            Mutation::Started
        };
        #[cfg(feature = "test-support")]
        if options.owner_access_policy().is_some()
            && crate::local::take_test_support("owner-policy-replace-source-before-rename")
        {
            let native = self.directory().diagnostic_path().to_path_buf();
            let mut backup_name = native.as_os_str().to_os_string();
            backup_name.push(".owner-policy-original");
            let backup = PathBuf::from(backup_name);
            if let Err(source) = std::fs::rename(&native, &backup).and_then(|()| std::fs::create_dir(&native)) {
                return Err(super::super::tree_recovery::LocalTreePublishError::new(
                    LocalFileError::from_io(LocalFileOperation::Metadata, Some(native), None, source),
                    Some(self),
                    Mutation::Indeterminate,
                    Publication::NotPublished,
                    access_state,
                ));
            }
        }
        let final_source = match parent.observe_entry(&source) {
            Ok(observation) if observation.identity() == self.identity.as_ref() => true,
            Ok(_) => false,
            Err(cause) => {
                return Err(super::super::tree_recovery::LocalTreePublishError::new(
                    cause,
                    Some(self),
                    Mutation::Indeterminate,
                    Publication::NotPublished,
                    access_state,
                ));
            }
        };
        if !final_source {
            return Err(super::super::tree_recovery::LocalTreePublishError::new(
                LocalFileError::new(LocalFileErrorKind::ChangedDuringRead, LocalFileOperation::Metadata),
                Some(self),
                Mutation::Indeterminate,
                Publication::NotPublished,
                access_state,
            ));
        }
        #[cfg(any(target_os = "macos", target_os = "ios", all(test, unix)))]
        let writable_root = if options.access() == crate::policy::LocalTreeFinalAccess::ReadOnlyCurrentUser {
            let root = match self.directory().root_authority().and_then(|root| {
                root.try_clone_authority().map_err(|source| {
                    LocalFileError::from_io(
                        LocalFileOperation::Metadata,
                        Some(self.directory().diagnostic_path().to_path_buf()),
                        None,
                        source,
                    )
                })
            }) {
                Ok(root) => root,
                Err(cause) => {
                    return Err(super::super::tree_recovery::LocalTreePublishError::new(
                        cause,
                        Some(self),
                        Mutation::Started,
                        Publication::NotPublished,
                        access_state,
                    ));
                }
            };
            if let Err(source) = crate::temp::internal::privacy_unix::apply_current_user_access(&root, true, false) {
                let cause = LocalFileError::from_io(
                    LocalFileOperation::Metadata,
                    Some(self.directory().diagnostic_path().to_path_buf()),
                    None,
                    source,
                );
                return Err(super::super::tree_recovery::LocalTreePublishError::new(
                    cause,
                    Some(self),
                    Mutation::Started,
                    Publication::NotPublished,
                    Access::Indeterminate,
                ));
            }
            Some(root)
        } else {
            None
        };
        let rename_options = crate::options::LocalRenameOptions::new().with_durability(options.durability());
        let rename_result = parent
            .filesystem()
            .rename_with_options(&source, &target, &rename_options);
        #[cfg(any(target_os = "macos", target_os = "ios", all(test, unix)))]
        let access_path = match &rename_result {
            Err(failure) if failure.state() == crate::outcome::LocalRenameFailureState::Unchanged => &source,
            _ => &target,
        };
        #[cfg(any(target_os = "macos", target_os = "ios", all(test, unix)))]
        let restore_error = writable_root.and_then(|root| {
            crate::temp::internal::privacy_unix::apply_current_user_access(&root, true, true)
                .err()
                .map(|source| {
                    LocalFileError::from_io(
                        LocalFileOperation::Metadata,
                        Some(parent.diagnostic_path().join(access_path)),
                        None,
                        source,
                    )
                })
        });
        #[cfg(not(any(target_os = "macos", target_os = "ios", all(test, unix))))]
        let restore_error: Option<LocalFileError> = None;
        match rename_result {
            Ok(outcome) => {
                if let Some(owner) = self.owner.as_mut() {
                    owner.disarm_after_publish();
                }
                self.owner.take();
                drop(self.directory.take());
                if let Some(cause) = restore_error {
                    return Err(super::super::tree_recovery::LocalTreePublishError::new(
                        cause,
                        None,
                        Mutation::Started,
                        Publication::Published,
                        Access::Indeterminate,
                    ));
                }
                let mut result = crate::outcome::LocalTreePublishOutcome::new(
                    Publication::Published,
                    outcome.durable(),
                    file_synced,
                    access_state,
                );
                result.set_owner_access_policy(options.owner_access_policy().copied());
                Ok(result)
            }
            Err(failure) => {
                let (mut cause, state) = failure.into_parts();
                let access_state = if let Some(restore_error) = restore_error {
                    cause = cause.with_cleanup_error(restore_error);
                    Access::Indeterminate
                } else {
                    access_state
                };
                let (publication, mutation, keep_stage) = match state {
                    crate::outcome::LocalRenameFailureState::Unchanged => {
                        (Publication::NotPublished, access_mutation, true)
                    }
                    crate::outcome::LocalRenameFailureState::Renamed => {
                        (Publication::Published, Mutation::Started, false)
                    }
                    crate::outcome::LocalRenameFailureState::Indeterminate => {
                        (Publication::Indeterminate, Mutation::Indeterminate, false)
                    }
                };
                if keep_stage {
                    Err(super::super::tree_recovery::LocalTreePublishError::new(
                        cause,
                        Some(self),
                        mutation,
                        publication,
                        access_state,
                    ))
                } else {
                    let mut retained = self.owner.take();
                    if let Some(owner) = retained.as_mut() {
                        owner.disarm_after_publish();
                    }
                    drop(self.directory.take());
                    Err(super::super::tree_recovery::LocalTreePublishError::new(
                        cause,
                        None,
                        mutation,
                        publication,
                        access_state,
                    )
                    .with_retained(retained))
                }
            }
        }
    }
}

#[cfg(all(test, unix))]
mod native_publish_failure_tests {
    use std::ffi::OsStr;

    use crate::LocalFileErrorKind;
    use crate::LocalFileOperation;
    use crate::LocalFileSystem;
    use crate::options::LocalDirectoryOpenOptions;
    use crate::options::LocalTempDirectoryOptions;
    use crate::options::LocalTreePublishOptions;
    use crate::outcome::LocalTreeAccessState;
    use crate::outcome::LocalTreeMutationState;
    use crate::outcome::LocalTreePublication;
    use crate::policy::LocalTreeFinalAccess;
    use crate::tests::native_io_fault_tests::NativeIoFaultGuard;
    use crate::tests::native_io_fault_tests::NativeIoPoint;

    #[test]
    fn test_publish_readonly_root_clone_failure_retains_access_changed_stage() {
        let fixture = tempfile::tempdir().expect("fixture");
        let parent = LocalFileSystem::host()
            .expect("host")
            .open_directory_with_options(fixture.path(), &LocalDirectoryOpenOptions::new())
            .expect("parent");
        let stage = parent
            .create_staged_directory(&LocalTempDirectoryOptions::new())
            .expect("stage");
        let path = stage.directory().diagnostic_path().to_path_buf();
        // Two parent observations and two root access handles precede the
        // publication-only writable root acquisition for this empty tree.
        let guard = NativeIoFaultGuard::fail_on(NativeIoPoint::CloneAuthority, 5);
        let error = stage
            .publish_new(
                &parent,
                OsStr::new("published"),
                &LocalTreePublishOptions::new().with_access(LocalTreeFinalAccess::ReadOnlyCurrentUser),
            )
            .expect_err("publication root authority clone fails after access application");
        assert!(guard.fired());
        assert_eq!(error.cause().kind(), LocalFileErrorKind::Io);
        assert_eq!(error.cause().operation(), LocalFileOperation::Metadata);
        assert_eq!(error.cause().path(), Some(path.as_path()));
        assert_eq!(error.mutation_state(), LocalTreeMutationState::Started);
        assert_eq!(error.publication(), LocalTreePublication::NotPublished);
        assert_eq!(error.access_state(), LocalTreeAccessState::ReadOnlyCurrentUser);
        assert!(!fixture.path().join("published").exists());
        drop(guard);
        let stage = error.into_stage().expect("failed publication retains stage owner");
        stage.cleanup().expect("read-only stage remains recoverable");
        assert!(!path.exists());
    }
}
