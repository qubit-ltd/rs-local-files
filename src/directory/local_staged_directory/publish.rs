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
        let rename_options = crate::options::LocalRenameOptions::new().with_durability(options.durability());
        match parent
            .filesystem()
            .rename_with_options(&source, &target, &rename_options)
        {
            Ok(outcome) => {
                if let Some(owner) = self.owner.as_mut() {
                    owner.disarm_after_publish();
                }
                self.owner.take();
                drop(self.directory.take());
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
                let (cause, state) = failure.into_parts();
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
