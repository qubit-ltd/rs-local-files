// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

// Implements rooted file and directory copy operations.
#[cfg(feature = "digest")]
mod conditional;
mod copy;
// Implements rooted deletion operations.
mod delete;
// Implements rooted directory operations.
mod directory;
#[path = "rooted_local_file_system/fixed_directory.rs"]
mod fixed_directory;
mod temp_sandbox_cleanup_error;
// Implements rooted metadata, reader, and writer operations.
#[path = "rooted_local_file_system/io.rs"]
mod io_operations;
// Implements rooted metadata conversion operations.
#[path = "rooted_local_file_system/metadata.rs"]
mod metadata_operations;
// Provides shared rooted path and I/O helpers.
mod path_support;
// Implements rooted rename operations.
mod rename;
// Describes rooted path-resolution steps.
mod resolution_step;
// Provides rooted authority and validation helpers.
mod support;
// Tracks rooted symbolic-link identity state.
mod symlink_identity;
// Implements rooted temporary-resource operations.
mod temp;

pub(super) use std::io;
pub(super) use std::path::Path;
pub(super) use std::path::PathBuf;
pub(super) use std::sync::Arc;

pub(crate) use path_support::rooted_destination_is_directory;
pub(crate) use path_support::rooted_io_error;
pub(crate) use path_support::rooted_metadata;
pub(crate) use path_support::rooted_path;
pub(crate) use path_support::rooted_temp_parent;
pub(crate) use path_support::temp_candidate;
pub(crate) use path_support::validate_rooted_temp_parent;
pub(crate) use support::resolve_rooted_path;
pub(crate) use support::resolve_rooted_path_allow_root;
pub(crate) use support::resolve_rooted_path_allow_root_with_component_policy_and_budget;
pub(crate) use support::resolve_rooted_path_allow_root_with_deadline;
pub(super) use support::sync_rooted_copy_parent_chain;
pub(super) use support::validate_rooted_list_start;

pub(super) use crate::LocalCopyFailure;
pub(super) use crate::LocalCopyMethod;
pub(super) use crate::LocalCopyOptions;
pub(super) use crate::LocalCopyOutcome;
pub(super) use crate::LocalCopyResult;
pub(super) use crate::LocalCopyStats;
pub(super) use crate::LocalCreateDirectoryOptions;
pub(super) use crate::LocalCreateDirectoryOutcome;
pub(super) use crate::LocalDeleteOptions;
pub(super) use crate::LocalDeleteOutcome;
pub(super) use crate::LocalDirectoryWalker;
pub(super) use crate::LocalFileError;
pub(super) use crate::LocalFileErrorKind;
pub(super) use crate::LocalFileOperation;
pub(super) use crate::LocalFileReader;
pub(super) use crate::LocalFileSystemCapabilities;
pub(super) use crate::LocalFileSystemLimits;
pub(super) use crate::LocalFileWriter;
pub(super) use crate::LocalListOptions;
pub(super) use crate::LocalReadOptions;
pub(super) use crate::LocalRenameOptions;
pub(super) use crate::LocalRenameOutcome;
pub(super) use crate::LocalRenameResult;
pub(super) use crate::LocalResult;
pub(super) use crate::LocalSymlinkPolicy;
pub(super) use crate::LocalTempDirectory;
pub(super) use crate::LocalTempDirectoryOptions;
pub(super) use crate::LocalTempFile;
pub(super) use crate::LocalTempFileOptions;
pub(super) use crate::LocalWriteMode;
pub(super) use crate::LocalWriteOptions;
pub(super) use crate::local::copy_failure_published;
pub(super) use crate::local::copy_failure_unchanged;
pub(super) use crate::local::ensure_required_directory_durability;
pub(super) use crate::local::published_durability;
pub(super) use crate::local::rename_failure_after_native_attempt;
pub(super) use crate::local::rename_failure_renamed;
pub(super) use crate::local::rename_failure_unchanged;
pub(super) use crate::local::validate_temp_affixes;

/// Descriptor- or handle-relative authority for one opened native directory.
#[derive(Clone, Debug)]
pub(crate) struct RootedLocalFileSystem {
    /// The single opened descriptor or handle authority.
    root: Arc<crate::rooted::Root>,
    /// Capability snapshot cached when the authority is opened.
    capabilities: LocalFileSystemCapabilities,
    /// Best-effort path limits captured from the opened root authority.
    limits: LocalFileSystemLimits,
}

impl RootedLocalFileSystem {
    /// Opens a native root descriptor or handle.
    ///
    /// # Parameters
    ///
    /// - `path`: Native directory path to anchor.
    ///
    /// # Returns
    ///
    /// A rooted authority whose later operations do not rely on path lookup.
    ///
    /// # Errors
    ///
    /// Returns `LocalFileError` when the directory cannot be bound and opened
    /// securely or the current platform lacks rooted primitives.
    #[allow(dead_code)] // Preserved as the public unrestricted-opening API.
    pub fn open(path: &Path) -> LocalResult<Self> {
        Self::open_with_options(path, &crate::options::LocalRootOpenOptions::new())
    }

    pub(crate) fn open_with_options(
        path: &Path,
        options: &crate::options::LocalRootOpenOptions,
    ) -> LocalResult<Self> {
        let mut contract = crate::resolution::ResolutionContract::unrestricted()
            .tightened(options.limits());
        if let Some(policy) = options.resolution_policy() {
            contract = contract.with_policy(policy.clone());
        }
        Self::open_with_contract(
            path,
            contract,
            options.symlink_policy().unwrap_or(LocalSymlinkPolicy::FollowWithinScope),
        )
    }

    pub(crate) fn open_with_contract(
        path: &Path,
        contract: crate::resolution::ResolutionContract,
        policy: LocalSymlinkPolicy,
    ) -> LocalResult<Self> {
        if policy == LocalSymlinkPolicy::FollowAcrossScope {
            return Err(LocalFileError::new(LocalFileErrorKind::InvalidOptions, LocalFileOperation::OpenRoot)
                .with_path(path.to_path_buf()));
        }
        #[cfg(unix)]
        let root = {
            let absolute = std::path::absolute(path).map_err(|source| LocalFileError::from_io(
                LocalFileOperation::OpenRoot, Some(path.to_path_buf()), None, source,
            ))?;
            let host = crate::rooted::Root::open(Path::new("/"))
                .map_err(|source| LocalFileError::from_io(LocalFileOperation::OpenRoot, Some(path.to_path_buf()), None, source))?
                .with_resolution_contract(contract.clone());
            let relative = absolute.strip_prefix("/").unwrap_or(&absolute);
            let resolved = support::resolve_rooted_path_allow_root_with_scope(
                &host, relative, policy, true, LocalFileOperation::OpenRoot,
                crate::directory::LocalResolutionScope::OpeningRoot,
            )?;
            let mut cursor = host.resolution_cursor().map_err(|source| LocalFileError::from_io(
                LocalFileOperation::OpenRoot, Some(path.to_path_buf()), None, source,
            ))?;
            for component in resolved.components() {
                cursor.descend(component.as_os_str()).map_err(|source| LocalFileError::from_io(
                    LocalFileOperation::OpenRoot, Some(path.to_path_buf()), None, source,
                ))?;
            }
            crate::rooted::Root::from_directory_authority_with_contract(
                cursor.into_file(), absolute, contract,
            )
        };
        #[cfg(windows)]
        let root = {
            use std::path::Component;
            let absolute = std::path::absolute(path).map_err(|source| LocalFileError::from_io(
                LocalFileOperation::OpenRoot, Some(path.to_path_buf()), None, source,
            ))?;
            let mut components = absolute.components();
            let prefix = match components.next() {
                Some(Component::Prefix(prefix)) => prefix,
                _ => return Err(LocalFileError::new(LocalFileErrorKind::InvalidPath, LocalFileOperation::OpenRoot)
                    .with_path(path.to_path_buf())),
            };
            let mut anchor = PathBuf::new();
            anchor.push(prefix.as_os_str());
            if !matches!(components.next(), Some(Component::RootDir)) {
                return Err(LocalFileError::new(LocalFileErrorKind::InvalidPath, LocalFileOperation::OpenRoot)
                    .with_path(path.to_path_buf()));
            }
            anchor.push(std::path::MAIN_SEPARATOR_STR);
            let host = crate::rooted::Root::open(&anchor)
                .map_err(|source| LocalFileError::from_io(LocalFileOperation::OpenRoot, Some(path.to_path_buf()), None, source))?
                .with_resolution_contract(contract.clone());
            let relative = absolute.strip_prefix(&anchor).map_err(|_| {
                LocalFileError::new(LocalFileErrorKind::InvalidPath, LocalFileOperation::OpenRoot)
                    .with_path(path.to_path_buf())
            })?;
            let resolved = support::resolve_rooted_path_allow_root_with_scope(
                &host,
                relative,
                policy,
                true,
                LocalFileOperation::OpenRoot,
                crate::directory::LocalResolutionScope::OpeningRoot,
            )?;
            let mut cursor = host.resolution_cursor().map_err(|source| LocalFileError::from_io(
                LocalFileOperation::OpenRoot, Some(path.to_path_buf()), None, source,
            ))?;
            for component in resolved.components() {
                cursor.descend(component.as_os_str()).map_err(|source| LocalFileError::from_io(
                    LocalFileOperation::OpenRoot, Some(path.to_path_buf()), None, source,
                ))?;
            }
            crate::rooted::Root::from_directory_authority_with_contract(cursor.into_file(), absolute, contract)
        };
        #[cfg(not(any(unix, windows)))]
        let root = {
            let root = crate::rooted::Root::open(path).map_err(|source| {
                LocalFileError::from_io(LocalFileOperation::OpenRoot, Some(path.to_path_buf()), None, source)
            })?;
            root.with_resolution_contract(contract)
        };
        let root = Arc::new(root);
        let limits = match root
            .try_clone_authority()
            .and_then(|file| crate::capability::probe_limits(&file))
        {
            Ok(limits) => limits,
            Err(_) => LocalFileSystemLimits::new(
                crate::SizeLimit::Unknown,
                crate::SizeLimit::Unknown,
                crate::LocalPathLengthUnit::native(),
            ),
        };
        Ok(Self {
            root,
            capabilities: LocalFileSystemCapabilities::detect_rooted(),
            limits,
        })
    }

    /// Returns the non-authoritative diagnostic path captured at open time.
    #[must_use]
    #[inline]
    pub fn diagnostic_path(&self) -> &Path {
        self.root.path()
    }

    /// Returns the native capability snapshot cached for this opened authority.
    #[must_use = "inspect the rooted capability snapshot"]
    #[inline]
    pub const fn capabilities(&self) -> LocalFileSystemCapabilities {
        self.capabilities
    }

    /// Returns limits observed from the opened root authority.
    #[must_use = "inspect the rooted path limits"]
    #[inline]
    pub const fn limits(&self) -> LocalFileSystemLimits {
        self.limits
    }

    /// Validates that a normalized backend path resolves to a directory.
    /// Propagates path/policy and native inspection errors; a resolved
    /// non-directory returns a structured type-conflict error.
    pub(crate) fn validate_directory(&self, path: &Path, symlink_policy: LocalSymlinkPolicy) -> LocalResult<()> {
        validate_rooted_list_start(&self.root, path, symlink_policy, None).map(|_| ())
    }

    /// Observes objective path limits through the nearest existing rooted
    /// entry or ancestor.
    /// Propagates path/policy, native open, and capability-probe errors. A
    /// successfully probed but unavailable limit is represented as unknown.
    pub(crate) fn limits_at(
        &self,
        path: &Path,
        symlink_policy: LocalSymlinkPolicy,
    ) -> LocalResult<LocalFileSystemLimits> {
        let file = self.open_nearest_probe(path, symlink_policy)?;
        match crate::capability::probe_limits(&file) {
            Ok(limits) => Ok(limits),
            Err(error) => Err(rooted_io_error(LocalFileOperation::Capabilities, path, error)),
        }
    }

    /// Observes dynamic capacity through the nearest existing rooted entry or
    /// ancestor.
    /// Propagates path/policy, native open, and capacity-probe errors. The
    /// returned observation does not reserve space for a later operation.
    pub(crate) fn space_at(
        &self,
        path: &Path,
        symlink_policy: LocalSymlinkPolicy,
    ) -> LocalResult<crate::LocalFileSystemSpace> {
        let file = self.open_nearest_probe(path, symlink_policy)?;
        match crate::capability::probe_space(&file) {
            Ok(space) => Ok(space),
            Err(error) => Err(rooted_io_error(LocalFileOperation::Capabilities, path, error)),
        }
    }

    /// Opens the nearest existing entry for a capability probe.
    /// Falls back to parents only for `NotFound`. Returns other path/policy or
    /// native errors, including rejection of non-file/non-directory entries.
    fn open_nearest_probe(&self, path: &Path, symlink_policy: LocalSymlinkPolicy) -> LocalResult<std::fs::File> {
        if path.as_os_str().is_empty() {
            return match self.root.open_probe_root() {
                Ok(file) => Ok(file),
                Err(error) => Err(rooted_io_error(LocalFileOperation::Capabilities, path, error)),
            };
        }
        let mut candidate =
            resolve_rooted_path_allow_root(&self.root, path, symlink_policy, true, LocalFileOperation::Capabilities)?;
        loop {
            if candidate.as_os_str().is_empty() {
                return match self.root.open_probe_root() {
                    Ok(file) => Ok(file),
                    Err(error) => Err(rooted_io_error(LocalFileOperation::Capabilities, path, error)),
                };
            }
            let candidate_path = rooted_path(&candidate, LocalFileOperation::Capabilities)?;
            match self.root.open_probe_file(&candidate_path) {
                Ok(file) => return Ok(file),
                Err(error) if error.kind() == io::ErrorKind::NotFound => {
                    if !candidate.pop() {
                        return match self.root.open_probe_root() {
                            Ok(file) => Ok(file),
                            Err(error) => Err(rooted_io_error(LocalFileOperation::Capabilities, path, error)),
                        };
                    }
                }
                Err(error) => {
                    return Err(rooted_io_error(LocalFileOperation::Capabilities, path, error));
                }
            }
        }
    }
}
