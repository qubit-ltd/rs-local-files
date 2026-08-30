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
mod directory;
#[path = "rooted_local_file_system/io.rs"]
mod io_operations;
#[path = "rooted_local_file_system/metadata.rs"]
mod metadata_operations;
mod path_support;
mod rename;
mod support;
mod temp;

pub(super) use std::fs;
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
pub(super) use crate::LocalFileSystemLimits;
pub(super) use crate::LocalFileSystemProtocols;
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
    /// New handle-bound authority used by migrated operations.
    authority: Arc<crate::authority::Authority>,
    /// Existing secure rooted implementation.
    root: Arc<crate::rooted::Root>,
    /// Capability snapshot cached when the authority is opened.
    capabilities: LocalFileSystemProtocols,
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
        if std::fs::metadata(path).is_ok_and(|metadata| !metadata.is_dir()) {
            return Err(
                LocalFileError::new(LocalFileErrorKind::NotDirectory, LocalFileOperation::OpenRoot)
                    .with_path(path.to_path_buf()),
            );
        }
        let authority = crate::authority::RootedAuthority::open(path, LocalSymlinkPolicy::FollowWithinScope)?;
        let root = crate::rooted::Root::open(path).map_err(|error| {
            LocalFileError::from_io(LocalFileOperation::OpenRoot, Some(path.to_path_buf()), None, error)
        })?;
        let root = Arc::new(root);
        let limits = root
            .try_clone_authority()
            .map(|file| crate::capability::probe_limits(&file))
            .unwrap_or_else(|_| LocalFileSystemLimits::new(crate::SizeLimit::Unknown, crate::SizeLimit::Unknown));
        Ok(Self {
            authority: Arc::new(crate::authority::Authority::Rooted(authority)),
            root,
            capabilities: LocalFileSystemProtocols::detect_rooted(),
            limits,
        })
    }

    /// Returns the handle-bound authority used by migrated operations.
    #[must_use]
    #[inline(always)]
    pub(crate) fn authority(&self) -> &crate::authority::Authority {
        &self.authority
    }

    /// Returns the non-authoritative diagnostic path captured at open time.
    #[must_use]
    #[inline(always)]
    pub fn diagnostic_path(&self) -> &Path {
        self.root.path()
    }

    /// Returns the native protocol snapshot cached for this opened authority.
    #[inline(always)]
    pub const fn protocols(&self) -> LocalFileSystemProtocols {
        self.capabilities
    }

    /// Returns limits observed from the opened root authority.
    #[inline(always)]
    pub const fn limits(&self) -> LocalFileSystemLimits {
        self.limits
    }
}
