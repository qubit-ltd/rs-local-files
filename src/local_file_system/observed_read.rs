// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Stateful Host or Rooted local filesystem service.

use std::path::Path;
use std::path::PathBuf;

use super::LocalFileError;
use super::LocalFileErrorKind;
use super::LocalFileOperation;
use super::LocalFileSystem;
use super::LocalNamespace;
use super::LocalResult;
use super::operation_error;
use super::resolve_operation_path;
use super::validate_scope_symlink_policy;
use crate::LocalSymlinkPolicy;
use crate::directory::binding::DirectoryPathBinding;
use crate::directory::binding::PathBinding;
use crate::local_file_system::internal::observed_read_path::ObservedReadPath;
use crate::local_file_system::io::has_directory_qualification;
use crate::observation::LocalObservedReader;
use crate::options::LocalObservedReadOptions;
use crate::path::LocalNamespacePath;
use crate::resolution::internal::checked_path_binding_deadline;
use crate::rooted::Root;

impl LocalFileSystem {
    /// Opens a reader with same-handle observations and optional name binding.
    /// Windows Host scope rejects path binding because drive-root ancestor
    /// traversal is not available for that namespace.
    ///
    /// # Parameters
    ///
    /// - `path`: Namespace path to resolve and open.
    /// - `options`: Observation, read, and optional path-binding policy.
    ///
    /// # Returns
    ///
    /// An observed reader for the resolved file.
    ///
    /// # Errors
    /// Returns path resolution, native open, observation, and binding-limit
    /// errors. A finite effective resolution budget on Host is unsupported.
    pub fn open_observed_reader(
        &self,
        path: &Path,
        options: &LocalObservedReadOptions,
    ) -> LocalResult<LocalObservedReader> {
        let resolver_started = std::time::Instant::now();
        let binding_options = options.path_binding().copied();
        #[cfg(windows)]
        if binding_options.is_some() && matches!(&self.core.namespace, LocalNamespace::Host) {
            return Err(
                LocalFileError::new(LocalFileErrorKind::Unsupported, LocalFileOperation::OpenReader)
                    .with_reason("Windows Host path binding requires drive-root ancestor traversal"),
            );
        }
        let binding_deadline = checked_path_binding_deadline(
            resolver_started,
            binding_options.and_then(|limits| limits.deadline()),
            LocalFileOperation::OpenReader,
            Some(path),
        )?;
        self.open_observed_reader_with_binding_limits(
            ObservedReadPath::Namespace(path),
            options,
            binding_options,
            binding_deadline,
            true,
            None,
        )
    }
    /// Opens an observed reader using caller-computed binding limits and
    /// deadline.
    ///
    /// The reader does not attach a new path binding; callers use this entry
    /// point when the enclosing authority owns binding verification.
    ///
    /// # Parameters
    ///
    /// - `path`: Namespace path to resolve and open.
    /// - `options`: Observation and read policy.
    /// - `limits`: Binding limits selected by the enclosing authority.
    /// - `deadline`: Binding deadline computed by the enclosing authority.
    ///
    /// # Returns
    ///
    /// An observed reader without an attached path binding.
    ///
    /// # Errors
    ///
    /// Returns path-resolution, opening, observation, or binding-limit errors.
    pub(crate) fn open_observed_reader_with_binding_deadline(
        &self,
        path: &Path,
        options: &LocalObservedReadOptions,
        limits: crate::options::LocalPathBindingOptions,
        deadline: Option<std::time::Instant>,
    ) -> LocalResult<LocalObservedReader> {
        self.open_observed_reader_with_binding_limits(
            ObservedReadPath::Namespace(path),
            options,
            Some(limits),
            deadline,
            false,
            None,
        )
    }
    /// Opens an observed reader using an existing fixed-directory authority.
    ///
    /// # Parameters
    ///
    /// - `original`: Path used to retain the directory binding.
    /// - `request_path`: Path to resolve relative to the fixed authority.
    /// - `component_policy`: Policy for traversed path components.
    /// - `options`: Observation, read, and optional path-binding policy.
    /// - `authority_binding_deadline`: Optional deadline imposed by the
    ///   authority.
    ///
    /// # Returns
    ///
    /// An observed reader bound to the fixed directory authority.
    ///
    /// # Errors
    ///
    /// Returns path-resolution, opening, observation, binding-limit, or
    /// deadline errors.
    pub(crate) fn open_observed_reader_fixed(
        &self,
        original: &Path,
        request_path: &Path,
        component_policy: &crate::directory::LocalComponentPolicy,
        options: &LocalObservedReadOptions,
        authority_binding_deadline: Option<std::time::Duration>,
    ) -> LocalResult<LocalObservedReader> {
        let resolver_started = std::time::Instant::now();
        let binding_options = options.path_binding().copied();
        let binding_deadline = checked_path_binding_deadline(
            resolver_started,
            binding_options
                .and_then(|limits| limits.deadline())
                .into_iter()
                .chain(authority_binding_deadline)
                .min(),
            LocalFileOperation::OpenReader,
            Some(original),
        )?;
        self.open_observed_reader_with_binding_limits(
            ObservedReadPath::FixedRelative {
                original,
                request_path,
                component_policy,
            },
            options,
            binding_options,
            binding_deadline,
            true,
            authority_binding_deadline,
        )
    }
    /// Reopens a fixed-relative source during path-binding verification.
    ///
    /// # Parameters
    ///
    /// - `original`: Original path whose binding is being checked.
    /// - `request_path`: Path to reopen relative to the fixed authority.
    /// - `component_policy`: Policy for traversed path components.
    /// - `options`: Observation and read policy for the reopened handle.
    /// - `deadline`: Previously computed reader binding deadline.
    /// - `authority_binding_deadline`: Optional deadline imposed by the
    ///   authority.
    ///
    /// # Returns
    ///
    /// An observed reader reopened under the supplied deadline.
    ///
    /// # Errors
    ///
    /// Returns path-resolution, opening, observation, binding-limit, or
    /// deadline errors.
    pub(crate) fn open_observed_reader_fixed_with_deadline(
        &self,
        original: &Path,
        request_path: &Path,
        component_policy: &crate::directory::LocalComponentPolicy,
        options: &LocalObservedReadOptions,
        deadline: Option<std::time::Instant>,
        authority_binding_deadline: Option<std::time::Duration>,
    ) -> LocalResult<LocalObservedReader> {
        let started_at = std::time::Instant::now();
        let authority_deadline = checked_path_binding_deadline(
            started_at,
            authority_binding_deadline,
            LocalFileOperation::OpenReader,
            Some(original),
        )?;
        let deadline = match (deadline, authority_deadline) {
            (Some(explicit), Some(authority)) => Some(explicit.min(authority)),
            (explicit, authority) => explicit.or(authority),
        };
        self.open_observed_reader_with_binding_limits(
            ObservedReadPath::FixedRelative {
                original,
                request_path,
                component_policy,
            },
            options,
            options.path_binding().copied(),
            deadline,
            false,
            authority_binding_deadline,
        )
    }
    /// Resolves a fixed-relative source under the retained Rooted authority.
    ///
    /// Namespace paths are already resolved by the caller and return `None`.
    /// Fixed-relative paths validate their relative form, clone the authority,
    /// debit their original bytes, then resolve components under the same
    /// deadline and mutable binding budget.
    ///
    /// # Parameters
    ///
    /// - `path_mode`: Source path and component policy to resolve.
    /// - `resolution`: Effective resolution contract selected by the caller.
    /// - `symlink_policy`: Policy applied while traversing the fixed path.
    /// - `binding_deadline`: Deadline shared with the enclosing operation.
    /// - `path_budget`: Remaining path-byte budget, updated during resolution.
    ///
    /// # Returns
    ///
    /// A resolved path for fixed-relative sources, or `None` for namespace
    /// paths.
    ///
    /// # Errors
    ///
    /// Returns invalid-state, invalid-path, authority-cloning, or
    /// path-resolution errors without changing their operation context.
    fn resolve_fixed_observed_path(
        &self,
        path_mode: &ObservedReadPath<'_>,
        resolution: &crate::resolution::ResolutionContract,
        symlink_policy: LocalSymlinkPolicy,
        binding_deadline: Option<std::time::Instant>,
        path_budget: &mut Option<crate::directory::binding::PathBindingBudget>,
    ) -> LocalResult<Option<PathBuf>> {
        Ok(match path_mode {
            ObservedReadPath::Namespace(_) => None,
            ObservedReadPath::FixedRelative {
                original,
                component_policy,
                ..
            } => {
                if original.is_absolute() || original.has_root() {
                    return Err(
                        LocalFileError::new(LocalFileErrorKind::InvalidPath, LocalFileOperation::OpenReader)
                            .with_path(original.to_path_buf()),
                    );
                }
                let LocalNamespace::Rooted(rooted) = &self.core.namespace else {
                    return Err(LocalFileError::new(
                        LocalFileErrorKind::InvalidState,
                        LocalFileOperation::OpenRoot,
                    ));
                };
                let source_root = rooted.root_authority();
                let authority = Root::from_directory_authority_with_contract(
                    source_root.try_clone_authority().map_err(|source| {
                        LocalFileError::from_io(
                            LocalFileOperation::OpenRoot,
                            Some(source_root.path().to_path_buf()),
                            None,
                            source,
                        )
                    })?,
                    source_root.path().to_path_buf(),
                    resolution.clone(),
                );
                if let Some(budget) = path_budget.as_mut() {
                    budget.debit(original.as_os_str().len(), LocalFileOperation::OpenReader, original)?;
                }
                Some(
                    crate::rooted_local_file_system::resolve_rooted_path_allow_root_with_component_policy_and_budget(
                        &authority,
                        original,
                        symlink_policy,
                        true,
                        LocalFileOperation::OpenReader,
                        component_policy,
                        binding_deadline,
                        path_budget.as_mut(),
                    )?,
                )
            }
        })
    }
    /// Resolves, opens, and optionally binds an observed reader for either
    /// namespace paths or fixed-relative paths.
    ///
    /// # Parameters
    ///
    /// - `path_mode`: Namespace or fixed-relative source path and component
    ///   policy.
    /// - `options`: Observation and read policy.
    /// - `binding_options`: Optional limits used to retain the path binding.
    /// - `binding_deadline`: Effective deadline for path resolution and
    ///   binding.
    /// - `attach_binding`: Whether to attach a binding to the returned reader.
    /// - `authority_binding_deadline`: Optional enclosing-authority deadline.
    ///
    /// # Returns
    ///
    /// An observed reader for the resolved file.
    ///
    /// # Errors
    ///
    /// Returns path-resolution, opening, observation, binding-limit, or
    /// deadline errors.
    fn open_observed_reader_with_binding_limits(
        &self,
        path_mode: ObservedReadPath<'_>,
        options: &LocalObservedReadOptions,
        binding_options: Option<crate::options::LocalPathBindingOptions>,
        binding_deadline: Option<std::time::Instant>,
        attach_binding: bool,
        authority_binding_deadline: Option<std::time::Duration>,
    ) -> LocalResult<LocalObservedReader> {
        let resolver_started = std::time::Instant::now();
        let path = path_mode.original();
        let request_path = path_mode.request_path();
        let fixed_path_requires_directory =
            matches!(&path_mode, ObservedReadPath::FixedRelative { .. }) && has_directory_qualification(path);
        let read_options = options.read_options().unwrap_or(&self.defaults.read);
        let symlink_policy = self.read_symlink_policy(read_options, None);
        let mut resolution = self.core.resolution.clone();
        if let Some(limits) = options.resolution_limits() {
            resolution = resolution.tightened(limits);
        }
        if let Some(limits) = binding_options.and_then(|binding| binding.resolution_limits()) {
            resolution = resolution.tightened(limits);
        }
        if matches!(&self.core.namespace, LocalNamespace::Host)
            && resolution.limits() != crate::options::LocalResolutionLimits::new()
        {
            return Err(
                LocalFileError::new(LocalFileErrorKind::Unsupported, LocalFileOperation::OpenReader)
                    .with_path(path.to_path_buf())
                    .with_reason("Host observed reads do not support finite resolution limits"),
            );
        }
        validate_scope_symlink_policy(self.scope(), symlink_policy, LocalFileOperation::OpenReader, Some(path))?;
        let mut path_budget =
            binding_options.map(|limits| crate::directory::binding::PathBindingBudget::new(limits.max_path_bytes()));
        check_binding_deadline(binding_deadline, path)?;
        let fixed_resolved = self
            .resolve_fixed_observed_path(
                &path_mode,
                &resolution,
                symlink_policy,
                binding_deadline,
                &mut path_budget,
            )
            .map_err(|error| operation_error(error, LocalFileOperation::OpenReader, request_path, None, None))?;
        let operation_path = fixed_resolved.as_deref().unwrap_or(path);
        let resolver = self.resolver_for(operation_path, LocalFileOperation::OpenReader)?;
        let resolved = resolve_operation_path(&resolver, operation_path, LocalFileOperation::OpenReader)?;
        let error_path = match path_mode {
            ObservedReadPath::Namespace(_) => resolved.namespace_absolute().to_path_buf(),
            ObservedReadPath::FixedRelative { .. } => request_path.to_path_buf(),
        };
        let error_current_directory = match path_mode {
            ObservedReadPath::Namespace(_) => resolver.current_directory().map(Path::to_path_buf),
            ObservedReadPath::FixedRelative { .. } => None,
        };
        let result = (|| -> LocalResult<LocalObservedReader> {
            if fixed_path_requires_directory {
                return Err(
                    LocalFileError::new(LocalFileErrorKind::NotDirectory, LocalFileOperation::OpenReader)
                        .with_reason("a directory-qualified path cannot be used as a file")
                        .with_path(error_path.clone()),
                );
            }
            if fixed_resolved.is_some()
                && let Some(budget) = path_budget.as_mut()
            {
                budget.debit(
                    resolved.namespace_absolute().as_os_str().len(),
                    LocalFileOperation::OpenReader,
                    path,
                )?;
            }
            if attach_binding && let Some(binding_options) = binding_options {
                let ancestors = match &path_mode {
                    ObservedReadPath::FixedRelative { original, .. } => original
                        .components()
                        .count()
                        .saturating_sub(1)
                        .max(resolved.namespace_absolute().components().count().saturating_sub(1)),
                    ObservedReadPath::Namespace(_) => {
                        resolved.namespace_absolute().components().count().saturating_sub(1)
                    }
                };
                if binding_options.max_ancestors().is_some_and(|limit| ancestors > limit)
                    || binding_options.max_ancestors() == Some(0)
                {
                    return Err(
                        LocalFileError::new(LocalFileErrorKind::ResourceLimit, LocalFileOperation::OpenReader)
                            .with_path(path.to_path_buf()),
                    );
                }
            }
            check_binding_deadline(binding_deadline, path)?;
            let namespace_absolute = if path.is_absolute() {
                path.to_path_buf()
            } else {
                resolver
                    .current_directory()
                    .unwrap_or_else(|| Path::new("/"))
                    .join(path)
            };
            let authority_path = namespace_absolute
                .strip_prefix(Path::new(std::path::MAIN_SEPARATOR_STR))
                .unwrap_or(&namespace_absolute);
            if binding_options.is_some() && fixed_resolved.is_none() {
                let absolute_bytes = resolved.namespace_absolute().as_os_str().len();
                let raw_component_bytes = path
                    .components()
                    .filter_map(|component| match component {
                        std::path::Component::Normal(name) => Some(name.len().saturating_add(1)),
                        _ => None,
                    })
                    .fold(0_usize, usize::saturating_add);
                if let Some(budget) = path_budget.as_mut() {
                    budget.debit(
                        absolute_bytes,
                        LocalFileOperation::OpenReader,
                        resolved.namespace_absolute(),
                    )?;
                    budget.debit(
                        raw_component_bytes,
                        LocalFileOperation::OpenReader,
                        resolved.namespace_absolute(),
                    )?;
                }
                let (root, symlink_policy) = match &self.core.namespace {
                    LocalNamespace::Host => (
                        Root::open(Path::new("/"))
                            .map_err(|source| {
                                LocalFileError::from_io(
                                    LocalFileOperation::OpenRoot,
                                    Some(namespace_absolute.clone()),
                                    None,
                                    source,
                                )
                            })?
                            .with_resolution_contract(resolution.clone()),
                        if symlink_policy == LocalSymlinkPolicy::FollowAcrossScope {
                            LocalSymlinkPolicy::FollowWithinScope
                        } else {
                            symlink_policy
                        },
                    ),
                    LocalNamespace::Rooted(rooted) => {
                        let authority = rooted.root_authority();
                        let file = authority.try_clone_authority().map_err(|source| {
                            LocalFileError::from_io(
                                LocalFileOperation::OpenRoot,
                                Some(namespace_absolute.clone()),
                                None,
                                source,
                            )
                        })?;
                        (
                            Root::from_directory_authority_with_contract(
                                file,
                                authority.path().to_path_buf(),
                                resolution.clone(),
                            ),
                            symlink_policy,
                        )
                    }
                };
                let allow_all = crate::directory::LocalComponentPolicy::allow_all();
                crate::rooted_local_file_system::resolve_rooted_path_allow_root_with_component_policy_and_budget(
                    &root,
                    authority_path,
                    symlink_policy,
                    true,
                    LocalFileOperation::OpenReader,
                    &allow_all,
                    binding_deadline,
                    path_budget.as_mut(),
                )?;
                check_binding_deadline(binding_deadline, resolved.namespace_absolute())?;
            }
            #[cfg(test)]
            crate::tests::rooted::support::resolution_observation_tests::reach_deadline_boundary(
                crate::tests::rooted::support::resolution_observation_tests::DeadlineBoundary::ObservedReaderBeforeContentResolution,
            );
            let reader = self.open_resolved_reader_with_policy_and_contract(
                &resolved,
                read_options,
                LocalFileOperation::OpenReader,
                resolver.current_directory(),
                symlink_policy,
                Some(resolution.clone()),
                binding_deadline,
            )?;
            check_binding_deadline(binding_deadline, path)?;
            let observed = LocalObservedReader::from_reader(reader, options.native_observation())?
                .bind_error_context(error_path.clone(), error_current_directory.clone());
            let observed = self.attach_observed_reader_binding(
                observed,
                &path_mode,
                path,
                request_path,
                &resolved,
                &namespace_absolute,
                authority_path,
                options,
                binding_options,
                attach_binding,
                binding_deadline,
                authority_binding_deadline,
                &resolution,
                &mut path_budget,
                symlink_policy,
                resolver_started,
            )?;
            Ok(observed)
        })();
        result.map_err(|error| {
            operation_error(
                error,
                LocalFileOperation::OpenReader,
                &error_path,
                None,
                error_current_directory.as_deref(),
            )
        })
    }
    /// Attaches fixed-relative or namespace pathname checks to an observed
    /// reader.
    ///
    /// Fixed-directory sources retain their original recipe. Namespace sources
    /// retain both the file binding and parent-directory anchors. When callers
    /// own verification, the reader is returned without installing a binding.
    ///
    /// # Parameters
    ///
    /// - `observed`: Reader and same-handle observation to augment.
    /// - `path_mode`: Namespace or fixed-relative source interpretation.
    /// - `path`: Original path used for deadline checks and diagnostics.
    /// - `request_path`: Path coordinate associated with the source request.
    /// - `resolved`: Resolved namespace path and authority-relative operand.
    /// - `namespace_absolute`: Absolute namespace path used to retain
    ///   authorities.
    /// - `authority_path`: Namespace path relative to the authority root.
    /// - `options`: Observation and read policy captured by the binding.
    /// - `binding_options`: Optional limits selecting whether a binding is
    ///   attached.
    /// - `attach_binding`: Whether this reader owns path-binding verification.
    /// - `binding_deadline`: Effective deadline shared by all binding stages.
    /// - `authority_binding_deadline`: Optional fixed-authority deadline.
    /// - `resolution`: Effective contract used for authority-relative rechecks.
    /// - `path_budget`: Remaining shared path-byte budget, updated by
    ///   retention.
    /// - `symlink_policy`: Policy used to resolve and recheck the path.
    /// - `resolver_started`: Start time for retained directory-chain limits.
    ///
    /// # Returns
    ///
    /// The reader with the selected path binding attached, if requested.
    ///
    /// # Errors
    ///
    /// Returns unsupported identity, path-binding, resource-limit, deadline, or
    /// authority-retention errors.
    #[allow(clippy::too_many_arguments)]
    fn attach_observed_reader_binding(
        &self,
        mut observed: LocalObservedReader,
        path_mode: &ObservedReadPath<'_>,
        path: &Path,
        request_path: &Path,
        resolved: &LocalNamespacePath,
        namespace_absolute: &Path,
        authority_path: &Path,
        options: &LocalObservedReadOptions,
        binding_options: Option<crate::options::LocalPathBindingOptions>,
        attach_binding: bool,
        binding_deadline: Option<std::time::Instant>,
        authority_binding_deadline: Option<std::time::Duration>,
        resolution: &crate::resolution::ResolutionContract,
        path_budget: &mut Option<crate::directory::binding::PathBindingBudget>,
        symlink_policy: LocalSymlinkPolicy,
        resolver_started: std::time::Instant,
    ) -> LocalResult<LocalObservedReader> {
        if attach_binding {
            if let ObservedReadPath::FixedRelative {
                original,
                component_policy,
                ..
            } = path_mode
            {
                if binding_options.is_some() {
                    let identity = observed.observation().identity().cloned().ok_or_else(|| {
                        LocalFileError::new(LocalFileErrorKind::Unsupported, LocalFileOperation::Metadata)
                    })?;
                    observed = observed.with_binding(Box::new(crate::directory::DirectoryReaderPathBinding::new(
                        self.clone(),
                        original.to_path_buf(),
                        request_path.to_path_buf(),
                        (*component_policy).clone(),
                        *options,
                        identity,
                        authority_binding_deadline,
                    )));
                }
                check_binding_deadline(binding_deadline, original)?;
                return Ok(observed);
            }
            let Some(binding_options) = binding_options else {
                check_binding_deadline(binding_deadline, path)?;
                return Ok(observed);
            };
            check_binding_deadline(binding_deadline, resolved.namespace_absolute())?;
            let identity =
                observed.observation().identity().cloned().ok_or_else(|| {
                    LocalFileError::new(LocalFileErrorKind::Unsupported, LocalFileOperation::Metadata)
                })?;
            let parent_anchors = self.retain_observed_parent_anchors(
                resolved,
                namespace_absolute,
                authority_path,
                &binding_options,
                resolution,
                binding_deadline,
                path_budget,
                symlink_policy,
                resolver_started,
            )?;
            check_binding_deadline(binding_deadline, resolved.namespace_absolute())?;
            observed = observed.with_binding(Box::new(PathBinding::new(
                self.clone(),
                resolved.namespace_absolute().to_path_buf(),
                identity,
                *options,
                binding_options,
            )));
            observed = observed.with_binding(Box::new(DirectoryPathBinding::new(
                parent_anchors,
                binding_options.deadline(),
            )));
        }
        check_binding_deadline(binding_deadline, path)?;
        Ok(observed)
    }
    /// Retains the directory chain that verifies a namespace reader binding.
    ///
    /// Host readers resolve from the host root with cross-scope links reduced
    /// to within-scope traversal. Rooted readers retain their existing root
    /// authority and policy. Both paths share the caller's deadline and byte
    /// budget, and return anchors only after the parent chain is retained.
    ///
    /// # Parameters
    ///
    /// - `resolved`: Resolved reader operand and authority-relative path.
    /// - `namespace_absolute`: Absolute diagnostic path for the operand.
    /// - `authority_path`: Absolute namespace path relative to the authority
    ///   root.
    /// - `binding_options`: Limits used when retaining the directory chain.
    /// - `resolution`: Resolution contract shared by temporary authorities.
    /// - `binding_deadline`: Deadline shared with reader opening and binding.
    /// - `path_budget`: Remaining binding path bytes, updated as paths resolve.
    /// - `symlink_policy`: Policy governing component traversal.
    /// - `resolver_started`: Start time used by the retained-chain limits.
    ///
    /// # Returns
    ///
    /// The retained parent-directory anchors in verification order.
    ///
    /// # Errors
    ///
    /// Returns authority, path-resolution, resource-limit, deadline, or native
    /// I/O errors from retaining the parent chain.
    #[allow(clippy::too_many_arguments)]
    fn retain_observed_parent_anchors(
        &self,
        resolved: &LocalNamespacePath,
        namespace_absolute: &Path,
        authority_path: &Path,
        binding_options: &crate::options::LocalPathBindingOptions,
        resolution: &crate::resolution::ResolutionContract,
        binding_deadline: Option<std::time::Instant>,
        path_budget: &mut Option<crate::directory::binding::PathBindingBudget>,
        symlink_policy: LocalSymlinkPolicy,
        resolver_started: std::time::Instant,
    ) -> LocalResult<Vec<crate::directory::binding::BindingAnchor>> {
        let parent_path = resolved.authority_relative().parent().unwrap_or_else(|| Path::new(""));
        let (parent_root, anchors) = match &self.core.namespace {
            LocalNamespace::Host => {
                let host_root = Root::open(Path::new("/"))
                    .map_err(|source| {
                        LocalFileError::from_io(
                            LocalFileOperation::OpenRoot,
                            Some(parent_path.to_path_buf()),
                            None,
                            source,
                        )
                    })?
                    .with_resolution_contract(resolution.clone());
                let relative_parent = parent_path.strip_prefix("/").unwrap_or(parent_path);
                let policy = if symlink_policy == LocalSymlinkPolicy::FollowAcrossScope {
                    LocalSymlinkPolicy::FollowWithinScope
                } else {
                    symlink_policy
                };
                let allow_all = crate::directory::LocalComponentPolicy::allow_all();
                let relative_parent =
                    crate::rooted_local_file_system::resolve_rooted_path_allow_root_with_component_policy_and_budget(
                        &host_root,
                        relative_parent,
                        policy,
                        true,
                        LocalFileOperation::OpenRoot,
                        &allow_all,
                        binding_deadline,
                        path_budget.as_mut(),
                    )?;
                check_binding_deadline(binding_deadline, &relative_parent)?;
                let diagnostic_parent = Path::new("/").join(&relative_parent);
                let (root, _, anchors) = crate::directory::retain_directory_chain(
                    &host_root,
                    &relative_parent,
                    &diagnostic_parent,
                    Some(namespace_absolute.parent().unwrap_or(Path::new("/"))),
                    true,
                    Some(binding_options),
                    path_budget.as_mut(),
                    policy,
                    resolver_started,
                )?;
                (root, anchors.unwrap_or_default())
            }
            LocalNamespace::Rooted(rooted) => {
                let source_root = rooted.root_authority();
                let working_root = Root::from_directory_authority_with_contract(
                    source_root.try_clone_authority().map_err(|source| {
                        LocalFileError::from_io(
                            LocalFileOperation::OpenRoot,
                            Some(resolved.namespace_absolute().to_path_buf()),
                            None,
                            source,
                        )
                    })?,
                    source_root.path().to_path_buf(),
                    resolution.clone(),
                );
                let allow_all = crate::directory::LocalComponentPolicy::allow_all();
                let resolved_parent =
                    crate::rooted_local_file_system::resolve_rooted_path_allow_root_with_component_policy_and_budget(
                        &working_root,
                        parent_path,
                        symlink_policy,
                        true,
                        LocalFileOperation::OpenRoot,
                        &allow_all,
                        binding_deadline,
                        path_budget.as_mut(),
                    )?;
                check_binding_deadline(binding_deadline, &resolved_parent)?;
                let diagnostic_parent = rooted.diagnostic_path().join(&resolved_parent);
                let recipe_parent = rooted
                    .diagnostic_path()
                    .join(authority_path.parent().unwrap_or_else(|| Path::new("")));
                let (root, _, anchors) = crate::directory::retain_directory_chain(
                    &working_root,
                    &resolved_parent,
                    &diagnostic_parent,
                    Some(&recipe_parent),
                    true,
                    Some(binding_options),
                    path_budget.as_mut(),
                    symlink_policy,
                    resolver_started,
                )?;
                (root, anchors.unwrap_or_default())
            }
        };
        drop(parent_root);
        check_binding_deadline(binding_deadline, resolved.namespace_absolute())?;
        Ok(anchors)
    }
}

/// Returns a timeout error when the supplied binding deadline has expired.
///
/// # Parameters
///
/// - `deadline`: Optional monotonic deadline to check.
/// - `path`: Path attached to a timeout error.
///
/// # Errors
///
/// Returns a timeout error when `deadline` has expired.
fn check_binding_deadline(deadline: Option<std::time::Instant>, path: &Path) -> LocalResult<()> {
    if deadline.is_some_and(|deadline| std::time::Instant::now() >= deadline) {
        return Err(LocalFileError::from_io(
            LocalFileOperation::OpenReader,
            Some(path.to_path_buf()),
            None,
            std::io::Error::new(std::io::ErrorKind::TimedOut, "path binding deadline exceeded"),
        ));
    }
    Ok(())
}
