// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
// qubit-style: allow multiple-public-types

#[path = "rooted_local_file_system/metadata.rs"]
mod metadata_operations;
mod path_support;

use std::fs;
use std::fs::File;
use std::io;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;

use path_support::rooted_destination_is_directory;
use path_support::rooted_io_error;
pub(crate) use path_support::rooted_metadata;
use path_support::rooted_path;
use path_support::rooted_temp_parent;
use path_support::temp_candidate;
use path_support::validate_rooted_temp_parent;

use crate::LocalCopyFailure;
use crate::LocalCopyMethod;
use crate::LocalCopyOptions;
use crate::LocalCopyOutcome;
use crate::LocalCopyResult;
use crate::LocalCopyStats;
use crate::LocalCreateDirectoryOptions;
use crate::LocalCreateDirectoryOutcome;
use crate::LocalDeleteOptions;
use crate::LocalDeleteOutcome;
use crate::LocalDirectoryWalker;
use crate::LocalFileError;
use crate::LocalFileErrorKind;
use crate::LocalFileOperation;
use crate::LocalFileReader;
use crate::LocalFileSystemLimits;
use crate::LocalFileSystemProtocols;
use crate::LocalFileWriter;
use crate::LocalListOptions;
use crate::LocalReadOptions;
use crate::LocalRenameOptions;
use crate::LocalRenameOutcome;
use crate::LocalRenameResult;
use crate::LocalResult;
use crate::LocalSymlinkPolicy;
use crate::LocalTempDirectory;
use crate::LocalTempDirectoryOptions;
use crate::LocalTempFile;
use crate::LocalTempFileOptions;
use crate::LocalWriteMode;
use crate::LocalWriteOptions;
use crate::local::copy_failure_published;
use crate::local::copy_failure_unchanged;
use crate::local::ensure_required_directory_durability;
use crate::local::published_durability;
use crate::local::rename_failure_after_native_attempt;
use crate::local::rename_failure_renamed;
use crate::local::rename_failure_unchanged;
use crate::local::validate_temp_affixes;

/// Descriptor- or handle-relative authority for one opened native directory.
#[derive(Clone, Debug)]
pub(crate) struct RootedLocalFileSystem {
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
            return Err(LocalFileError::new(
                LocalFileErrorKind::NotDirectory,
                LocalFileOperation::OpenRoot,
            )
            .with_path(path.to_path_buf()));
        }
        let root = crate::rooted::Root::open(path).map_err(|error| {
            LocalFileError::from_io(
                LocalFileOperation::OpenRoot,
                Some(path.to_path_buf()),
                None,
                error,
            )
        })?;
        let root = Arc::new(root);
        let limits = root
            .try_clone_authority()
            .map(|file| crate::capability::probe_limits(&file))
            .unwrap_or_else(|_| {
                LocalFileSystemLimits::new(crate::SizeLimit::Unknown, crate::SizeLimit::Unknown)
            });
        Ok(Self {
            root,
            capabilities: LocalFileSystemProtocols::detect_rooted(),
            limits,
        })
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

include!("rooted_local_file_system/temp.rs");
include!("rooted_local_file_system/io.rs");
include!("rooted_local_file_system/directory.rs");
include!("rooted_local_file_system/copy.rs");
include!("rooted_local_file_system/delete.rs");
include!("rooted_local_file_system/rename.rs");
include!("rooted_local_file_system/support.rs");
