// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Stateful Host or Rooted local filesystem service.

use super::HostLocalFileSystem;
use super::Input;
use super::LocalFileError;
use super::LocalFileErrorKind;
use super::LocalFileKind;
use super::LocalFileMetadata;
use super::LocalFileOperation;
use super::LocalFileReader;
use super::LocalFileSystem;
use super::LocalFileWriter;
use super::LocalNamespace;
use super::LocalReadOptions;
use super::LocalResult;
use super::LocalWriteOptions;
use super::Path;
use super::operation_error;
use super::reject_directory_qualified_file;
use super::resolve_operation_path;
use super::validate_scope_symlink_policy;
use super::with_current_directory;
use crate::LocalSymlinkPolicy;
use crate::directory::binding::DirectoryPathBinding;
use crate::directory::binding::PathBinding;
#[cfg(feature = "test-support")]
use crate::local::take_test_support;
use crate::local_file_system_validation::validate_write_options;
use crate::observation::LocalObservationRequirement;
use crate::observation::LocalObservedReader;
use crate::options::LocalObservedReadOptions;
use crate::options::LocalReadAllOptions;
use crate::options::LocalSnapshotOptions;
use crate::path::LocalNamespacePath;
use crate::rooted::Root;
use crate::test_support::TestFaultPoint;

impl LocalFileSystem {
    /// Opens a reader with same-handle observations and optional name binding.
    /// Windows Host scope rejects path binding because drive-root ancestor
    /// traversal is not available for that namespace.
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
        let binding_deadline = binding_options
            .and_then(|limits| limits.deadline())
            .and_then(|limit| resolver_started.checked_add(limit));
        self.open_observed_reader_with_binding_limits(
            path,
            options,
            binding_options,
            binding_deadline,
            resolver_started,
            true,
        )
    }

    pub(crate) fn open_observed_reader_with_binding_deadline(
        &self,
        path: &Path,
        options: &LocalObservedReadOptions,
        limits: crate::options::LocalPathBindingOptions,
        deadline: Option<std::time::Instant>,
    ) -> LocalResult<LocalObservedReader> {
        self.open_observed_reader_with_binding_limits(
            path,
            options,
            Some(limits),
            deadline,
            std::time::Instant::now(),
            false,
        )
    }

    fn open_observed_reader_with_binding_limits(
        &self,
        path: &Path,
        options: &LocalObservedReadOptions,
        binding_options: Option<crate::options::LocalPathBindingOptions>,
        binding_deadline: Option<std::time::Instant>,
        resolver_started: std::time::Instant,
        attach_binding: bool,
    ) -> LocalResult<LocalObservedReader> {
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
        let resolver = self.resolver_for(path, LocalFileOperation::OpenReader)?;
        let resolved = resolve_operation_path(&resolver, path, LocalFileOperation::OpenReader)?;
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
        if binding_options.is_some() {
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
        let reader = self.open_resolved_reader_with_policy_and_contract(
            &resolved,
            read_options,
            LocalFileOperation::OpenReader,
            resolver.current_directory(),
            symlink_policy,
            Some(resolution.clone()),
        )?;
        check_binding_deadline(binding_deadline, path)?;
        let mut observed = LocalObservedReader::from_reader(reader, options.native_observation())?;
        if attach_binding {
            let Some(binding_options) = binding_options else {
                check_binding_deadline(binding_deadline, path)?;
                return Ok(observed);
            };
            let ancestors = resolved.namespace_absolute().components().count().saturating_sub(1);
            if binding_options.max_ancestors().is_some_and(|limit| ancestors > limit)
                || binding_options.max_ancestors() == Some(0)
            {
                return Err(
                    LocalFileError::new(LocalFileErrorKind::ResourceLimit, LocalFileOperation::OpenReader)
                        .with_path(resolved.namespace_absolute().to_path_buf()),
                );
            }
            check_binding_deadline(binding_deadline, resolved.namespace_absolute())?;
            let identity =
                observed.observation().identity().cloned().ok_or_else(|| {
                    LocalFileError::new(LocalFileErrorKind::Unsupported, LocalFileOperation::Metadata)
                })?;
            let parent_path = resolved.authority_relative().parent().unwrap_or_else(|| Path::new(""));
            let (parent_root, parent_anchors) = match &self.core.namespace {
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
                    let relative_parent = crate::rooted_local_file_system::resolve_rooted_path_allow_root_with_component_policy_and_budget(
                        &host_root, relative_parent, policy, true, LocalFileOperation::OpenRoot, &allow_all,
                        binding_deadline, path_budget.as_mut(),
                    )?;
                    check_binding_deadline(binding_deadline, &relative_parent)?;
                    let diagnostic_parent = Path::new("/").join(&relative_parent);
                    let (root, _, anchors) = crate::directory::retain_directory_chain(
                        &host_root,
                        &relative_parent,
                        &diagnostic_parent,
                        Some(namespace_absolute.parent().unwrap_or(Path::new("/"))),
                        true,
                        Some(&binding_options),
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
                    let resolved_parent = crate::rooted_local_file_system::resolve_rooted_path_allow_root_with_component_policy_and_budget(
                        &working_root, parent_path, symlink_policy, true, LocalFileOperation::OpenRoot,
                        &allow_all, binding_deadline, path_budget.as_mut(),
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
                        Some(&binding_options),
                        path_budget.as_mut(),
                        symlink_policy,
                        resolver_started,
                    )?;
                    (root, anchors.unwrap_or_default())
                }
            };
            drop(parent_root);
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

    /// Reads metadata for the final target while honoring this filesystem's
    /// link policy.
    ///
    /// Returns file or directory metadata. Link rejection, missing targets,
    /// resolution errors, and native metadata failures are returned as
    /// `LocalFileError` with metadata-operation context.
    pub fn target_metadata(&self, path: &Path) -> LocalResult<LocalFileMetadata> {
        let resolver = self.resolver_for(path, LocalFileOperation::Metadata)?;
        let resolved = resolve_operation_path(&resolver, path, LocalFileOperation::Metadata)?;
        let metadata = match &self.core.namespace {
            LocalNamespace::Host => {
                let entry =
                    HostLocalFileSystem::metadata_with_policy(resolved.authority_relative(), self.symlink_policy)
                        .map_err(|error| {
                            operation_error(
                                error,
                                LocalFileOperation::Metadata,
                                resolved.namespace_absolute(),
                                None,
                                resolver.current_directory(),
                            )
                        })?;
                if entry.kind() == LocalFileKind::Symlink && !self.symlink_policy.follows() {
                    return Err(operation_error(
                        LocalFileError::new(LocalFileErrorKind::Unsupported, LocalFileOperation::Metadata)
                            .with_reason("target metadata requires following a symbolic link"),
                        LocalFileOperation::Metadata,
                        resolved.namespace_absolute(),
                        None,
                        resolver.current_directory(),
                    ));
                }
                if entry.kind() == LocalFileKind::Symlink && self.symlink_policy.follows() {
                    std::fs::metadata(resolved.authority_relative())
                        .map(|metadata| LocalFileMetadata::from_native(&metadata))
                        .map_err(|error| {
                            LocalFileError::from_io(
                                LocalFileOperation::Metadata,
                                Some(resolved.namespace_absolute().to_path_buf()),
                                None,
                                error,
                            )
                        })
                } else {
                    Ok(entry)
                }
            }
            LocalNamespace::Rooted(rooted) => {
                rooted.target_metadata(resolved.authority_relative(), self.symlink_policy)
            }
        }
        .map_err(|error| {
            operation_error(
                error,
                LocalFileOperation::Metadata,
                resolved.namespace_absolute(),
                None,
                resolver.current_directory(),
            )
        })?;
        if resolved.directory_required() && metadata.kind() != LocalFileKind::Directory {
            return Err(operation_error(
                LocalFileError::new(LocalFileErrorKind::NotDirectory, LocalFileOperation::Metadata),
                LocalFileOperation::Metadata,
                resolved.namespace_absolute(),
                None,
                resolver.current_directory(),
            ));
        }
        Ok(metadata)
    }

    /// Resolves an existing operand to its namespace coordinate.
    ///
    /// Host results are native canonical paths; Rooted results are virtual
    /// absolute paths and do not grant authority for a later operation.
    /// Returns path-policy, missing-target, or native resolution errors.
    pub fn canonicalize(&self, path: &Path) -> LocalResult<std::path::PathBuf> {
        let resolver = self.resolver_for(path, LocalFileOperation::Metadata)?;
        let resolved = resolve_operation_path(&resolver, path, LocalFileOperation::Metadata)?;
        let canonical = match &self.core.namespace {
            LocalNamespace::Host => {
                let entry =
                    HostLocalFileSystem::metadata_with_policy(resolved.authority_relative(), self.symlink_policy)
                        .map_err(|error| {
                            operation_error(
                                error,
                                LocalFileOperation::Metadata,
                                resolved.namespace_absolute(),
                                None,
                                resolver.current_directory(),
                            )
                        })?;
                if entry.kind() == LocalFileKind::Symlink && !self.symlink_policy.follows() {
                    return Err(operation_error(
                        LocalFileError::new(LocalFileErrorKind::Unsupported, LocalFileOperation::Metadata)
                            .with_reason("canonicalization requires following a symbolic link"),
                        LocalFileOperation::Metadata,
                        resolved.namespace_absolute(),
                        None,
                        resolver.current_directory(),
                    ));
                }
                std::fs::canonicalize(resolved.authority_relative()).map_err(|source| {
                    LocalFileError::from_io(
                        LocalFileOperation::Metadata,
                        Some(resolved.namespace_absolute().to_path_buf()),
                        None,
                        source,
                    )
                })
            }
            LocalNamespace::Rooted(rooted) => {
                rooted.canonicalize_virtual(resolved.authority_relative(), self.symlink_policy)
            }
        }
        .map_err(|error| {
            operation_error(
                error,
                LocalFileOperation::Metadata,
                resolved.namespace_absolute(),
                None,
                resolver.current_directory(),
            )
        })?;
        if resolved.directory_required() {
            let metadata = match &self.core.namespace {
                LocalNamespace::Host => std::fs::metadata(resolved.authority_relative())
                    .map(|metadata| LocalFileMetadata::from_native(&metadata))
                    .map_err(|error| {
                        LocalFileError::from_io(
                            LocalFileOperation::Metadata,
                            Some(resolved.namespace_absolute().to_path_buf()),
                            None,
                            error,
                        )
                    }),
                LocalNamespace::Rooted(rooted) => {
                    rooted.target_metadata(resolved.authority_relative(), self.symlink_policy)
                }
            }
            .map_err(|error| {
                operation_error(
                    error,
                    LocalFileOperation::Metadata,
                    resolved.namespace_absolute(),
                    None,
                    resolver.current_directory(),
                )
            })?;
            if metadata.kind() != LocalFileKind::Directory {
                return Err(operation_error(
                    LocalFileError::new(LocalFileErrorKind::NotDirectory, LocalFileOperation::Metadata),
                    LocalFileOperation::Metadata,
                    resolved.namespace_absolute(),
                    None,
                    resolver.current_directory(),
                ));
            }
        }
        Ok(canonical)
    }

    /// Reads a regular file completely using the filesystem's reader defaults.
    pub fn read_all(&self, path: &Path) -> LocalResult<Vec<u8>> {
        self.read_all_with_options(path, &LocalReadAllOptions::new())
    }

    /// Reads a regular file completely with an optional byte limit.
    ///
    /// Returns `ResourceLimit` when the observed length or bytes read exceed
    /// the configured maximum. Other open/read failures retain structured
    /// operation context; partial bytes are discarded.
    pub fn read_all_with_options(&self, path: &Path, options: &LocalReadAllOptions) -> LocalResult<Vec<u8>> {
        self.read_all_with_context(path, options, |bytes, _, _| Ok(bytes))
    }

    fn read_all_with_context<T>(
        &self,
        path: &Path,
        options: &LocalReadAllOptions,
        finish: impl FnOnce(Vec<u8>, &Path, Option<&Path>) -> LocalResult<T>,
    ) -> LocalResult<T> {
        let read_options = options.read_options().unwrap_or(&self.defaults.read);
        let symlink_policy = self.read_symlink_policy(read_options, options.symlink_policy());
        validate_scope_symlink_policy(self.scope(), symlink_policy, LocalFileOperation::Read, Some(path))?;
        let resolver = self.resolver_for(path, LocalFileOperation::Read)?;
        let resolved = resolve_operation_path(&resolver, path, LocalFileOperation::Read)?;
        let mut reader = self.open_resolved_reader_with_policy(
            &resolved,
            read_options,
            LocalFileOperation::Read,
            resolver.current_directory(),
            symlink_policy,
        )?;
        let maximum = options.max_bytes();
        if maximum.is_some_and(|limit| reader.metadata().len() > limit) {
            return Err(operation_error(
                LocalFileError::new(LocalFileErrorKind::ResourceLimit, LocalFileOperation::Read),
                LocalFileOperation::Read,
                resolved.namespace_absolute(),
                None,
                resolver.current_directory(),
            ));
        }
        #[cfg(test)]
        crate::observation::snapshot::run_read_hook();
        let bytes = crate::observation::reader::collect_limited(&mut reader, maximum).map_err(|error| {
            operation_error(
                error,
                LocalFileOperation::Read,
                resolved.namespace_absolute(),
                None,
                resolver.current_directory(),
            )
        })?;
        finish(bytes, resolved.namespace_absolute(), resolver.current_directory())
    }

    /// Reads a complete UTF-8 text file using the filesystem's reader defaults.
    pub fn read_text(&self, path: &Path) -> LocalResult<String> {
        self.read_text_with_options(path, &LocalReadAllOptions::new())
    }

    /// Reads a complete UTF-8 text file with an optional byte limit.
    ///
    /// Returns `InvalidEncoding` with the original UTF-8 conversion error
    /// when the complete file is not valid UTF-8.
    pub fn read_text_with_options(&self, path: &Path, options: &LocalReadAllOptions) -> LocalResult<String> {
        self.read_all_with_context(path, options, |bytes, absolute, pwd| {
            String::from_utf8(bytes).map_err(|source| {
                with_current_directory(
                    LocalFileError::from_utf8(LocalFileOperation::Read, Some(absolute.to_path_buf()), source),
                    pwd,
                )
            })
        })
    }

    /// Reads bytes while checking same-handle observations before and after.
    ///
    /// This detects observed identity, size, or native timestamp changes; it
    /// does not provide an atomic snapshot against adversarial concurrent I/O.
    /// Returns `ChangedDuringRead` when the observations do not match.
    pub fn read_consistent_with_options(
        &self,
        path: &Path,
        options: &LocalSnapshotOptions,
    ) -> LocalResult<crate::LocalFileSnapshot> {
        let symlink_policy = self.read_symlink_policy(options.read_options(), options.symlink_policy());
        validate_scope_symlink_policy(self.scope(), symlink_policy, LocalFileOperation::Read, Some(path))?;
        let resolver = self.resolver_for(path, LocalFileOperation::Read)?;
        let resolved = resolve_operation_path(&resolver, path, LocalFileOperation::Read)?;
        let reader = self.open_resolved_reader_with_policy(
            &resolved,
            options.read_options(),
            LocalFileOperation::Read,
            resolver.current_directory(),
            symlink_policy,
        )?;
        let mut observed =
            LocalObservedReader::from_reader(reader, LocalObservationRequirement::Required).map_err(|error| {
                operation_error(
                    error,
                    LocalFileOperation::Read,
                    resolved.namespace_absolute(),
                    None,
                    resolver.current_directory(),
                )
            })?;
        let before = observed.observation().clone();
        if before.metadata().len() > options.max_bytes() {
            return Err(operation_error(
                LocalFileError::new(LocalFileErrorKind::ResourceLimit, LocalFileOperation::Read),
                LocalFileOperation::Read,
                resolved.namespace_absolute(),
                None,
                resolver.current_directory(),
            ));
        }
        let bytes = observed.read_to_end_limited(options.max_bytes()).map_err(|error| {
            operation_error(
                error,
                LocalFileOperation::Read,
                resolved.namespace_absolute(),
                None,
                resolver.current_directory(),
            )
        })?;
        #[cfg(test)]
        crate::observation::snapshot::run_read_hook();
        let after = observed.observe_now().map_err(|error| {
            operation_error(
                error,
                LocalFileOperation::Read,
                resolved.namespace_absolute(),
                None,
                resolver.current_directory(),
            )
        })?;
        if !crate::observation::snapshot::observations_match(&before, &after, bytes.len() as u64) {
            return Err(operation_error(
                LocalFileError::new(LocalFileErrorKind::ChangedDuringRead, LocalFileOperation::Read),
                LocalFileOperation::Read,
                resolved.namespace_absolute(),
                None,
                resolver.current_directory(),
            ));
        }
        Ok(crate::LocalFileSnapshot::new(bytes, before, after))
    }

    /// Reads final-entry metadata without following the final symlink.
    ///
    /// Returns resolution, policy, or native metadata errors. A
    /// directory-qualified operand returns `NotDirectory` unless the observed
    /// final entry is a real directory.
    pub fn metadata(&self, path: &Path) -> LocalResult<LocalFileMetadata> {
        let resolver = self.resolver_for(path, LocalFileOperation::Metadata)?;
        let resolved = resolve_operation_path(&resolver, path, LocalFileOperation::Metadata)?;
        self.core.fail_if_requested(TestFaultPoint::Metadata).map_err(|error| {
            with_current_directory(
                LocalFileError::from_io(LocalFileOperation::Metadata, Some(path.to_path_buf()), None, error),
                resolver.current_directory(),
            )
        })?;
        let metadata = match &self.core.namespace {
            LocalNamespace::Host => {
                HostLocalFileSystem::metadata_with_policy(resolved.authority_relative(), self.symlink_policy)
            }
            LocalNamespace::Rooted(rooted) => rooted.metadata(resolved.authority_relative(), self.symlink_policy),
        }
        .map_err(|error| {
            operation_error(
                error,
                LocalFileOperation::Metadata,
                resolved.namespace_absolute(),
                None,
                resolver.current_directory(),
            )
        })?;
        if resolved.directory_required() && metadata.kind() != LocalFileKind::Directory {
            let error = LocalFileError::new(LocalFileErrorKind::NotDirectory, LocalFileOperation::Metadata)
                .with_path(resolved.namespace_absolute().to_path_buf());
            return Err(with_current_directory(error, resolver.current_directory()));
        }
        Ok(metadata)
    }

    /// Opens a reader using this instance's default reader options.
    ///
    /// Returns the owned reader or error described by
    /// [`Self::open_reader_with_options`].
    pub fn open_reader(&self, path: &Path) -> LocalResult<LocalFileReader> {
        self.open_reader_with_options(path, &self.defaults.read)
    }

    /// Opens a reader using one complete explicit options value.
    ///
    /// Requires a regular file after applying this instance's symlink policy.
    /// Returns path-resolution, type, or native open errors, including retry
    /// timeout failures. The returned reader owns the opened native handle.
    pub fn open_reader_with_options(&self, path: &Path, options: &LocalReadOptions) -> LocalResult<LocalFileReader> {
        let symlink_policy = self.read_symlink_policy(options, None);
        validate_scope_symlink_policy(self.scope(), symlink_policy, LocalFileOperation::OpenReader, Some(path))?;
        let resolver = self.resolver_for(path, LocalFileOperation::OpenReader)?;
        let resolved = resolve_operation_path(&resolver, path, LocalFileOperation::OpenReader)?;
        self.open_resolved_reader(
            &resolved,
            options,
            LocalFileOperation::OpenReader,
            resolver.current_directory(),
        )
    }

    /// Opens an already-bound operand without querying the process PWD again.
    ///
    /// Returns an opened regular-file reader or a structured opening error
    /// carrying the same namespace path and PWD snapshot as the caller.
    fn open_resolved_reader(
        &self,
        resolved: &LocalNamespacePath,
        options: &LocalReadOptions,
        operation: LocalFileOperation,
        current_directory: Option<&Path>,
    ) -> LocalResult<LocalFileReader> {
        let symlink_policy = self.read_symlink_policy(options, None);
        self.open_resolved_reader_with_policy(resolved, options, operation, current_directory, symlink_policy)
    }

    pub(crate) const fn read_symlink_policy(
        &self,
        options: &LocalReadOptions,
        outer_override: Option<LocalSymlinkPolicy>,
    ) -> LocalSymlinkPolicy {
        match outer_override {
            Some(policy) => policy,
            None => match options.symlink_policy() {
                Some(policy) => policy,
                None => self.symlink_policy,
            },
        }
    }

    fn open_resolved_reader_with_policy(
        &self,
        resolved: &LocalNamespacePath,
        options: &LocalReadOptions,
        operation: LocalFileOperation,
        current_directory: Option<&Path>,
        symlink_policy: crate::LocalSymlinkPolicy,
    ) -> LocalResult<LocalFileReader> {
        self.open_resolved_reader_with_policy_and_contract(
            resolved,
            options,
            operation,
            current_directory,
            symlink_policy,
            None,
        )
    }

    fn open_resolved_reader_with_policy_and_contract(
        &self,
        resolved: &LocalNamespacePath,
        options: &LocalReadOptions,
        operation: LocalFileOperation,
        current_directory: Option<&Path>,
        symlink_policy: crate::LocalSymlinkPolicy,
        resolution: Option<crate::resolution::ResolutionContract>,
    ) -> LocalResult<LocalFileReader> {
        validate_scope_symlink_policy(
            self.scope(),
            symlink_policy,
            operation,
            Some(resolved.namespace_absolute()),
        )?;
        reject_directory_qualified_file(resolved, operation, current_directory)?;
        match &self.core.namespace {
            LocalNamespace::Host => {
                HostLocalFileSystem::open_reader_with_policy(resolved.authority_relative(), options, symlink_policy)
            }
            LocalNamespace::Rooted(rooted) => match resolution {
                Some(contract) => {
                    rooted.open_reader_with_contract(resolved.authority_relative(), options, symlink_policy, contract)
                }
                None => rooted.open_reader(resolved.authority_relative(), options, symlink_policy),
            },
        }
        .map_err(|error| operation_error(error, operation, resolved.namespace_absolute(), None, current_directory))
    }

    /// Reads at most `max_bytes` using the default reader options.
    ///
    /// Returns the prefix or error described by
    /// [`Self::read_prefix_with_options`].
    pub fn read_prefix(&self, path: &Path, max_bytes: usize) -> LocalResult<Vec<u8>> {
        self.read_prefix_with_options(path, max_bytes, &self.defaults.read)
    }

    /// Reads at most `max_bytes` using one complete explicit options value.
    ///
    /// Stops at the byte limit or EOF and returns an owned byte vector. A zero
    /// limit still resolves and opens the source. Returns the opening errors
    /// of [`Self::open_reader_with_options`] or a subsequent read error;
    /// interrupted reads are retried and partial bytes are discarded on error.
    pub fn read_prefix_with_options(
        &self,
        path: &Path,
        max_bytes: usize,
        options: &LocalReadOptions,
    ) -> LocalResult<Vec<u8>> {
        let symlink_policy = self.read_symlink_policy(options, None);
        validate_scope_symlink_policy(self.scope(), symlink_policy, LocalFileOperation::Read, Some(path))?;
        let resolver = self.resolver_for(path, LocalFileOperation::Read)?;
        let resolved = resolve_operation_path(&resolver, path, LocalFileOperation::Read)?;
        let error_path = resolved.namespace_absolute().to_path_buf();
        let mut reader = self.open_resolved_reader(
            &resolved,
            options,
            LocalFileOperation::Read,
            resolver.current_directory(),
        )?;
        if max_bytes == 0 {
            return Ok(Vec::new());
        }
        let mut result = Vec::with_capacity(max_bytes.min(8192));
        let mut buffer = [0_u8; 8192];
        while result.len() < max_bytes {
            let read_len = (max_bytes - result.len()).min(buffer.len());
            #[cfg(feature = "test-support")]
            if take_test_support("local-fs-read-prefix-read") {
                let error = LocalFileError::from_io(
                    LocalFileOperation::Read,
                    Some(error_path),
                    None,
                    std::io::Error::other("injected prefix read failure"),
                );
                return Err(with_current_directory(error, resolver.current_directory()));
            }
            #[cfg(feature = "test-support")]
            let read_result = if take_test_support("local-prefix-interrupted") {
                Err(std::io::Error::from(std::io::ErrorKind::Interrupted))
            } else {
                Input::read(&mut reader, &mut buffer[..read_len])
            };
            #[cfg(not(feature = "test-support"))]
            let read_result = Input::read(&mut reader, &mut buffer[..read_len]);
            if matches!(&read_result, Err(error) if error.kind() == std::io::ErrorKind::Interrupted) {
                continue;
            }
            let count = read_result.map_err(|source| {
                with_current_directory(
                    LocalFileError::from_io(LocalFileOperation::Read, Some(error_path.clone()), None, source),
                    resolver.current_directory(),
                )
            })?;
            if count == 0 {
                break;
            }
            result.extend_from_slice(&buffer[..count]);
        }
        Ok(result)
    }

    /// Opens a writer using this instance's default writer options.
    ///
    /// Returns the owned writer or error described by
    /// [`Self::open_writer_with_options`].
    pub fn open_writer(&self, path: &Path) -> LocalResult<LocalFileWriter> {
        self.open_writer_with_options(path, &self.defaults.write)
    }

    /// Opens a writer using one complete explicit options value.
    ///
    /// Create modes return an unpublished staging writer; append opens an
    /// existing regular file directly. Returns path, type, requirement, or
    /// native preparation errors. Requested parent creation may remain after
    /// an error. Commit or abort a staging writer explicitly when its final
    /// publication or cleanup result must be observed.
    pub fn open_writer_with_options(&self, path: &Path, options: &LocalWriteOptions) -> LocalResult<LocalFileWriter> {
        validate_write_options(options, self.capabilities(), LocalFileOperation::OpenWriter).map_err(|error| {
            with_current_directory(
                error.with_path(path.to_path_buf()),
                self.current_directory.virtual_path(),
            )
        })?;
        let resolver = self.resolver_for(path, LocalFileOperation::OpenWriter)?;
        let resolved = resolve_operation_path(&resolver, path, LocalFileOperation::OpenWriter)?;
        self.reject_root_operand(&resolved, LocalFileOperation::OpenWriter, resolver.current_directory())?;
        reject_directory_qualified_file(&resolved, LocalFileOperation::OpenWriter, resolver.current_directory())?;
        match &self.core.namespace {
            LocalNamespace::Host => HostLocalFileSystem::open_writer_with_policy(
                resolved.authority_relative(),
                options,
                self.symlink_policy,
            ),
            LocalNamespace::Rooted(rooted) => {
                rooted.open_writer(resolved.authority_relative(), options, self.symlink_policy)
            }
        }
        .map(|writer| {
            writer.bind_namespace(
                resolved.namespace_absolute().to_path_buf(),
                resolver.current_directory().map(Path::to_path_buf),
            )
        })
        .map_err(|error| {
            operation_error(
                error,
                LocalFileOperation::OpenWriter,
                resolved.namespace_absolute(),
                None,
                resolver.current_directory(),
            )
        })
    }
}

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

#[cfg(test)]
mod tests {
    use std::path::Path;

    use super::operation_error;
    use crate::error::LocalFileError;
    use crate::error::LocalFileErrorKind;
    use crate::error::LocalFileErrorSource;
    use crate::error::LocalFileOperation;

    #[test]
    fn read_operation_context_preserves_typed_observation_source() {
        let error = LocalFileError::from_io(
            LocalFileOperation::Metadata,
            Some(Path::new("native-path").to_path_buf()),
            None,
            std::io::Error::from_raw_os_error(libc::EIO),
        );

        let wrapped = operation_error(
            error,
            LocalFileOperation::Read,
            Path::new("/sub/file"),
            None,
            Some(Path::new("/sub")),
        );

        assert_eq!(wrapped.kind(), LocalFileErrorKind::Io);
        assert_eq!(wrapped.operation(), LocalFileOperation::Read);
        assert_eq!(wrapped.path(), Some(Path::new("/sub/file")));
        assert_eq!(wrapped.current_directory(), Some(Path::new("/sub")));
        assert!(matches!(
            wrapped.typed_source(),
            Some(LocalFileErrorSource::Io(source)) if source.raw_os_error() == Some(libc::EIO)
        ));
    }
}
