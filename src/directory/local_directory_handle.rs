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
use crate::directory::binding::PathBindingBudget;
use crate::local::LocalNamespace;
use crate::observation::LocalFileObservation;
use crate::observation::LocalObservedReader;
use crate::observation::LocalSecurityObservation;
use crate::observation::LocalSecurityObservationOptions;
use crate::options::LocalDirectoryOpenOptions;
use crate::options::LocalObservedReadOptions;
use crate::options::LocalPathBindingOptions;
use crate::options::LocalWriteMode;
use crate::resolution::internal::checked_path_binding_deadline;
use crate::rooted::Root;
use crate::rooted_local_file_system::RootParentBehavior;

#[path = "local_directory_handle/observed.rs"]
mod observed;
// Implements conditional-publication capability and writer methods.
#[path = "local_directory_handle/conditional.rs"]
mod conditional;
// Implements staged-directory lifecycle and recovery methods.
#[path = "local_directory_handle/staged.rs"]
mod staged;

#[cfg(all(test, unix))]
type ComponentPolicyCheckpoint = (PathBuf, PathBuf, PathBuf, Arc<Barrier>);
#[cfg(all(test, unix))]
static COMPONENT_POLICY_CHECKPOINT: OnceLock<Mutex<Option<ComponentPolicyCheckpoint>>> = OnceLock::new();

/// Cloneable fixed directory capability with independent operation policy.
///
/// Operations are resolved relative to the retained directory authority, so a
/// later rename of its diagnostic path does not redirect the handle. Relative
/// paths and component policies constrain resolution; they do not lock the
/// namespace or filter names merely discovered during recursive traversal.
///
/// # Examples
///
/// ```
/// use std::path::Path;
///
/// use qubit_local_files::LocalFileSystem;
/// use qubit_local_files::options::LocalDirectoryOpenOptions;
///
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let fixture = tempfile::tempdir()?;
/// std::fs::write(fixture.path().join("source"), b"data")?;
/// let filesystem = LocalFileSystem::rooted(fixture.path())?;
/// let directory = filesystem.open_directory_with_options(
///     Path::new("/"),
///     &LocalDirectoryOpenOptions::new(),
/// )?;
/// let observation = directory.observe_entry(Path::new("source"))?;
/// assert_eq!(observation.metadata().len(), 4);
/// // Native identity is optional and identifies the object, not its contents.
/// let _identity = observation.identity();
/// # Ok(())
/// # }
/// ```
#[derive(Clone)]
pub struct LocalDirectoryHandle {
    /// Filesystem namespace and authority used for operations.
    filesystem: LocalFileSystem,
    /// Additional deny-name rules applied during path resolution.
    component_policy: LocalComponentPolicy,
    /// Best-effort diagnostic location; it does not grant access.
    diagnostic_path: PathBuf,
    /// Initial observations retained for optional opening-path verification.
    binding_observations: Option<Vec<LocalFileObservation>>,
    /// Retained identities and authorities used for optional path binding.
    binding_anchors: Option<Vec<BindingAnchor>>,
    /// Cooperative time budget for binding work, when configured.
    binding_deadline: Option<Duration>,
}

impl std::fmt::Debug for LocalDirectoryHandle {
    /// Formats handle configuration while redacting its diagnostic path.
    ///
    /// Formatting does not access the filesystem or expose the retained native
    /// authority.
    ///
    /// # Parameters
    /// - `formatter`: Destination buffer receiving the redacted configuration.
    ///
    /// # Returns
    /// `Ok(())` after the configuration has been written to the formatter.
    ///
    /// # Errors
    /// Returns the formatter's error if writing the debug representation fails.
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("LocalDirectoryHandle")
            .field("filesystem", &self.filesystem)
            .field("component_policy", &self.component_policy)
            .field(
                "diagnostic_path",
                &crate::local_file_system::redacted_path(&self.diagnostic_path),
            )
            .field("binding_observations", &self.binding_observations)
            .field("binding_enabled", &self.binding_anchors.is_some())
            .field("binding_deadline", &self.binding_deadline)
            .finish()
    }
}

impl LocalDirectoryHandle {
    /// Opens a child directory and returns a narrower fixed authority.
    ///
    /// Absolute paths are rejected; parent components are resolved inside
    /// this authority and cannot escape its root. The returned handle retains
    /// this handle's denied component names and adds any names denied by the
    /// supplied options.
    /// Rooted paths and platform prefixes are also rejected with `InvalidPath`.
    ///
    /// # Parameters
    /// - `relative`: Child path to resolve beneath this handle.
    /// - `options`: Resolution, symlink, and binding policies for the child.
    ///
    /// # Returns
    /// A handle retaining authority to the resolved child directory.
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
        let mut resolution = self
            .filesystem
            .core
            .resolution
            .clone()
            .tightened(options.resolution_limits().unwrap_or_default());
        if let Some(limits) = options.path_binding().and_then(|binding| binding.resolution_limits()) {
            resolution = resolution.tightened(limits);
        }
        if let Some(policy) = options.resolution_policy() {
            resolution = resolution.with_policy(policy.clone());
        }
        let parent_root = self.root_authority()?;
        let working_root = Root::from_directory_authority_with_contract(
            parent_root.try_clone_authority().map_err(|source| {
                LocalFileError::from_io(LocalFileOperation::OpenRoot, Some(relative.to_path_buf()), None, source)
            })?,
            parent_root.path().to_path_buf(),
            resolution,
        );
        let policy = options.symlink_policy().unwrap_or(self.filesystem.symlink_policy());
        let started_at = Instant::now();
        let deadline = checked_path_binding_deadline(
            started_at,
            options.path_binding().and_then(|limits| limits.deadline()),
            LocalFileOperation::OpenRoot,
            Some(raw_relative),
        )?;
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
        let binding_observations = if let (Some(limits), Some(mut child_observations)) =
            (options.path_binding(), child_observations)
        {
            let (mut observations, mut anchors) =
                if let (Some(observations), Some(anchors)) = (&self.binding_observations, &self.binding_anchors) {
                    if let Some(budget) = path_budget.as_mut() {
                        for anchor in anchors {
                            budget.debit(anchor.path.as_os_str().len(), LocalFileOperation::OpenRoot, &diagnostic)?;
                            budget.debit(
                                anchor.opening_recipe.as_os_str().len(),
                                LocalFileOperation::OpenRoot,
                                &diagnostic,
                            )?;
                        }
                    }
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
                    if let Some(budget) = path_budget.as_mut() {
                        let path_bytes = root.path().as_os_str().len();
                        budget.debit(path_bytes, LocalFileOperation::OpenRoot, &diagnostic)?;
                        budget.debit(path_bytes, LocalFileOperation::OpenRoot, &diagnostic)?;
                    }
                    anchors.push(binding_anchor(
                        root,
                        root.path().to_path_buf(),
                        identity,
                        self.filesystem.symlink_policy(),
                    )?);
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
            observations
                .try_reserve(additional)
                .map_err(|_| LocalFileError::new(LocalFileErrorKind::ResourceLimit, LocalFileOperation::OpenRoot))?;
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

    /// Opens a fixed directory capability using the supplied resolution policy.
    ///
    /// The resulting handle retains a native directory authority and optional
    /// path-binding observations; later path renames do not redirect it.
    ///
    /// # Parameters
    /// - `filesystem`: Filesystem namespace and authority from which to open.
    /// - `path`: Directory path to resolve under the filesystem's scope.
    /// - `options`: Resolution, symlink, and optional binding policies.
    ///
    /// # Returns
    /// A handle retaining authority to the resolved directory.
    ///
    /// # Errors
    /// Returns path, policy, capability, resource-limit, or native I/O errors.
    pub(crate) fn open(
        filesystem: &LocalFileSystem,
        path: &Path,
        options: &LocalDirectoryOpenOptions,
    ) -> LocalResult<Self> {
        let started_at = std::time::Instant::now();
        let mut resolution = filesystem
            .core
            .resolution
            .clone()
            .tightened(options.resolution_limits().unwrap_or_default());
        if let Some(limits) = options.path_binding().and_then(|binding| binding.resolution_limits()) {
            resolution = resolution.tightened(limits);
        }
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
                    let host_root = Root::open(Path::new("/"))
                        .map_err(|source| {
                            LocalFileError::from_io(
                                LocalFileOperation::OpenRoot,
                                Some(path.to_path_buf()),
                                None,
                                source,
                            )
                        })?
                        .with_resolution_contract(resolution.clone());
                    let relative = absolute.strip_prefix("/").unwrap_or(&absolute);
                    let policy = if policy == LocalSymlinkPolicy::FollowAcrossScope {
                        LocalSymlinkPolicy::FollowWithinScope
                    } else {
                        policy
                    };
                    let deadline = path_binding_deadline(
                        started_at,
                        options.path_binding(),
                        LocalFileOperation::OpenRoot,
                        Some(path),
                    )?;
                    validate_parent_steps(
                        &host_root,
                        relative,
                        policy,
                        path,
                        deadline,
                        options.component_policy(),
                        path_budget.as_mut(),
                        RootParentBehavior::StayAtHostRoot,
                    )?;
                    let resolved =
                        crate::rooted_local_file_system::resolve_host_root_path_with_component_policy_and_budget(
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
                    let policy = if policy == LocalSymlinkPolicy::FollowAcrossScope {
                        LocalSymlinkPolicy::FollowWithinScope
                    } else {
                        policy
                    };
                    let opened = crate::rooted_local_file_system::RootedLocalFileSystem::open_with_contract(
                        path,
                        resolution.clone(),
                        policy,
                    )?;
                    let authority = opened.root_authority();
                    let root = Root::from_directory_authority_with_contract(
                        authority.try_clone_authority().map_err(|source| {
                            LocalFileError::from_io(
                                LocalFileOperation::OpenRoot,
                                Some(path.to_path_buf()),
                                None,
                                source,
                            )
                        })?,
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
                let authority = source_root.try_clone_authority().map_err(|source| {
                    LocalFileError::from_io(LocalFileOperation::OpenRoot, Some(path.to_path_buf()), None, source)
                })?;
                let working_root = Root::from_directory_authority_with_contract(
                    authority,
                    source_root.path().to_path_buf(),
                    resolution.clone(),
                );
                let virtual_path = if path.is_absolute() {
                    path.to_path_buf()
                } else {
                    filesystem.current_directory()?.join(path)
                };
                let authority_path = virtual_path
                    .strip_prefix(Path::new(std::path::MAIN_SEPARATOR_STR))
                    .unwrap_or(&virtual_path);
                let deadline = path_binding_deadline(
                    started_at,
                    options.path_binding(),
                    LocalFileOperation::OpenRoot,
                    Some(path),
                )?;
                validate_parent_steps(
                    &working_root,
                    authority_path,
                    policy,
                    path,
                    deadline,
                    options.component_policy(),
                    path_budget.as_mut(),
                    RootParentBehavior::RejectEscape,
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

    /// Returns observations retained when this handle's path binding opened.
    ///
    /// # Parameters
    /// - `limits`: Collection limits applied to the returned ancestor sequence.
    ///
    /// # Returns
    /// A cloned sequence of observations retained when path binding was
    /// enabled.
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

    /// Copies between two relative paths beneath this authority.
    ///
    /// The component policy validates the source and destination paths during
    /// path resolution. Names discovered while copying a tree are not filtered
    /// by that policy.
    ///
    /// Invalid operands produce an unchanged typed copy failure.
    ///
    /// # Parameters
    /// - `source`: Source path beneath this authority.
    /// - `destination`: Destination path beneath this authority.
    /// - `options`: Copy policy, limits, and conflict behavior.
    ///
    /// # Returns
    /// A typed copy result describing completion or failure and any partial
    /// effect.
    ///
    /// # Errors
    /// Returns the primary typed copy error together with the proven
    /// destination state, partial statistics, and any retained staging or
    /// cleanup details.
    pub fn copy_with_options(&self, source: &Path, destination: &Path, options: &LocalCopyOptions) -> LocalCopyResult {
        let started_at = Instant::now();
        let policy = options
            .symlink_policy_override()
            .unwrap_or(self.filesystem.symlink_policy());
        if let Err(error) = crate::local_file_system_validation::validate_copy_options(
            self.filesystem.scope(),
            self.filesystem.symlink_policy(),
            options,
            self.filesystem.capabilities(),
            Some(source),
            Some(destination),
        ) {
            return Err(crate::local::copy_failure_unchanged(error));
        }
        let operation_deadline = options.deadline().and_then(|duration| started_at.checked_add(duration));
        if options.deadline().is_some() && operation_deadline.is_none() {
            return Err(crate::local::copy_failure_unchanged(
                LocalFileError::new(LocalFileErrorKind::InvalidOptions, LocalFileOperation::Copy)
                    .with_reason("copy deadline exceeds the monotonic clock range")
                    .with_path(source.to_path_buf())
                    .with_target(destination.to_path_buf()),
            ));
        }
        let binding_deadline = self.binding_deadline.and_then(|limit| started_at.checked_add(limit));
        if self.binding_deadline.is_some() && binding_deadline.is_none() {
            return Err(crate::local::copy_failure_unchanged(
                LocalFileError::new(LocalFileErrorKind::InvalidOptions, LocalFileOperation::Copy)
                    .with_reason("path binding deadline exceeds the monotonic clock range")
                    .with_path(source.to_path_buf())
                    .with_target(destination.to_path_buf()),
            ));
        }
        let effective_deadline = match (operation_deadline, binding_deadline) {
            (Some(operation), Some(binding)) => Some(operation.min(binding)),
            (operation, binding) => operation.or(binding),
        };
        if let Err(error) = check_operation_deadline(effective_deadline, source, LocalFileOperation::Copy) {
            return Err(crate::local::copy_failure_unchanged(
                error.with_target(destination.to_path_buf()),
            ));
        }
        let resolved_source = self
            .validate_with_policy_until(source, false, policy, effective_deadline, LocalFileOperation::Copy)
            .map_err(|error| crate::local::copy_failure_unchanged(error.with_target(destination.to_path_buf())))?;
        let resolved_destination = self
            .validate_with_policy_until(destination, false, policy, effective_deadline, LocalFileOperation::Copy)
            .map_err(|error| {
                crate::local::copy_failure_unchanged(
                    error
                        .with_path(source.to_path_buf())
                        .with_target(destination.to_path_buf()),
                )
            })?;
        if let Err(error) = check_operation_deadline(effective_deadline, source, LocalFileOperation::Copy) {
            return Err(crate::local::copy_failure_unchanged(
                error.with_target(destination.to_path_buf()),
            ));
        }
        #[cfg(all(test, unix))]
        wait_at_component_policy_checkpoint(&self.diagnostic_path, source, destination);
        self.filesystem
            .copy_with_options_started_at(&resolved_source, &resolved_destination, options, started_at)
    }

    /// Creates a directory beneath this authority.
    ///
    /// # Parameters
    /// - `relative`: New directory path beneath this authority.
    /// - `options`: Creation and parent-directory behavior.
    ///
    /// # Returns
    /// The outcome describing whether the directory was created or already
    /// existed.
    ///
    /// # Errors
    /// Returns path, policy, conflict, requirement, or native I/O errors.
    pub fn create_directory_with_options(
        &self,
        relative: &Path,
        options: &LocalCreateDirectoryOptions,
    ) -> LocalResult<LocalCreateDirectoryOutcome> {
        self.filesystem
            .create_directory_with_options(&self.checked(relative, false)?, options)
    }

    /// Reads final-entry metadata beneath this authority.
    ///
    /// Returns `None` only when the entry is safely absent.
    ///
    /// # Parameters
    /// - `relative`: Entry path resolved beneath this directory authority.
    ///
    /// # Returns
    /// `Some` with metadata when the entry exists, or `None` when it is safely
    /// absent.
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

    /// Starts a directory listing for a relative path under this authority.
    ///
    /// The component policy validates the supplied path during path
    /// resolution. Names discovered by recursive traversal are not filtered
    /// by that policy.
    ///
    /// # Parameters
    /// - `relative`: Directory path to list beneath this authority.
    /// - `options`: Traversal limits and listing behavior.
    ///
    /// # Returns
    /// A walker that yields entries according to the supplied traversal
    /// options.
    ///
    /// # Errors
    /// Returns path, policy, option, type, or native opening errors.
    pub fn list_with_options(&self, relative: &Path, options: &LocalListOptions) -> LocalResult<LocalDirectoryWalker> {
        self.list_with_options_started_at(relative, options, std::time::Instant::now())
    }

    /// Observes the fixed directory through its retained native handle.
    ///
    /// # Returns
    /// Metadata and optional native identity observed from the retained handle.
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
    ///
    /// # Parameters
    /// - `relative`: Entry path resolved beneath this directory authority.
    ///
    /// # Returns
    /// Metadata and optional native identity for the entry observed without
    /// following its final link.
    ///
    /// # Errors
    /// Returns path, resolution-policy, resource-limit, and native query
    /// errors.
    pub fn observe_entry(&self, relative: &Path) -> LocalResult<LocalFileObservation> {
        self.observe_entry_with_link_count(relative)
            .map(|(observation, _)| observation)
    }

    /// Opens an observed reader beneath this directory authority.
    ///
    /// Reader options and optional path-binding checks follow the same
    /// retained authority as other handle operations.
    ///
    /// # Parameters
    /// - `relative`: File path to open beneath this directory authority.
    /// - `options`: Read and optional observation-verification policy.
    ///
    /// # Returns
    /// A reader bound to the resolved file and requested verification policy.
    ///
    /// # Errors
    /// Returns path, policy, type, observation, resource-limit, or I/O errors.
    pub fn open_observed_reader(
        &self,
        relative: &Path,
        options: &LocalObservedReadOptions,
    ) -> LocalResult<LocalObservedReader> {
        observed::open(self, relative, options)
    }

    /// Opens a writer under this authority using the supplied write options.
    ///
    /// # Parameters
    /// - `relative`: Destination path beneath this authority.
    /// - `options`: Write mode and other write constraints.
    ///
    /// # Returns
    /// A writer configured for the requested destination and write mode.
    ///
    /// # Errors
    /// Returns path, policy, option, requirement, or native opening errors.
    pub fn open_writer_with_options(
        &self,
        relative: &Path,
        options: &LocalWriteOptions,
    ) -> LocalResult<LocalFileWriter> {
        let follow_final = options.mode() != LocalWriteMode::CreateNew;
        self.filesystem
            .open_writer_with_options(&self.checked(relative, follow_final)?, options)
    }

    /// Queries security facts from this handle's retained directory authority.
    ///
    /// Each requested field is queried independently, so returned facts may
    /// describe different instants. The result is not an atomic snapshot;
    /// `Unknown` locality does not establish that the filesystem is local.
    /// Reparse-point handling remains the responsibility of the resolution
    /// policy used to obtain this handle.
    ///
    /// # Parameters
    /// - `options`: Selects which security facts are requested and required.
    ///
    /// # Returns
    /// A best-effort observation containing independently queried facts; fields
    /// that could not be established are represented according to the options.
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

    /// Clones the native authority used to query security properties.
    ///
    /// The returned file refers to the retained directory object rather than
    /// reopening its diagnostic path.
    ///
    /// # Returns
    /// A cloned file handle for the retained directory authority.
    ///
    /// # Errors
    /// Returns an `OpenRoot` I/O error if the native authority cannot be
    /// cloned.
    pub(crate) fn security_authority(&self) -> LocalResult<std::fs::File> {
        self.root_authority()?.try_clone_authority().map_err(|source| {
            LocalFileError::from_io(
                LocalFileOperation::Metadata,
                Some(self.diagnostic_path.clone()),
                None,
                source,
            )
        })
    }

    /// Starts listing with a caller-provided monotonic start time.
    ///
    /// Sharing the start time lets a compound operation enforce one deadline
    /// across validation and walker construction.
    ///
    /// # Parameters
    /// - `relative`: Directory path to list beneath this authority.
    /// - `options`: Traversal limits and listing behavior.
    /// - `started_at`: Monotonic start shared with the enclosing operation.
    ///
    /// # Returns
    /// A walker that yields entries according to the supplied traversal
    /// options.
    ///
    /// # Errors
    /// Returns invalid-option, timeout, path, policy, or native opening errors.
    pub(crate) fn list_with_options_started_at(
        &self,
        relative: &Path,
        options: &LocalListOptions,
        started_at: std::time::Instant,
    ) -> LocalResult<LocalDirectoryWalker> {
        let policy = options.symlink_policy().unwrap_or(self.filesystem.symlink_policy());
        crate::local_file_system_validation::validate_list_options(
            self.filesystem.scope(),
            self.filesystem.symlink_policy(),
            options,
            Some(relative),
        )?;
        let operation_deadline = options.deadline().and_then(|duration| started_at.checked_add(duration));
        if options.deadline().is_some() && operation_deadline.is_none() {
            return Err(
                LocalFileError::new(LocalFileErrorKind::InvalidOptions, LocalFileOperation::List)
                    .with_reason("listing deadline exceeds the monotonic clock range")
                    .with_path(relative.to_path_buf()),
            );
        }
        let binding_deadline = self.binding_deadline.and_then(|limit| started_at.checked_add(limit));
        if self.binding_deadline.is_some() && binding_deadline.is_none() {
            return Err(
                LocalFileError::new(LocalFileErrorKind::InvalidOptions, LocalFileOperation::List)
                    .with_reason("path binding deadline exceeds the monotonic clock range")
                    .with_path(relative.to_path_buf()),
            );
        }
        let effective_deadline = match (operation_deadline, binding_deadline) {
            (Some(operation), Some(binding)) => Some(operation.min(binding)),
            (operation, binding) => operation.or(binding),
        };
        check_operation_deadline(effective_deadline, relative, LocalFileOperation::List)?;
        let resolved =
            self.validate_with_policy_until(relative, true, policy, effective_deadline, LocalFileOperation::List)?;
        check_operation_deadline(effective_deadline, relative, LocalFileOperation::List)?;
        self.filesystem
            .list_with_options_started_at(&resolved, options, started_at)
    }

    /// Observes an entry without following its final link and retains its link
    /// count.
    ///
    /// The link count is optional because some platforms or filesystems do not
    /// report it. The observation and count are read through one resolution
    /// cursor.
    ///
    /// # Parameters
    /// - `relative`: Entry path resolved beneath this directory authority.
    ///
    /// # Returns
    /// The entry observation and its optional reported link count.
    ///
    /// # Errors
    /// Returns path, policy, permission, unsupported-observation, or query
    /// errors.
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

    /// Returns the filesystem configuration retained by this handle.
    ///
    /// The returned reference is borrowed from the handle and carries the
    /// namespace and symlink policy used by its operations.
    ///
    /// # Returns
    /// A reference to the filesystem configuration retained by this handle.
    #[must_use]
    #[inline]
    pub(super) const fn filesystem(&self) -> &LocalFileSystem {
        &self.filesystem
    }

    /// Returns the best-effort path used to annotate errors from this handle.
    ///
    /// This path is diagnostic only and does not grant or redirect authority.
    ///
    /// # Returns
    /// The borrowed diagnostic path associated with this handle.
    #[must_use]
    #[inline]
    pub(crate) fn diagnostic_path(&self) -> &Path {
        &self.diagnostic_path
    }

    /// Borrows the retained rooted authority, rejecting host-only handles.
    ///
    /// The reference remains tied to this handle and cannot outlive it.
    ///
    /// # Returns
    /// The rooted authority retained by this handle.
    ///
    /// # Errors
    /// Returns `InvalidState` when this handle uses the host namespace.
    pub(super) fn root_authority(&self) -> LocalResult<&Root> {
        match &self.filesystem.core.namespace {
            LocalNamespace::Rooted(rooted) => Ok(rooted.root_authority()),
            LocalNamespace::Host => Err(LocalFileError::new(
                LocalFileErrorKind::InvalidState,
                LocalFileOperation::OpenRoot,
            )),
        }
    }

    /// Validates and resolves an operation path with this handle's policy.
    ///
    /// `follow_final` selects whether the final path component may be followed.
    ///
    /// # Parameters
    /// - `relative`: Operation path to resolve beneath this handle.
    /// - `follow_final`: Whether resolution may follow the final link.
    ///
    /// # Returns
    /// The validated path relative to the retained root.
    ///
    /// # Errors
    /// Returns path, policy, resource-limit, and native resolution errors.
    pub(super) fn checked(&self, relative: &Path, follow_final: bool) -> LocalResult<PathBuf> {
        self.validate(relative, follow_final)
    }

    /// Checks lexical path shape and denied components without filesystem
    /// access.
    ///
    /// Parent components may cancel preceding normal components but may not
    /// escape this handle's root.
    ///
    /// # Parameters
    /// - `path`: Candidate relative path to check lexically.
    ///
    /// # Returns
    /// `()` when the path is lexically valid under this handle's component
    /// policy.
    ///
    /// # Errors
    /// Returns `InvalidPath` for rooted or escaping paths, or a
    /// component-policy error.
    pub(super) fn validate_direct_input(&self, path: &Path) -> LocalResult<()> {
        if path.is_absolute() || path.has_root() {
            return Err(
                LocalFileError::new(LocalFileErrorKind::InvalidPath, LocalFileOperation::BindPath)
                    .with_path(path.to_path_buf()),
            );
        }
        let mut depth = 0_usize;
        for component in path.components() {
            match component {
                Component::CurDir => {}
                Component::ParentDir if depth > 0 => depth -= 1,
                Component::ParentDir => {
                    return Err(
                        LocalFileError::new(LocalFileErrorKind::InvalidPath, LocalFileOperation::BindPath)
                            .with_path(path.to_path_buf()),
                    );
                }
                Component::Normal(name) => {
                    self.component_policy
                        .validate_component(name)
                        .map_err(|error| error.with_path(path.to_path_buf()))?;
                    depth += 1;
                }
                Component::RootDir | Component::Prefix(_) => {
                    return Err(
                        LocalFileError::new(LocalFileErrorKind::InvalidPath, LocalFileOperation::BindPath)
                            .with_path(path.to_path_buf()),
                    );
                }
            }
        }
        Ok(())
    }

    /// Resolves a relative path under this handle using its configured policy.
    ///
    /// `follow_final` controls whether resolution follows the final link.
    ///
    /// # Parameters
    /// - `path`: Relative path to resolve under this handle.
    /// - `follow_final`: Whether resolution may follow the final link.
    ///
    /// # Returns
    /// The resolved path relative to the retained root.
    ///
    /// # Errors
    /// Returns path, policy, resource-limit, and native resolution errors.
    fn validate(&self, path: &Path, follow_final: bool) -> LocalResult<PathBuf> {
        self.validate_with_policy(path, follow_final, self.filesystem.symlink_policy())
    }

    /// Resolves a relative path using an explicit symlink policy.
    ///
    /// `follow_final` independently controls whether the final component is
    /// followed; configured binding limits still apply.
    ///
    /// # Parameters
    /// - `path`: Relative path to resolve under this handle.
    /// - `follow_final`: Whether resolution may follow the final component.
    /// - `symlink_policy`: Policy governing symbolic-link resolution.
    ///
    /// # Returns
    /// The resolved path relative to the retained root.
    ///
    /// # Errors
    /// Returns path, policy, resource-limit, and native resolution errors.
    fn validate_with_policy(
        &self,
        path: &Path,
        follow_final: bool,
        symlink_policy: LocalSymlinkPolicy,
    ) -> LocalResult<PathBuf> {
        self.validate_with_policy_until(path, follow_final, symlink_policy, None, LocalFileOperation::BindPath)
    }

    /// Resolves a relative path while enforcing an optional operation deadline.
    ///
    /// The deadline and timeout operation let compound operations share one
    /// time budget and report expiration under their own operation kind.
    ///
    /// # Parameters
    /// - `path`: Relative path to resolve under this handle.
    /// - `follow_final`: Whether resolution may follow the final component.
    /// - `symlink_policy`: Policy governing symbolic-link resolution.
    /// - `operation_deadline`: Optional absolute monotonic deadline.
    /// - `timeout_operation`: Operation label used when the deadline expires.
    ///
    /// # Returns
    /// The resolved path relative to the retained root.
    ///
    /// # Errors
    /// Returns path, policy, resource-limit, and timeout errors.
    fn validate_with_policy_until(
        &self,
        path: &Path,
        follow_final: bool,
        symlink_policy: LocalSymlinkPolicy,
        operation_deadline: Option<Instant>,
        timeout_operation: LocalFileOperation,
    ) -> LocalResult<PathBuf> {
        if path.is_absolute() || path.has_root() {
            return Err(
                LocalFileError::new(LocalFileErrorKind::InvalidPath, LocalFileOperation::BindPath)
                    .with_path(path.to_path_buf()),
            );
        }
        let mut normalized = PathBuf::new();
        for component in path.components() {
            check_operation_deadline(operation_deadline, path, timeout_operation)?;
            match component {
                Component::CurDir => continue,
                Component::ParentDir => {
                    if !normalized.as_os_str().is_empty() {
                        match &self.filesystem.core.namespace {
                            LocalNamespace::Rooted(rooted) => rooted
                                .validate_directory_until(&normalized, symlink_policy, operation_deadline)
                                .map_err(|error| timeout_operation_error(error, timeout_operation))?,
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
        let deadline = match operation_deadline {
            Some(deadline) => Some(deadline),
            None => {
                checked_path_binding_deadline(Instant::now(), self.binding_deadline, timeout_operation, Some(path))?
            }
        };
        check_operation_deadline(deadline, path, timeout_operation)?;
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
            )
            .map_err(|error| timeout_operation_error(error, timeout_operation))?;
        for component in resolved.components() {
            check_operation_deadline(deadline, path, timeout_operation)?;
            if let Component::Normal(name) = component {
                self.component_policy.validate_component(name)?;
            }
        }
        check_operation_deadline(deadline, path, timeout_operation)?;
        Ok(resolved)
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

/// Retains directory identities and authorities for path-binding verification.
///
/// When binding is disabled, the returned observation and anchor collections
/// are absent; otherwise each traversed directory is observed under the same
/// resolution contract and bounded by the supplied budget and deadline.
///
/// # Parameters
/// - `root`: Root authority from which to traverse the directory chain.
/// - `relative`: Directory path to retain beneath `root`.
/// - `diagnostic_path`: Path attached to errors and the returned authority.
/// - `opening_recipe`: Optional original path used for binding verification.
/// - `retain_binding`: Whether to collect observations and authority anchors.
/// - `limits`: Optional path-binding limits and deadline.
/// - `path_budget`: Mutable budget shared by path-binding work.
/// - `symlink_policy`: Policy governing symbolic-link resolution.
/// - `started_at`: Monotonic start time for deadline accounting.
///
/// # Returns
/// The retained root and optional observation and anchor sequences.
///
/// # Errors
/// Returns path, policy, resource-limit, timeout, identity, or native I/O
/// errors.
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
        anchors.push(binding_anchor(
            root,
            root.path().to_path_buf(),
            identity,
            symlink_policy,
        )?);
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
                file,
                authority_path.clone(),
                root.resolution_contract().clone(),
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
        cursor.into_file(),
        diagnostic_path.to_path_buf(),
        root.resolution_contract().clone(),
    );
    Ok((
        root,
        retain_binding.then_some(observations),
        retain_binding.then_some(anchors),
    ))
}

/// Resolves each prefix before a parent component to prevent escaping through
/// links.
///
/// `parent_behavior` distinguishes host-root clamping from rooted-scope escape
/// rejection while preserving the original input in reported path errors.
///
/// # Parameters
/// - `root`: Root authority used to resolve prefixes.
/// - `path`: Path whose parent-component prefixes are checked.
/// - `policy`: Symlink policy used while resolving each prefix.
/// - `original`: Original path attached to any reported error.
/// - `deadline`: Optional absolute monotonic deadline for resolution.
/// - `component_policy`: Deny-name policy applied to path components.
/// - `path_budget`: Mutable path-byte budget shared by resolution.
/// - `parent_behavior`: Whether parents clamp at the host root or reject
///   escape.
///
/// # Returns
/// `()` when all parent-step prefixes satisfy the selected scope behavior.
///
/// # Errors
/// Returns invalid-path, timeout, policy, or native resolution errors.
#[allow(clippy::too_many_arguments)]
fn validate_parent_steps(
    root: &Root,
    path: &Path,
    policy: LocalSymlinkPolicy,
    original: &Path,
    deadline: Option<Instant>,
    component_policy: &LocalComponentPolicy,
    mut path_budget: Option<&mut PathBindingBudget>,
    parent_behavior: RootParentBehavior,
) -> LocalResult<()> {
    let mut prefix = PathBuf::new();
    for component in path.components() {
        match component {
            Component::Normal(name) => prefix.push(name),
            Component::ParentDir => {
                if !prefix.as_os_str().is_empty() {
                    let resolve = if parent_behavior == RootParentBehavior::StayAtHostRoot {
                        crate::rooted_local_file_system::resolve_host_root_path_with_component_policy_and_budget
                    } else {
                        crate::rooted_local_file_system::resolve_rooted_path_allow_root_with_component_policy_and_budget
                    };
                    resolve(
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
                    if parent_behavior == RootParentBehavior::StayAtHostRoot {
                        continue;
                    }
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

/// Returns a timeout error when an optional monotonic deadline has expired.
///
/// The test build may supply a controlled clock; production uses
/// `Instant::now`.
///
/// # Parameters
/// - `deadline`: Optional absolute monotonic deadline to check.
/// - `path`: Path attached to a timeout error.
/// - `operation`: Operation label attached to a timeout error.
///
/// # Returns
/// `Ok(())` before the deadline, otherwise a timed-out operation error.
///
/// # Errors
/// Returns a timed-out I/O error when the deadline has expired.
fn check_operation_deadline(deadline: Option<Instant>, path: &Path, operation: LocalFileOperation) -> LocalResult<()> {
    let Some(deadline) = deadline else {
        return Ok(());
    };
    #[cfg(test)]
    let now = crate::tests::rooted::support::resolution_observation_tests::resolution_now(Some(deadline));
    #[cfg(not(test))]
    let now = Instant::now();
    if now >= deadline {
        return Err(LocalFileError::from_io(
            operation,
            Some(path.to_path_buf()),
            None,
            std::io::Error::new(std::io::ErrorKind::TimedOut, "path resolution deadline exceeded"),
        ));
    }
    Ok(())
}

/// Reassigns timeout errors to the enclosing operation when their source is a
/// resolution timeout.
///
/// Listing-specific timeout errors keep their existing operation
/// classification.
///
/// # Parameters
/// - `error`: Resolution error to classify for the enclosing operation.
/// - `operation`: Operation to assign to a matching timeout error.
///
/// # Returns
/// The adjusted timeout error, or the original error when it should be
/// preserved.
fn timeout_operation_error(error: LocalFileError, operation: LocalFileOperation) -> LocalFileError {
    if error.io_error_kind() == std::io::ErrorKind::TimedOut
        && (operation != LocalFileOperation::List
            || error
                .io_error()
                .is_some_and(|source| source.to_string() == "local listing deadline exceeded"))
    {
        error.with_operation(operation)
    } else {
        error
    }
}

/// Computes the absolute deadline for an optional path-binding time limit.
///
/// Clock-range overflow is returned as an operation error by the shared helper.
///
/// # Parameters
/// - `started_at`: Monotonic start time for the operation.
/// - `limits`: Optional binding settings containing a relative time limit.
/// - `operation`: Operation label used for deadline-construction errors.
/// - `path`: Optional path attached to a deadline-construction error.
///
/// # Returns
/// The absolute deadline, or `None` when no binding deadline is configured.
///
/// # Errors
/// Returns an operation error if the deadline exceeds the monotonic clock
/// range.
fn path_binding_deadline(
    started_at: Instant,
    limits: Option<&LocalPathBindingOptions>,
    operation: LocalFileOperation,
    path: Option<&Path>,
) -> LocalResult<Option<Instant>> {
    checked_path_binding_deadline(started_at, limits.and_then(|limits| limits.deadline()), operation, path)
}

/// Checks whether the elapsed time for path-binding work exceeded its limit.
///
/// Expiration is reported as a timed-out `OpenRoot` operation with the supplied
/// diagnostic path.
///
/// # Parameters
/// - `limits`: Optional binding settings containing the elapsed-time limit.
/// - `started_at`: Monotonic start time for the binding work.
/// - `path`: Diagnostic path attached to a timeout error.
///
/// # Returns
/// `()` while within the configured time limit.
///
/// # Errors
/// Returns a timed-out `OpenRoot` error after the configured limit expires.
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

/// Builds a retained authority anchor for later path-binding identity checks.
///
/// The anchor stores the supplied native identity, resolution contract, and
/// symlink policy alongside a cloned directory authority.
///
/// # Parameters
/// - `root`: Authority to clone for the anchor.
/// - `path`: Diagnostic and opening path retained by the anchor.
/// - `identity`: Native identity observed for the directory.
/// - `symlink_policy`: Policy used for later identity checks.
///
/// # Returns
/// An anchor retaining the cloned authority and supplied identity contract.
///
/// # Errors
/// Returns an `OpenRoot` I/O error if the native authority cannot be cloned.
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
