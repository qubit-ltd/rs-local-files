// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
use std::path::Component;
use std::path::Path;
use std::path::PathBuf;
#[cfg(all(test, unix))]
use std::sync::Arc;
#[cfg(all(test, unix))]
use std::sync::Barrier;
#[cfg(all(test, unix))]
use std::sync::Mutex;
#[cfg(all(test, unix))]
use std::sync::OnceLock;
use std::time::Duration;
use std::time::Instant;

use crate::LocalCopyOptions;
use crate::LocalCopyResult;
use crate::LocalCreateDirectoryOptions;
use crate::LocalCreateDirectoryOutcome;
use crate::LocalDirectoryWalker;
use crate::LocalFileError;
use crate::LocalFileErrorKind;
use crate::LocalFileMetadata;
use crate::LocalFileOperation;
use crate::LocalFileSystem;
use crate::LocalFileWriter;
use crate::LocalListOptions;
use crate::LocalResult;
use crate::LocalSymlinkPolicy;
use crate::LocalWriteOptions;
use crate::directory::LocalComponentPolicy;
use crate::directory::binding::BindingAnchor;
use crate::directory::binding::DirectoryPathBinding;
use crate::directory::binding::PathBindingBudget;
use crate::local::LocalNamespace;
use crate::observation::LocalFileObservation;
use crate::observation::LocalObservedReader;
use crate::observation::LocalSecurityObservation;
use crate::observation::LocalSecurityObservationOptions;
use crate::options::LocalDirectoryOpenOptions;
use crate::options::LocalObservedReadOptions;
use crate::options::LocalPathBindingOptions;
use crate::rooted::Root;

#[cfg(all(test, unix))]
type ComponentPolicyCheckpoint = (PathBuf, PathBuf, PathBuf, Arc<Barrier>);
#[cfg(all(test, unix))]
static COMPONENT_POLICY_CHECKPOINT: OnceLock<Mutex<Option<ComponentPolicyCheckpoint>>> = OnceLock::new();

/// Cloneable fixed directory capability with independent operation policy.
#[derive(Clone, Debug)]
pub struct LocalDirectoryHandle {
    filesystem: LocalFileSystem,
    component_policy: LocalComponentPolicy,
    diagnostic_path: PathBuf,
    binding_observations: Option<Vec<LocalFileObservation>>,
    binding_anchors: Option<Vec<BindingAnchor>>,
    binding_deadline: Option<Duration>,
}

impl LocalDirectoryHandle {
    /// Queries security facts from this handle's retained directory authority.
    ///
    /// Each requested field is queried independently, so returned facts may
    /// describe different instants. The result is not an atomic snapshot;
    /// `Unknown` locality does not establish that the filesystem is local.
    /// Reparse-point handling remains the responsibility of the resolution
    /// policy used to obtain this handle.
    ///
    /// # Errors
    /// Returns `RequirementNotMet` when a required fact cannot be supplied,
    /// and propagates permission, malformed-data, identity-change, and I/O
    /// failures from the retained authority.
    pub fn security_observation(
        &self,
        options: &LocalSecurityObservationOptions,
    ) -> LocalResult<LocalSecurityObservation> {
        let authority = self.security_authority()?;
        crate::observation::security::query_security(&authority, options)
    }

    pub(super) const fn filesystem(&self) -> &LocalFileSystem {
        &self.filesystem
    }

    pub(super) fn diagnostic_path(&self) -> &Path {
        &self.diagnostic_path
    }

    pub(crate) fn security_authority(&self) -> LocalResult<std::fs::File> {
        self.root_authority()?.try_clone_authority().map_err(|source| {
            LocalFileError::from_io(LocalFileOperation::Metadata, Some(self.diagnostic_path.clone()), None, source)
        })
    }

    pub(crate) fn open(
        filesystem: &LocalFileSystem,
        path: &Path,
        options: &LocalDirectoryOpenOptions,
    ) -> LocalResult<Self> {
        let started_at = std::time::Instant::now();
        let mut resolution = filesystem.core.resolution.clone().tightened(
            options.resolution_limits().unwrap_or_default(),
        );
        if let Some(policy) = options.resolution_policy() {
            resolution = resolution.with_policy(policy.clone());
        }
        let policy = options.symlink_policy().unwrap_or(filesystem.symlink_policy());
        if options.symlink_policy() == Some(LocalSymlinkPolicy::FollowAcrossScope) {
            return Err(LocalFileError::new(
                LocalFileErrorKind::InvalidOptions,
                LocalFileOperation::OpenRoot,
            ));
        }
        for component in path.components() {
            if let Component::Normal(name) = component {
                options.component_policy().validate_component(name)?;
            }
        }
        let binding_enabled = options.path_binding().is_some();
        #[cfg(windows)]
        if binding_enabled && matches!(&filesystem.core.namespace, LocalNamespace::Host) {
            return Err(
                LocalFileError::new(LocalFileErrorKind::Unsupported, LocalFileOperation::OpenRoot)
                    .with_reason("Windows Host path binding requires drive-root ancestor traversal"),
            );
        }
        let mut path_budget = options
            .path_binding()
            .map(|limits| PathBindingBudget::new(limits.max_path_bytes()));
        let (rooted, binding_observations, binding_anchors, authority_path) = match &filesystem.core.namespace {
            LocalNamespace::Host => {
                #[cfg(unix)]
                {
                    let absolute = std::path::absolute(path).map_err(|source| {
                        LocalFileError::from_io(LocalFileOperation::OpenRoot, Some(path.to_path_buf()), None, source)
                    })?;
                    let host_root = Root::open(Path::new("/")).map_err(|source| {
                        LocalFileError::from_io(LocalFileOperation::OpenRoot, Some(path.to_path_buf()), None, source)
                    })?.with_resolution_contract(resolution.clone());
                    let relative = absolute.strip_prefix("/").unwrap_or(&absolute);
                    let policy = if policy == LocalSymlinkPolicy::FollowAcrossScope {
                        LocalSymlinkPolicy::FollowWithinScope
                    } else {
                        policy
                    };
                    let deadline = path_binding_deadline(started_at, options.path_binding());
                    validate_parent_steps(
                        &host_root,
                        relative,
                        policy,
                        path,
                        deadline,
                        options.component_policy(),
                        path_budget.as_mut(),
                    )?;
                    let resolved =
                            crate::rooted_local_file_system::resolve_rooted_path_allow_root_with_component_policy_and_budget(
                                &host_root,
                                relative,
                                policy,
                                true,
                                LocalFileOperation::OpenRoot,
                                options.component_policy(),
                                deadline,
                                path_budget.as_mut(),
                            )?;
                    for component in resolved.components() {
                        if let Component::Normal(name) = component {
                            options.component_policy().validate_component(name)?;
                        }
                    }
                    let diagnostic = Path::new("/").join(&resolved);
                    let (root, observations, anchors) = retain_directory_chain(
                        &host_root,
                        &resolved,
                        &diagnostic,
                        Some(&absolute),
                        binding_enabled,
                        options.path_binding(),
                        path_budget.as_mut(),
                        policy,
                        started_at,
                    )?;
                    (root, observations, anchors, diagnostic)
                }
                #[cfg(windows)]
                {
                    if !options.component_policy().is_allow_all() {
                        return Err(
                            LocalFileError::new(LocalFileErrorKind::Unsupported, LocalFileOperation::OpenRoot)
                                .with_reason("Windows Host directory opening cannot validate legacy component-policy target expansions"),
                        );
                    }
                    let opened = crate::rooted_local_file_system::RootedLocalFileSystem::open_with_contract(
                        path, resolution.clone(), policy,
                    )?;
                    let authority = opened.root_authority();
                    let root = Root::from_directory_authority_with_contract(
                        authority.try_clone_authority().map_err(|source| LocalFileError::from_io(
                            LocalFileOperation::OpenRoot, Some(path.to_path_buf()), None, source,
                        ))?,
                        authority.path().to_path_buf(),
                        resolution.clone(),
                    );
                    let diagnostic = root.path().to_path_buf();
                    let (root, observations, anchors) = retain_directory_chain(
                        &root,
                        Path::new(""),
                        &diagnostic,
                        Some(path),
                        binding_enabled,
                        options.path_binding(),
                        None,
                        policy,
                        started_at,
                    )?;
                    (root, observations, anchors, diagnostic)
                }
                #[cfg(not(any(unix, windows)))]
                {
                    if !options.component_policy().is_allow_all() {
                        return Err(
                            LocalFileError::new(LocalFileErrorKind::Unsupported, LocalFileOperation::OpenRoot)
                                .with_reason("Windows host directory opening cannot validate expanded reparse targets"),
                        );
                    }
                    let root = Root::open(path).map_err(|source| {
                        LocalFileError::from_io(LocalFileOperation::OpenRoot, Some(path.to_path_buf()), None, source)
                    })?;
                    let diagnostic = root.path().to_path_buf();
                    let (root, observations, anchors) = retain_directory_chain(
                        &root,
                        Path::new(""),
                        &diagnostic,
                        Some(root.path()),
                        binding_enabled,
                        options.path_binding(),
                        None,
                        policy,
                        started_at,
                    )?;
                    (root, observations, anchors, diagnostic)
                }
            }
            LocalNamespace::Rooted(rooted) => {
                let source_root = rooted.root_authority();
                let authority = source_root.try_clone_authority().map_err(|source| LocalFileError::from_io(
                    LocalFileOperation::OpenRoot, Some(path.to_path_buf()), None, source,
                ))?;
                let working_root = Root::from_directory_authority_with_contract(
                    authority, source_root.path().to_path_buf(), resolution.clone(),
                );
                let virtual_path = if path.is_absolute() {
                    path.to_path_buf()
                } else {
                    filesystem.current_directory()?.join(path)
                };
                let authority_path = virtual_path
                    .strip_prefix(Path::new(std::path::MAIN_SEPARATOR_STR))
                    .unwrap_or(&virtual_path);
                let deadline = path_binding_deadline(started_at, options.path_binding());
                validate_parent_steps(
                    &working_root,
                    authority_path,
                    policy,
                    path,
                    deadline,
                    options.component_policy(),
                    path_budget.as_mut(),
                )?;
                let resolved =
                    crate::rooted_local_file_system::resolve_rooted_path_allow_root_with_component_policy_and_budget(
                        &working_root,
                        authority_path,
                        policy,
                        true,
                        LocalFileOperation::OpenRoot,
                        options.component_policy(),
                        deadline,
                        path_budget.as_mut(),
                    )?;
                for component in resolved.components() {
                    if let Component::Normal(name) = component {
                        options.component_policy().validate_component(name)?;
                    }
                }
                let diagnostic = rooted.diagnostic_path().join(&resolved);
                let opening_recipe = rooted.diagnostic_path().join(authority_path);
                let (root, observations, anchors) = retain_directory_chain(
                    &working_root,
                    &resolved,
                    &diagnostic,
                    Some(&opening_recipe),
                    binding_enabled,
                    options.path_binding(),
                    path_budget.as_mut(),
                    policy,
                    started_at,
                )?;
                (root, observations, anchors, diagnostic)
            }
        };
        let fs = LocalFileSystem::from_root_authority(
            rooted,
            if policy == LocalSymlinkPolicy::FollowAcrossScope {
                LocalSymlinkPolicy::FollowWithinScope
            } else {
                policy
            },
        )?;
        let binding_deadline = options.path_binding().and_then(|limits| limits.deadline());
        Ok(Self {
            filesystem: fs,
            component_policy: options.component_policy().clone(),
            diagnostic_path: authority_path,
            binding_observations,
            binding_anchors,
            binding_deadline,
        })
    }

    fn validate(&self, path: &Path, follow_final: bool) -> LocalResult<PathBuf> {
        self.validate_with_policy(path, follow_final, self.filesystem.symlink_policy())
    }

    fn validate_with_policy(
        &self,
        path: &Path,
        follow_final: bool,
        symlink_policy: LocalSymlinkPolicy,
    ) -> LocalResult<PathBuf> {
        if path.is_absolute() || path.has_root() {
            return Err(
                LocalFileError::new(LocalFileErrorKind::InvalidPath, LocalFileOperation::BindPath)
                    .with_path(path.to_path_buf()),
            );
        }
        let mut normalized = PathBuf::new();
        for component in path.components() {
            match component {
                Component::CurDir => continue,
                Component::ParentDir => {
                    if !normalized.as_os_str().is_empty() {
                        match &self.filesystem.core.namespace {
                            LocalNamespace::Rooted(rooted) => rooted.validate_directory(&normalized, symlink_policy)?,
                            LocalNamespace::Host => {
                                return Err(LocalFileError::new(
                                    LocalFileErrorKind::InvalidState,
                                    LocalFileOperation::BindPath,
                                ));
                            }
                        }
                    }
                    if !normalized.pop() {
                        return Err(
                            LocalFileError::new(LocalFileErrorKind::InvalidPath, LocalFileOperation::BindPath)
                                .with_path(path.to_path_buf()),
                        );
                    }
                }
                Component::Normal(name) => {
                    self.component_policy.validate_component(name)?;
                    normalized.push(name);
                }
                Component::RootDir | Component::Prefix(_) => {
                    return Err(
                        LocalFileError::new(LocalFileErrorKind::InvalidPath, LocalFileOperation::BindPath)
                            .with_path(path.to_path_buf()),
                    );
                }
            }
        }
        let root = self.root_authority()?;
        let started_at = Instant::now();
        let deadline = self.binding_deadline.and_then(|limit| started_at.checked_add(limit));
        let resolved =
            crate::rooted_local_file_system::resolve_rooted_path_allow_root_with_component_policy_and_budget(
                root,
                path,
                symlink_policy,
                follow_final,
                LocalFileOperation::BindPath,
                &self.component_policy,
                deadline,
                None,
            )?;
        for component in resolved.components() {
            if let Component::Normal(name) = component {
                self.component_policy.validate_component(name)?;
            }
        }
        Ok(resolved)
    }

    /// Opens a child directory and returns a narrower fixed authority.
    ///
    /// Absolute paths are rejected; parent components are resolved inside
    /// this authority and cannot escape its root. The returned handle retains
    /// this handle's denied component names and adds any names denied by the
    /// supplied options.
    /// Rooted paths and platform prefixes are also rejected with `InvalidPath`.
    ///
    /// # Errors
    /// Returns path, policy, type, resource-limit, or native I/O errors.
    pub fn open_directory(&self, relative: &Path, options: &LocalDirectoryOpenOptions) -> LocalResult<Self> {
        if relative.is_absolute()
            || relative.has_root()
            || relative
                .components()
                .any(|component| matches!(component, Component::Prefix(_)))
        {
            return Err(
                LocalFileError::new(LocalFileErrorKind::InvalidPath, LocalFileOperation::BindPath)
                    .with_path(relative.to_path_buf()),
            );
        }
        let raw_relative = relative;
        let component_policy = self.component_policy.combined_with(options.component_policy());
        let mut resolution = self.filesystem.core.resolution.clone().tightened(
            options.resolution_limits().unwrap_or_default(),
        );
        if let Some(policy) = options.resolution_policy() {
            resolution = resolution.with_policy(policy.clone());
        }
        let parent_root = self.root_authority()?;
        let working_root = Root::from_directory_authority_with_contract(
            parent_root.try_clone_authority().map_err(|source| LocalFileError::from_io(
                LocalFileOperation::OpenRoot, Some(relative.to_path_buf()), None, source,
            ))?,
            parent_root.path().to_path_buf(),
            resolution,
        );
        let policy = options.symlink_policy().unwrap_or(self.filesystem.symlink_policy());
        let started_at = Instant::now();
        let deadline = options
            .path_binding()
            .and_then(|limits| limits.deadline())
            .and_then(|limit| started_at.checked_add(limit));
        let mut path_budget = options
            .path_binding()
            .map(|limits| PathBindingBudget::new(limits.max_path_bytes()));
        let relative =
            crate::rooted_local_file_system::resolve_rooted_path_allow_root_with_component_policy_and_budget(
                &working_root,
                raw_relative,
                policy,
                true,
                LocalFileOperation::OpenRoot,
                &component_policy,
                deadline,
                path_budget.as_mut(),
            )?;
        let diagnostic = self.diagnostic_path.join(&relative);
        for component in relative.components() {
            if let Component::Normal(name) = component {
                component_policy.validate_component(name)?;
            }
        }
        if options.symlink_policy() == Some(LocalSymlinkPolicy::FollowAcrossScope) {
            return Err(LocalFileError::new(
                LocalFileErrorKind::InvalidOptions,
                LocalFileOperation::OpenRoot,
            ));
        }
        let (root, child_observations, child_anchors) = retain_directory_chain(
            &working_root,
            &relative,
            &diagnostic,
            Some(&self.diagnostic_path.join(raw_relative)),
            options.path_binding().is_some(),
            options.path_binding(),
            path_budget.as_mut(),
            policy,
            started_at,
        )?;
        let filesystem = LocalFileSystem::from_root_authority(
            root,
            if policy == LocalSymlinkPolicy::FollowAcrossScope {
                LocalSymlinkPolicy::FollowWithinScope
            } else {
                policy
            },
        )?;
        let mut binding_anchors = None;
        let binding_deadline = options.path_binding().and_then(|limits| limits.deadline());
        let binding_observations =
            if let (Some(limits), Some(mut child_observations)) = (options.path_binding(), child_observations) {
                let (mut observations, mut anchors) =
                    if let (Some(observations), Some(anchors)) = (&self.binding_observations, &self.binding_anchors) {
                        (observations.clone(), anchors.clone())
                    } else {
                        let observation = self.observe()?;
                        let identity = observation.identity().cloned().ok_or_else(|| {
                            LocalFileError::new(LocalFileErrorKind::Unsupported, LocalFileOperation::Metadata)
                        })?;
                        let mut observations = Vec::new();
                        observations.try_reserve_exact(1).map_err(|_| {
                            LocalFileError::new(LocalFileErrorKind::ResourceLimit, LocalFileOperation::OpenRoot)
                        })?;
                        observations.push(observation);
                        let mut anchors = Vec::new();
                        anchors.try_reserve_exact(1).map_err(|_| {
                            LocalFileError::new(LocalFileErrorKind::ResourceLimit, LocalFileOperation::OpenRoot)
                        })?;
                        let root = self.root_authority()?;
                        anchors.push(binding_anchor(root, root.path().to_path_buf(), identity, self.filesystem.symlink_policy())?);
                        (observations, anchors)
                    };
                let additional = child_observations.len().saturating_sub(1);
                let ancestor_count = observations.len().saturating_sub(1).saturating_add(additional);
                if limits.max_ancestors().is_some_and(|maximum| ancestor_count > maximum) {
                    return Err(
                        LocalFileError::new(LocalFileErrorKind::ResourceLimit, LocalFileOperation::OpenRoot)
                            .with_path(diagnostic),
                    );
                }
                child_observations.drain(..1);
                observations.try_reserve(additional).map_err(|_| {
                    LocalFileError::new(LocalFileErrorKind::ResourceLimit, LocalFileOperation::OpenRoot)
                })?;
                observations.extend(child_observations);
                if let Some(mut child_anchors) = child_anchors {
                    child_anchors.drain(..1);
                    anchors.try_reserve(child_anchors.len()).map_err(|_| {
                        LocalFileError::new(LocalFileErrorKind::ResourceLimit, LocalFileOperation::OpenRoot)
                    })?;
                    anchors.extend(child_anchors);
                }
                binding_anchors = Some(anchors);
                Some(observations)
            } else {
                None
            };
        Ok(Self {
            filesystem,
            component_policy,
            diagnostic_path: diagnostic,
            binding_observations,
            binding_anchors,
            binding_deadline,
        })
    }

    pub(super) fn checked(&self, relative: &Path, follow_final: bool) -> LocalResult<PathBuf> {
        self.validate(relative, follow_final)
    }

    /// Reads final-entry metadata beneath this authority.
    ///
    /// Returns `None` only when the entry is safely absent.
    ///
    /// # Errors
    /// Returns path, policy, permission, or native query errors.
    pub fn entry_metadata(&self, relative: &Path) -> LocalResult<Option<LocalFileMetadata>> {
        let path = self.checked(relative, false)?;
        match self.filesystem.metadata(&path) {
            Ok(metadata) => Ok(Some(metadata)),
            Err(error) if error.kind() == LocalFileErrorKind::NotFound => Ok(None),
            Err(error) => Err(error),
        }
    }

    /// Observes the fixed directory through its retained native handle.
    ///
    /// # Errors
    /// Returns native handle or observation-query errors.
    pub fn observe(&self) -> LocalResult<LocalFileObservation> {
        let file = self.root_authority()?.try_clone_authority().map_err(|source| {
            LocalFileError::from_io(
                LocalFileOperation::Metadata,
                Some(self.diagnostic_path.clone()),
                None,
                source,
            )
        })?;
        crate::observation::query::query_observation(&file, crate::observation::LocalObservationRequirement::Optional)
    }

    /// Observes one entry relative to this authority without following its
    /// final link.
    ///
    /// The returned identity, when available, can be compared with a later
    /// observation through this same fixed directory authority. That
    /// comparison identifies the native object; it does not guarantee
    /// unchanged contents.
    pub fn observe_entry(&self, relative: &Path) -> LocalResult<LocalFileObservation> {
        self.observe_entry_with_link_count(relative)
            .map(|(observation, _)| observation)
    }

    pub(crate) fn observe_entry_with_link_count(
        &self,
        relative: &Path,
    ) -> LocalResult<(LocalFileObservation, Option<u64>)> {
        let relative = self.checked(relative, false)?;
        let mut components = relative
            .components()
            .filter_map(|component| match component {
                Component::Normal(name) => Some(name),
                _ => None,
            })
            .peekable();
        let mut cursor = self.root_authority()?.resolution_cursor().map_err(|source| {
            LocalFileError::from_io(
                LocalFileOperation::Metadata,
                Some(self.diagnostic_path.clone()),
                None,
                source,
            )
        })?;
        let mut final_name = None;
        while let Some(name) = components.next() {
            if components.peek().is_some() {
                cursor.descend(name).map_err(|source| {
                    LocalFileError::from_io(
                        LocalFileOperation::Metadata,
                        Some(self.diagnostic_path.join(&relative)),
                        None,
                        source,
                    )
                })?;
            } else {
                final_name = Some(name);
            }
        }
        let name = final_name
            .ok_or_else(|| LocalFileError::new(LocalFileErrorKind::InvalidPath, LocalFileOperation::Metadata))?;
        cursor
            .observe_entry_with_link_count(name)
            .map_err(|error| error.with_path(self.diagnostic_path.join(relative)))
    }

    /// Returns observations retained when this handle's path binding opened.
    ///
    /// # Errors
    /// Returns `InvalidState` if binding was not enabled, or `ResourceLimit`
    /// when the supplied collection limits are exceeded.
    pub fn ancestor_observations(&self, limits: &LocalListOptions) -> LocalResult<Vec<LocalFileObservation>> {
        let started_at = std::time::Instant::now();
        let observations = self
            .binding_observations
            .as_ref()
            .ok_or_else(|| LocalFileError::new(LocalFileErrorKind::InvalidState, LocalFileOperation::Metadata))?;
        if limits.max_depth().is_some_and(|limit| observations.len() > limit)
            || limits.max_entries().is_some_and(|limit| observations.len() > limit)
            || limits
                .max_seen_name_bytes()
                .is_some_and(|limit| self.diagnostic_path.as_os_str().len() > limit)
            || limits
                .max_open_directories()
                .is_some_and(|limit| observations.len() > limit)
            || limits.deadline().is_some_and(|limit| limit.is_zero())
        {
            return Err(LocalFileError::new(
                LocalFileErrorKind::ResourceLimit,
                LocalFileOperation::Metadata,
            ));
        }
        let mut result = Vec::new();
        result
            .try_reserve_exact(observations.len())
            .map_err(|_| LocalFileError::new(LocalFileErrorKind::ResourceLimit, LocalFileOperation::Metadata))?;
        result.extend(observations.iter().cloned());
        if limits.deadline().is_some_and(|limit| started_at.elapsed() >= limit) {
            return Err(LocalFileError::from_io(
                LocalFileOperation::Metadata,
                Some(self.diagnostic_path.clone()),
                None,
                std::io::Error::new(std::io::ErrorKind::TimedOut, "ancestor observation deadline exceeded"),
            ));
        }
        Ok(result)
    }

    /// Opens an observed reader beneath this directory authority.
    ///
    /// Reader options and optional path-binding checks follow the same
    /// retained authority as other handle operations.
    ///
    /// # Errors
    /// Returns path, policy, type, observation, resource-limit, or I/O errors.
    pub fn open_observed_reader(
        &self,
        relative: &Path,
        options: &LocalObservedReadOptions,
    ) -> LocalResult<LocalObservedReader> {
        let read_options = options.read_options().unwrap_or(self.filesystem.default_read_options());
        let symlink_policy = self.filesystem.read_symlink_policy(read_options, None);
        crate::local_file_system_validation::validate_scope_symlink_policy(
            self.filesystem.scope(),
            symlink_policy,
            LocalFileOperation::OpenReader,
            Some(relative),
        )?;
        let checked = self.validate_with_policy(relative, true, symlink_policy)?;
        let mut reader = self.filesystem.open_observed_reader(&checked, options)?;
        if let Some(anchors) = &self.binding_anchors {
            reader = reader.with_binding(Box::new(DirectoryPathBinding::new(
                anchors.clone(),
                self.binding_deadline,
            )));
        }
        Ok(reader)
    }

    /// Starts a directory listing for a relative path under this authority.
    ///
    /// The component policy validates the supplied path during path
    /// resolution. Names discovered by recursive traversal are not filtered
    /// by that policy.
    ///
    /// # Errors
    /// Returns path, policy, option, type, or native opening errors.
    pub fn list_with_options(&self, relative: &Path, options: &LocalListOptions) -> LocalResult<LocalDirectoryWalker> {
        let policy = options.symlink_policy().unwrap_or(self.filesystem.symlink_policy());
        self.filesystem
            .list_with_options(&self.validate_with_policy(relative, true, policy)?, options)
    }

    /// Opens a writer under this authority using the supplied write options.
    ///
    /// # Errors
    /// Returns path, policy, option, requirement, or native opening errors.
    pub fn open_writer_with_options(
        &self,
        relative: &Path,
        options: &LocalWriteOptions,
    ) -> LocalResult<LocalFileWriter> {
        self.filesystem
            .open_writer_with_options(&self.checked(relative, true)?, options)
    }

    /// Creates a directory beneath this authority.
    ///
    /// # Errors
    /// Returns path, policy, conflict, requirement, or native I/O errors.
    pub fn create_directory_with_options(
        &self,
        relative: &Path,
        options: &LocalCreateDirectoryOptions,
    ) -> LocalResult<LocalCreateDirectoryOutcome> {
        self.filesystem
            .create_directory_with_options(&self.checked(relative, true)?, options)
    }

    /// Copies between two relative paths beneath this authority.
    ///
    /// The component policy validates the source and destination paths during
    /// path resolution. Names discovered while copying a tree are not filtered
    /// by that policy.
    ///
    /// Invalid operands produce an unchanged typed copy failure.
    pub fn copy_with_options(&self, source: &Path, destination: &Path, options: &LocalCopyOptions) -> LocalCopyResult {
        let policy = options
            .symlink_policy_override()
            .unwrap_or(self.filesystem.symlink_policy());
        match (
            self.validate_with_policy(source, false, policy),
            self.validate_with_policy(destination, false, policy),
        ) {
            (Ok(resolved_source), Ok(resolved_destination)) => {
                #[cfg(all(test, unix))]
                wait_at_component_policy_checkpoint(&self.diagnostic_path, source, destination);
                self.filesystem
                    .copy_with_options(&resolved_source, &resolved_destination, options)
            }
            (Err(error), _) | (_, Err(error)) => Err(crate::outcome::LocalCopyFailure::new(
                error,
                crate::outcome::LocalCopyFailureState::Unchanged,
                crate::outcome::LocalCopyStats::default(),
                None,
                None,
            )),
        }
    }

    pub(super) fn root_authority(&self) -> LocalResult<&Root> {
        match &self.filesystem.core.namespace {
            LocalNamespace::Rooted(rooted) => Ok(rooted.root_authority()),
            LocalNamespace::Host => Err(LocalFileError::new(
                LocalFileErrorKind::InvalidState,
                LocalFileOperation::OpenRoot,
            )),
        }
    }
}

#[cfg(all(test, unix))]
pub(crate) fn install_component_policy_checkpoint(
    handle: &LocalDirectoryHandle,
    source: &Path,
    destination: &Path,
    barrier: Arc<Barrier>,
) {
    let checkpoint = COMPONENT_POLICY_CHECKPOINT.get_or_init(|| Mutex::new(None));
    *checkpoint.lock().expect("component policy checkpoint lock") = Some((
        handle.diagnostic_path.clone(),
        source.to_path_buf(),
        destination.to_path_buf(),
        barrier,
    ));
}

#[cfg(all(test, unix))]
fn wait_at_component_policy_checkpoint(root: &Path, source: &Path, destination: &Path) {
    let checkpoint = COMPONENT_POLICY_CHECKPOINT.get_or_init(|| Mutex::new(None));
    let barrier = {
        let mut checkpoint = checkpoint.lock().expect("component policy checkpoint lock");
        match checkpoint.as_ref() {
            Some((expected_root, expected_source, expected_destination, _))
                if expected_root == root && expected_source == source && expected_destination == destination =>
            {
                checkpoint.take().map(|(_, _, _, barrier)| barrier)
            }
            _ => None,
        }
    };
    if let Some(barrier) = barrier {
        barrier.wait();
        barrier.wait();
    }
}

#[allow(clippy::type_complexity, clippy::too_many_arguments)]
pub(crate) fn retain_directory_chain(
    root: &Root,
    relative: &Path,
    diagnostic_path: &Path,
    opening_recipe: Option<&Path>,
    retain_binding: bool,
    limits: Option<&LocalPathBindingOptions>,
    mut path_budget: Option<&mut PathBindingBudget>,
    symlink_policy: LocalSymlinkPolicy,
    started_at: std::time::Instant,
) -> LocalResult<(Root, Option<Vec<LocalFileObservation>>, Option<Vec<BindingAnchor>>)> {
    let depth = relative.components().count();
    if limits.is_some_and(|limits| {
        limits.max_ancestors().is_some_and(|maximum| depth > maximum)
            || limits.deadline().is_some_and(|maximum| started_at.elapsed() >= maximum)
    }) {
        return Err(
            LocalFileError::new(LocalFileErrorKind::ResourceLimit, LocalFileOperation::OpenRoot)
                .with_path(diagnostic_path.to_path_buf()),
        );
    }
    let mut observations = Vec::new();
    let mut anchors = Vec::new();
    if retain_binding {
        check_binding_deadline(limits, started_at, diagnostic_path)?;
        if let Some(budget) = path_budget.as_deref_mut() {
            budget.debit(
                root.path().as_os_str().len(),
                LocalFileOperation::OpenRoot,
                diagnostic_path,
            )?;
        }
        observations
            .try_reserve_exact(depth.saturating_add(1))
            .map_err(|_| LocalFileError::new(LocalFileErrorKind::ResourceLimit, LocalFileOperation::OpenRoot))?;
        anchors
            .try_reserve_exact(depth.saturating_add(1))
            .map_err(|_| LocalFileError::new(LocalFileErrorKind::ResourceLimit, LocalFileOperation::OpenRoot))?;
        let root_file = root.try_clone_authority().map_err(|source| {
            LocalFileError::from_io(
                LocalFileOperation::Metadata,
                Some(diagnostic_path.to_path_buf()),
                None,
                source,
            )
        })?;
        let observation = crate::observation::query::query_observation(
            &root_file,
            crate::observation::LocalObservationRequirement::Optional,
        )?;
        check_binding_deadline(limits, started_at, diagnostic_path)?;
        let identity = observation
            .identity()
            .cloned()
            .ok_or_else(|| LocalFileError::new(LocalFileErrorKind::Unsupported, LocalFileOperation::Metadata))?;
        anchors.push(binding_anchor(root, root.path().to_path_buf(), identity, symlink_policy)?);
        observations.push(observation);
    }
    let mut cursor = root.resolution_cursor().map_err(|source| {
        LocalFileError::from_io(
            LocalFileOperation::OpenRoot,
            Some(diagnostic_path.to_path_buf()),
            None,
            source,
        )
    })?;
    let mut prefix = PathBuf::new();
    for component in relative.components() {
        check_binding_deadline(limits, started_at, diagnostic_path)?;
        let Component::Normal(name) = component else {
            continue;
        };
        let metadata = cursor.metadata(name).map_err(|source| {
            LocalFileError::from_io(
                LocalFileOperation::Metadata,
                Some(diagnostic_path.to_path_buf()),
                None,
                source,
            )
        })?;
        check_binding_deadline(limits, started_at, diagnostic_path)?;
        if metadata.kind() != crate::rooted::EntryKind::Directory {
            return Err(
                LocalFileError::new(LocalFileErrorKind::NotDirectory, LocalFileOperation::OpenRoot)
                    .with_path(diagnostic_path.to_path_buf()),
            );
        }
        cursor.descend(name).map_err(|source| {
            LocalFileError::from_io(
                LocalFileOperation::OpenRoot,
                Some(diagnostic_path.to_path_buf()),
                None,
                source,
            )
        })?;
        check_binding_deadline(limits, started_at, diagnostic_path)?;
        prefix.push(name);
        if retain_binding {
            let authority_path = root.path().join(&prefix);
            if let Some(budget) = path_budget.as_deref_mut() {
                budget.debit(
                    authority_path.as_os_str().len(),
                    LocalFileOperation::OpenRoot,
                    diagnostic_path,
                )?;
            }
            let file = cursor.try_clone_current().map_err(|source| {
                LocalFileError::from_io(
                    LocalFileOperation::Metadata,
                    Some(diagnostic_path.to_path_buf()),
                    None,
                    source,
                )
            })?;
            let observation = crate::observation::query::query_observation(
                &file,
                crate::observation::LocalObservationRequirement::Optional,
            )?;
            check_binding_deadline(limits, started_at, diagnostic_path)?;
            let identity = observation
                .identity()
                .cloned()
                .ok_or_else(|| LocalFileError::new(LocalFileErrorKind::Unsupported, LocalFileOperation::Metadata))?;
            let authority = Root::from_directory_authority_with_contract(
                file, authority_path.clone(), root.resolution_contract().clone(),
            );
            anchors.push(BindingAnchor {
                path: authority_path.clone(),
                opening_recipe: authority_path,
                identity,
                authority: std::sync::Arc::new(authority),
                contract: root.resolution_contract().clone(),
                symlink_policy,
            });
            observations.push(observation);
        }
    }
    if retain_binding && let (Some(recipe), Some(anchor)) = (opening_recipe, anchors.last_mut()) {
        if let Some(budget) = path_budget {
            budget.debit(recipe.as_os_str().len(), LocalFileOperation::OpenRoot, diagnostic_path)?;
        }
        anchor.opening_recipe = recipe.to_path_buf();
    }
    check_binding_deadline(limits, started_at, diagnostic_path)?;
    let root = Root::from_directory_authority_with_contract(
        cursor.into_file(), diagnostic_path.to_path_buf(), root.resolution_contract().clone(),
    );
    Ok((
        root,
        retain_binding.then_some(observations),
        retain_binding.then_some(anchors),
    ))
}

fn validate_parent_steps(
    root: &Root,
    path: &Path,
    policy: LocalSymlinkPolicy,
    original: &Path,
    deadline: Option<Instant>,
    component_policy: &LocalComponentPolicy,
    mut path_budget: Option<&mut PathBindingBudget>,
) -> LocalResult<()> {
    let mut prefix = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Normal(name) => prefix.push(name),
            Component::ParentDir => {
                if !prefix.as_os_str().is_empty() {
                    crate::rooted_local_file_system::resolve_rooted_path_allow_root_with_component_policy_and_budget(
                        root,
                        &prefix,
                        policy,
                        true,
                        LocalFileOperation::OpenRoot,
                        component_policy,
                        deadline,
                        path_budget.as_deref_mut(),
                    )
                    .map_err(|error| error.with_path(original.to_path_buf()))?;
                }
                if !prefix.pop() {
                    return Err(
                        LocalFileError::new(LocalFileErrorKind::InvalidPath, LocalFileOperation::OpenRoot)
                            .with_path(original.to_path_buf()),
                    );
                }
            }
            Component::RootDir | Component::CurDir => {}
            Component::Prefix(_) => {
                return Err(
                    LocalFileError::new(LocalFileErrorKind::InvalidPath, LocalFileOperation::OpenRoot)
                        .with_path(original.to_path_buf()),
                );
            }
        }
    }
    Ok(())
}

fn path_binding_deadline(started_at: Instant, limits: Option<&LocalPathBindingOptions>) -> Option<Instant> {
    limits
        .and_then(|limits| limits.deadline())
        .and_then(|duration| started_at.checked_add(duration))
}

fn check_binding_deadline(
    limits: Option<&LocalPathBindingOptions>,
    started_at: Instant,
    path: &Path,
) -> LocalResult<()> {
    if limits.is_some_and(|limits| {
        limits
            .deadline()
            .is_some_and(|deadline| started_at.elapsed() >= deadline)
    }) {
        return Err(LocalFileError::from_io(
            LocalFileOperation::OpenRoot,
            Some(path.to_path_buf()),
            None,
            std::io::Error::new(std::io::ErrorKind::TimedOut, "path binding deadline exceeded"),
        ));
    }
    Ok(())
}

fn binding_anchor(
    root: &Root,
    path: PathBuf,
    identity: crate::observation::LocalFileIdentity,
    symlink_policy: LocalSymlinkPolicy,
) -> LocalResult<BindingAnchor> {
    let authority = Root::from_directory_authority_with_contract(
        root.try_clone_authority().map_err(|source| {
            LocalFileError::from_io(LocalFileOperation::OpenRoot, Some(path.clone()), None, source)
        })?,
        path.clone(),
        root.resolution_contract().clone(),
    );
    Ok(BindingAnchor {
        opening_recipe: path.clone(),
        path,
        identity,
        authority: std::sync::Arc::new(authority),
        contract: root.resolution_contract().clone(),
        symlink_policy,
    })
}
