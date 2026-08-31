// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
// qubit-style: allow multiple-public-types

mod copy;
mod delete;
mod delete_work;
mod directory;
#[path = "rooted_local_file_system/io.rs"]
mod io_operations;
#[path = "rooted_local_file_system/metadata.rs"]
mod metadata_operations;
mod path_support;
mod rename;
mod resolution_step;
mod support;
mod symlink_identity;
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
    pub fn open(path: &Path) -> LocalResult<Self> {
        let root = match crate::rooted::Root::open(path) {
            Ok(root) => root,
            Err(error) => {
                return Err(LocalFileError::from_io(
                    LocalFileOperation::OpenRoot,
                    Some(path.to_path_buf()),
                    None,
                    error,
                ));
            }
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
    #[inline]
    pub const fn capabilities(&self) -> LocalFileSystemCapabilities {
        self.capabilities
    }

    /// Returns limits observed from the opened root authority.
    #[inline]
    pub const fn limits(&self) -> LocalFileSystemLimits {
        self.limits
    }

    /// Validates that a normalized backend path resolves to a directory.
    pub(crate) fn validate_directory(&self, path: &Path, symlink_policy: LocalSymlinkPolicy) -> LocalResult<()> {
        validate_rooted_list_start(&self.root, path, symlink_policy)
    }

    /// Observes objective path limits through the nearest existing rooted
    /// entry or ancestor.
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
    fn open_nearest_probe(&self, path: &Path, symlink_policy: LocalSymlinkPolicy) -> LocalResult<std::fs::File> {
        if path.as_os_str().is_empty() {
            return match self.root.open_probe_root() {
                Ok(file) => Ok(file),
                Err(error) => Err(rooted_io_error(LocalFileOperation::Capabilities, path, error)),
            };
        }
        let resolved = resolve_rooted_path(&self.root, path, symlink_policy, true, LocalFileOperation::Capabilities)?;
        let mut candidate = resolved.as_path().to_path_buf();
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
