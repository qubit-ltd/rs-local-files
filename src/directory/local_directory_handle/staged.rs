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

#[cfg(all(test, target_os = "linux"))]
mod owner_policy_failed_open_tests {
    use std::fs;
    use std::os::fd::AsRawFd;
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::fs::MetadataExt;
    use std::path::Path;

    use crate::LocalFileSystem;
    use crate::options::LocalDirectoryOpenOptions;
    use crate::options::LocalTempDirectoryOptions;
    use crate::policy::LocalOwnerAccessPolicy;

    fn create_failed_stage() -> (tempfile::TempDir, crate::directory::tree_recovery::LocalTreeCreateError) {
        let fixture = tempfile::tempdir().expect("temporary directory");
        fs::write(fixture.path().join("sentinel"), b"independent sentinel").expect("sentinel");
        let filesystem = LocalFileSystem::rooted(fixture.path()).expect("rooted filesystem");
        let parent = filesystem
            .open_directory_with_options(Path::new("/"), &LocalDirectoryOpenOptions::new())
            .expect("open parent");
        crate::temp::internal::privacy_unix::install_owner_open_failure_hook(|parent, name| {
            let c_name = std::ffi::CString::new(name.as_bytes()).expect("stage name contains no NUL");
            let original = Path::new("/proc/self/fd")
                .join(parent.as_raw_fd().to_string())
                .join(name);
            fs::write(original.join("payload"), b"original payload").expect("original payload");
            let moved = std::ffi::CString::new("moved-original").expect("static name");
            // SAFETY: names are NUL terminated and parent is an open directory.
            assert_eq!(
                unsafe { libc::renameat(parent.as_raw_fd(), c_name.as_ptr(), parent.as_raw_fd(), moved.as_ptr(),) },
                0
            );
            // SAFETY: same authority and a valid directory mode.
            assert_eq!(unsafe { libc::mkdirat(parent.as_raw_fd(), c_name.as_ptr(), 0o700) }, 0);
            Err(std::io::Error::other("injected directory open failure"))
        });
        let options = LocalTempDirectoryOptions::new()
            .with_owner_access_policy(LocalOwnerAccessPolicy::new().with_unix_modes(0o700, 0o600));
        let error = parent
            .create_staged_directory(&options)
            .expect_err("injected open failure");
        (fixture, error)
    }

    #[test]
    fn owner_policy_failed_open_does_not_arm_tree_cleanup_for_replacement() {
        use crate::outcome::LocalTreeMutationState;

        let (fixture, error) = create_failed_stage();
        assert_eq!(error.mutation_state(), LocalTreeMutationState::Started);
        assert_eq!(error.cause().io_error().and_then(std::io::Error::raw_os_error), None);
        let retained_path = error.cause().path().expect("failed stage path").to_path_buf();
        let replacement = fs::metadata(&retained_path).expect("replacement metadata");
        let replacement_identity = crate::observation::LocalFileIdentity::unix(replacement.dev(), replacement.ino());
        assert!(
            error.retained_resources().is_empty(),
            "unproven failed-open entry must not arm tree cleanup"
        );
        assert!(error.cause().temp_recovery().is_some(), "native recovery is retained");
        assert_eq!(
            fs::read(fixture.path().join("moved-original/payload")).expect("moved original"),
            b"original payload"
        );
        assert_eq!(
            fs::read(fixture.path().join("sentinel")).expect("sentinel survives"),
            b"independent sentinel"
        );

        let (cause, _, owners) = error.into_parts();
        assert!(owners.is_empty(), "failed open has no verifiable tree owner");
        let (_, mut recovery) = cause.into_temp_recovery_parts();
        let mut recovery = recovery.take().expect("native recovery owner");
        assert!(recovery.identity().is_none(), "replacement identity is unproven");
        assert_ne!(Some(&replacement_identity), recovery.identity());
        assert!(recovery.cleanup().is_err(), "unknown identity must refuse cleanup");
        assert!(retained_path.is_dir(), "explicit recovery must leave replacement");
        drop(recovery);
        assert!(retained_path.is_dir(), "recovery Drop must leave replacement");
    }

    #[test]
    fn owner_policy_failed_open_drop_does_not_delete_replacement() {
        let (fixture, error) = create_failed_stage();
        let retained_path = error.cause().path().expect("failed stage path").to_path_buf();
        drop(error);
        assert!(retained_path.is_dir(), "Drop must not delete the replacement directory");
        assert_eq!(
            fs::read(fixture.path().join("moved-original/payload")).expect("moved original"),
            b"original payload"
        );
        assert_eq!(
            fs::read(fixture.path().join("sentinel")).expect("sentinel survives"),
            b"independent sentinel"
        );
    }
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
        let observation = directory.observe().map_err(|error| {
            if error.path().is_none() {
                error.with_path(self.diagnostic_path().join(relative))
            } else {
                error
            }
        })?;
        if observation.identity() != Some(expected) {
            return Err(
                LocalFileError::new(LocalFileErrorKind::RequirementNotMet, LocalFileOperation::Metadata)
                    .with_path(self.diagnostic_path().join(relative))
                    .with_reason("opened stage identity does not match the expected identity"),
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
                #[cfg(test)]
                let metadata_result = crate::tests::native_io_fault_tests::metadata(&file);
                #[cfg(not(test))]
                let metadata_result = file.metadata();
                let metadata = metadata_result.map_err(|source| {
                    LocalFileError::from_io(
                        LocalFileOperation::Metadata,
                        Some(directory.diagnostic_path().to_path_buf()),
                        None,
                        source,
                    )
                })?;
                // SAFETY: `geteuid` is a side-effect-free process identity query.
                if metadata.mode() & 0o077 != 0 || metadata.uid() != unsafe { libc::geteuid() } {
                    return Err(LocalFileError::new(
                        LocalFileErrorKind::RequirementNotMet,
                        LocalFileOperation::Metadata,
                    )
                    .with_path(directory.diagnostic_path().to_path_buf())
                    .with_reason("stage mode or effective UID does not satisfy the required privacy policy"));
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
                )
                .map_err(|error| {
                    if error.path().is_none() {
                        error.with_path(directory.diagnostic_path().to_path_buf())
                    } else {
                        error
                    }
                })?;
                if access_observation.identity() != Some(expected) {
                    return Err(LocalFileError::new(
                        LocalFileErrorKind::ChangedDuringRead,
                        LocalFileOperation::Metadata,
                    )
                    .with_path(directory.diagnostic_path().to_path_buf())
                    .with_reason("opened stage identity changed during privacy verification"));
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
