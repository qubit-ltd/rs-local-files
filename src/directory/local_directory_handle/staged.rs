// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
#[cfg(test)]
use std::cell::RefCell;
use std::path::Path;
use std::path::PathBuf;

use crate::LocalFileError;
use crate::LocalFileErrorKind;
use crate::LocalFileOperation;
use crate::directory::LocalDirectoryHandle;
use crate::directory::LocalStagedDirectory;
use crate::directory::tree_recovery::LocalTreeCreateError;
use crate::directory::tree_recovery::LocalTreeRetainedResource;
use crate::directory::tree_validation::LocalTreeValidationOptions;
use crate::error::LocalResult;
use crate::observation::LocalFileIdentity;
use crate::options::LocalCreateDirectoryOptions;
use crate::options::LocalDirectoryOpenOptions;
use crate::options::LocalTempCleanupLimits;
use crate::options::LocalTempDirectoryOptions;
use crate::outcome::LocalTreeMutationState;
#[cfg(test)]
type StageCreatedHook = Box<dyn FnOnce(&Path)>;
#[cfg(test)]
thread_local! {
    static STAGE_CREATED_HOOK: RefCell<Option<StageCreatedHook>> = const { RefCell::new(None) };
}

#[cfg(test)]
pub(crate) struct StageCreatedHookGuard;

#[cfg(test)]
impl Drop for StageCreatedHookGuard {
    fn drop(&mut self) {
        STAGE_CREATED_HOOK.with(|slot| slot.borrow_mut().take());
    }
}

impl LocalDirectoryHandle {
    #[cfg(test)]
    pub(in crate::directory) fn install_stage_created_hook(
        hook: impl FnOnce(&Path) + 'static,
    ) -> StageCreatedHookGuard {
        STAGE_CREATED_HOOK.with(|slot| {
            assert!(slot.borrow().is_none(), "stage creation hook already installed");
            *slot.borrow_mut() = Some(Box::new(hook));
        });
        StageCreatedHookGuard
    }

    #[cfg(test)]
    pub(in crate::directory) fn run_stage_created_hook(name: &Path) {
        let hook = STAGE_CREATED_HOOK.with(|slot| slot.borrow_mut().take());
        if let Some(hook) = hook {
            hook(name);
        }
    }

    /// Reopens a stage beneath this authority only if its identity still
    /// matches `expected`.
    /// If reopening or validation fails, this method leaves the named stage in
    /// place so the caller can retry, inspect, or clean it up explicitly.
    pub fn open_staged_directory(
        &self,
        relative: &Path,
        expected: &LocalFileIdentity,
        options: &LocalTreeValidationOptions,
    ) -> LocalResult<LocalStagedDirectory> {
        let directory = self.open_directory(relative, &LocalDirectoryOpenOptions::new())?;
        let observation = directory.observe()?;
        if observation.identity() != Some(expected) {
            return Err(
                LocalFileError::new(LocalFileErrorKind::RequirementNotMet, LocalFileOperation::Metadata)
                    .with_path(self.diagnostic_path().join(relative)),
            );
        }
        let private = options.privacy() == crate::policy::LocalPrivacyRequirement::Required;
        if private && options.owner_access_policy().is_none() {
            #[cfg(unix)]
            let file = directory.root_authority()?.try_clone_authority().map_err(|source| {
                LocalFileError::from_io(
                    LocalFileOperation::Metadata,
                    Some(directory.diagnostic_path().to_path_buf()),
                    None,
                    source,
                )
            })?;
            #[cfg(unix)]
            {
                use std::os::unix::fs::MetadataExt;
                let metadata = file.metadata().map_err(|source| {
                    LocalFileError::from_io(
                        LocalFileOperation::Metadata,
                        Some(directory.diagnostic_path().to_path_buf()),
                        None,
                        source,
                    )
                })?;
                if metadata.mode() & 0o077 != 0 || metadata.uid() != unsafe { libc::getuid() } {
                    return Err(LocalFileError::new(
                        LocalFileErrorKind::RequirementNotMet,
                        LocalFileOperation::Metadata,
                    ));
                }
            }
            #[cfg(windows)]
            {
                let parent_file = self.root_authority()?.try_clone_authority().map_err(|source| {
                    LocalFileError::from_io(
                        LocalFileOperation::Metadata,
                        Some(directory.diagnostic_path().to_path_buf()),
                        None,
                        source,
                    )
                })?;
                let access_file =
                    crate::temp::internal::privacy_windows::open_existing_relative(&parent_file, relative, true)
                        .map_err(|source| {
                            LocalFileError::from_io(
                                LocalFileOperation::Metadata,
                                Some(directory.diagnostic_path().to_path_buf()),
                                None,
                                source,
                            )
                        })?;
                let access_observation = crate::observation::query::query_observation(
                    &access_file,
                    crate::observation::LocalObservationRequirement::Required,
                )?;
                if access_observation.identity() != Some(expected) {
                    return Err(LocalFileError::new(
                        LocalFileErrorKind::ChangedDuringRead,
                        LocalFileOperation::Metadata,
                    ));
                }
                crate::temp::internal::privacy_windows::verify_current_user_access(&access_file, true, false).map_err(
                    |source| {
                        LocalFileError::from_io(
                            LocalFileOperation::Metadata,
                            Some(directory.diagnostic_path().to_path_buf()),
                            None,
                            source,
                        )
                    },
                )?;
            }
        }
        let identity = observation.identity().cloned();
        let path = self.diagnostic_path().join(relative);
        let mut owner = LocalTreeRetainedResource::new_for_reopen(
            path,
            relative.to_path_buf(),
            identity.clone(),
            self.clone(),
            Some(directory.clone()),
            LocalTempCleanupLimits::new(),
        );
        owner.set_owner_access_policy(options.owner_access_policy().copied());
        let mut stage = LocalStagedDirectory::new(
            owner,
            directory,
            relative.to_path_buf(),
            identity,
            private,
            options.owner_access_policy().copied(),
        );
        super::super::internal::tree_verification::verify_tree(&stage, options.exact_entries(), options)?;
        stage.arm_after_reopen_validation();
        Ok(stage)
    }

    /// Creates a uniquely named stage directly beneath this fixed directory
    /// authority. The `parent` and `create_parent` options do not select or
    /// create an alternate parent for this fixed-authority entry point.
    pub fn create_staged_directory(
        &self,
        options: &LocalTempDirectoryOptions,
    ) -> Result<LocalStagedDirectory, LocalTreeCreateError> {
        if options.max_attempts() == Some(0) {
            return Err(LocalTreeCreateError::new(
                LocalFileError::new(LocalFileErrorKind::InvalidOptions, LocalFileOperation::GenerateName),
                None,
                Vec::new(),
                LocalTreeMutationState::NotStarted,
            ));
        }
        if let Some(policy) = options.owner_access_policy()
            && let Err(cause) = LocalStagedDirectory::validate_owner_policy(policy)
        {
            return Err(LocalTreeCreateError::new(
                cause,
                None,
                Vec::new(),
                LocalTreeMutationState::NotStarted,
            ));
        }
        let parent = self
            .root_authority()
            .map_err(|cause| LocalTreeCreateError::new(cause, None, Vec::new(), LocalTreeMutationState::NotStarted))?;
        let mut created_file = None;
        let mut relative = None;
        let mut attempts = 0_usize;
        while options.max_attempts().is_none_or(|maximum| attempts < maximum) {
            attempts = attempts.saturating_add(1);
            let name = LocalStagedDirectory::stage_name(options).map_err(|cause| {
                LocalTreeCreateError::new(cause, None, Vec::new(), LocalTreeMutationState::NotStarted)
            })?;
            #[cfg(all(test, feature = "test-support"))]
            if crate::local::take_repeated_test_support("staged-directory-collision") {
                continue;
            }
            if options.privacy() == crate::policy::LocalPrivacyRequirement::Required
                || options.owner_access_policy().is_some()
            {
                match LocalStagedDirectory::create_stage_directory(parent, &name, options.owner_access_policy()) {
                    Ok(file) => {
                        created_file = Some(file);
                        relative = Some(PathBuf::from(name));
                        break;
                    }
                    Err(error)
                        if error.source.kind() == std::io::ErrorKind::AlreadyExists && error.retained.is_none() =>
                    {
                        continue;
                    }
                    Err(error) => {
                        let path = self.diagnostic_path().join(&name);
                        let mutation_state = if error.retained.is_some() {
                            LocalTreeMutationState::Started
                        } else {
                            LocalTreeMutationState::NotStarted
                        };
                        let identity = error.retained.as_ref().and_then(|entry| entry.identity.clone());
                        let retained = identity
                            .clone()
                            .map(|identity| {
                                LocalTreeRetainedResource::new(
                                    path.clone(),
                                    PathBuf::from(&name),
                                    Some(identity),
                                    self.clone(),
                                    None,
                                    options.cleanup_limits(),
                                )
                                .with_owner_access_policy(options.owner_access_policy().copied())
                            })
                            .into_iter()
                            .collect();
                        let recovery = error
                            .retained
                            .map(|entry| crate::LocalTempRecovery::from_native(path.clone(), entry));
                        let mut cause = LocalFileError::from_io(
                            LocalFileOperation::CreateTempDirectory,
                            Some(path),
                            None,
                            error.source,
                        );
                        if let Some(recovery) = recovery {
                            cause = cause.with_temp_recovery(recovery);
                        }
                        return Err(LocalTreeCreateError::new(cause, None, retained, mutation_state));
                    }
                }
            } else {
                let path = PathBuf::from(&name);
                match self
                    .filesystem()
                    .create_directory_with_options(&path, &LocalCreateDirectoryOptions::new())
                {
                    Ok(_outcome) => {
                        relative = Some(path);
                        break;
                    }
                    Err(error) if error.kind() == LocalFileErrorKind::AlreadyExists => continue,
                    Err(error) => {
                        let state = match error.effect_state() {
                            Some(crate::error::LocalFileEffectState::Unchanged) => LocalTreeMutationState::NotStarted,
                            Some(
                                crate::error::LocalFileEffectState::PartiallyApplied
                                | crate::error::LocalFileEffectState::Applied,
                            ) => LocalTreeMutationState::Started,
                            Some(crate::error::LocalFileEffectState::Indeterminate) | None => {
                                LocalTreeMutationState::Indeterminate
                            }
                        };
                        return Err(LocalTreeCreateError::new(error, None, Vec::new(), state));
                    }
                }
            }
        }
        let Some(relative) = relative else {
            return Err(LocalTreeCreateError::new(
                LocalFileError::new(
                    LocalFileErrorKind::AlreadyExists,
                    LocalFileOperation::CreateTempDirectory,
                ),
                None,
                Vec::new(),
                LocalTreeMutationState::NotStarted,
            ));
        };
        let path = self.diagnostic_path().join(&relative);
        let mut default_directory = if created_file.is_none() {
            match self.open_directory(&relative, &LocalDirectoryOpenOptions::new()) {
                Ok(directory) => Some(directory),
                Err(cause) => {
                    let resource = LocalTreeRetainedResource::new(
                        path.clone(),
                        relative.clone(),
                        None,
                        self.clone(),
                        None,
                        options.cleanup_limits(),
                    );
                    return Err(LocalTreeCreateError::new(
                        cause,
                        None,
                        vec![resource],
                        LocalTreeMutationState::Started,
                    ));
                }
            }
        } else {
            None
        };
        let created_identity = if let Some(file) = created_file.as_ref() {
            let observation = match crate::observation::query::query_observation(
                file,
                crate::observation::LocalObservationRequirement::Required,
            ) {
                Ok(observation) => observation,
                Err(cause) => {
                    let owner = LocalTreeRetainedResource::new(
                        path.clone(),
                        relative.clone(),
                        None,
                        self.clone(),
                        None,
                        options.cleanup_limits(),
                    )
                    .with_owner_access_policy(options.owner_access_policy().copied());
                    return Err(LocalTreeCreateError::new(
                        cause,
                        None,
                        vec![owner],
                        LocalTreeMutationState::Started,
                    ));
                }
            };
            match observation.identity().cloned() {
                Some(identity) => Some(identity),
                None => {
                    let owner = LocalTreeRetainedResource::new(
                        path.clone(),
                        relative.clone(),
                        None,
                        self.clone(),
                        None,
                        options.cleanup_limits(),
                    )
                    .with_owner_access_policy(options.owner_access_policy().copied());
                    return Err(LocalTreeCreateError::new(
                        LocalFileError::new(LocalFileErrorKind::Unsupported, LocalFileOperation::Metadata)
                            .with_path(path.clone()),
                        None,
                        vec![owner],
                        LocalTreeMutationState::Started,
                    ));
                }
            }
        } else {
            let observation = match default_directory
                .as_ref()
                .expect("default stage directory is open")
                .observe()
            {
                Ok(observation) => observation,
                Err(cause) => {
                    let resource = LocalTreeRetainedResource::new(
                        path.clone(),
                        relative.clone(),
                        None,
                        self.clone(),
                        default_directory.take(),
                        options.cleanup_limits(),
                    );
                    return Err(LocalTreeCreateError::new(
                        cause,
                        None,
                        vec![resource],
                        LocalTreeMutationState::Started,
                    ));
                }
            };
            match observation.identity().cloned() {
                Some(identity) => Some(identity),
                None => {
                    let resource = LocalTreeRetainedResource::new(
                        path.clone(),
                        relative.clone(),
                        None,
                        self.clone(),
                        default_directory.take(),
                        options.cleanup_limits(),
                    );
                    return Err(LocalTreeCreateError::new(
                        LocalFileError::new(LocalFileErrorKind::Unsupported, LocalFileOperation::Metadata)
                            .with_path(path.clone()),
                        None,
                        vec![resource],
                        LocalTreeMutationState::Started,
                    ));
                }
            }
        };
        #[cfg(test)]
        Self::run_stage_created_hook(&relative);
        let directory = match default_directory
            .take()
            .map_or_else(|| self.open_directory(&relative, &LocalDirectoryOpenOptions::new()), Ok)
        {
            Ok(directory) => directory,
            Err(cause) => {
                let identity = created_identity.clone().or_else(|| {
                    self.observe_entry(&relative)
                        .ok()
                        .and_then(|observation| observation.identity().cloned())
                });
                let resource = LocalTreeRetainedResource::new(
                    path,
                    relative,
                    identity,
                    self.clone(),
                    None,
                    options.cleanup_limits(),
                )
                .with_owner_access_policy(options.owner_access_policy().copied());
                return Err(LocalTreeCreateError::new(
                    cause,
                    None,
                    vec![resource],
                    LocalTreeMutationState::Started,
                ));
            }
        };
        let observation = match directory.observe() {
            Ok(observation) => observation,
            Err(cause) => {
                let identity = created_identity.clone().or_else(|| {
                    self.observe_entry(&relative)
                        .ok()
                        .and_then(|observation| observation.identity().cloned())
                });
                let child = if created_identity.is_some() {
                    None
                } else {
                    Some(directory.clone())
                };
                let resource = LocalTreeRetainedResource::new(
                    path,
                    relative,
                    identity,
                    self.clone(),
                    child,
                    options.cleanup_limits(),
                )
                .with_owner_access_policy(options.owner_access_policy().copied());
                return Err(LocalTreeCreateError::new(
                    cause,
                    None,
                    vec![resource],
                    LocalTreeMutationState::Started,
                ));
            }
        };
        if created_identity
            .as_ref()
            .is_some_and(|identity| observation.identity() != Some(identity))
        {
            let owner = LocalTreeRetainedResource::new(
                path.clone(),
                relative.clone(),
                created_identity.clone(),
                self.clone(),
                None,
                options.cleanup_limits(),
            )
            .with_owner_access_policy(options.owner_access_policy().copied());
            return Err(LocalTreeCreateError::new(
                LocalFileError::new(LocalFileErrorKind::ChangedDuringRead, LocalFileOperation::Metadata)
                    .with_path(path),
                None,
                vec![owner],
                LocalTreeMutationState::Indeterminate,
            ));
        }
        let named_observation = match self.observe_entry(&relative) {
            Ok(observation) => observation,
            Err(cause) => {
                let owner = LocalTreeRetainedResource::new(
                    path.clone(),
                    relative.clone(),
                    created_identity.clone(),
                    self.clone(),
                    None,
                    options.cleanup_limits(),
                )
                .with_owner_access_policy(options.owner_access_policy().copied());
                return Err(LocalTreeCreateError::new(
                    cause,
                    None,
                    vec![owner],
                    LocalTreeMutationState::Indeterminate,
                ));
            }
        };
        if created_identity
            .as_ref()
            .is_some_and(|identity| named_observation.identity() != Some(identity))
        {
            let owner = LocalTreeRetainedResource::new(
                path.clone(),
                relative.clone(),
                created_identity.clone(),
                self.clone(),
                None,
                options.cleanup_limits(),
            )
            .with_owner_access_policy(options.owner_access_policy().copied());
            return Err(LocalTreeCreateError::new(
                LocalFileError::new(LocalFileErrorKind::ChangedDuringRead, LocalFileOperation::Metadata)
                    .with_path(path),
                None,
                vec![owner],
                LocalTreeMutationState::Indeterminate,
            ));
        }
        let identity = created_identity.or_else(|| observation.identity().cloned());
        let mut owner = LocalTreeRetainedResource::new(
            path,
            relative.clone(),
            identity.clone(),
            self.clone(),
            Some(directory.clone()),
            options.cleanup_limits(),
        );
        owner.set_owner_access_policy(options.owner_access_policy().copied());
        Ok(LocalStagedDirectory::new(
            owner,
            directory,
            relative,
            identity,
            options.privacy() == crate::policy::LocalPrivacyRequirement::Required,
            options.owner_access_policy().copied(),
        ))
    }
}
