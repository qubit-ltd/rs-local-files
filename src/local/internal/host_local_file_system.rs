// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

#[path = "host_local_file_system/io.rs"]
mod io_operations;
mod path_resolution;

use std::fs;
use std::io;
use std::path::Path;
use std::path::PathBuf;

pub(crate) use path_resolution::resolve_host_path;

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
use crate::LocalFileError;
use crate::LocalFileErrorKind;
use crate::LocalFileOperation;
use crate::LocalFileSystemProtocols;
use crate::LocalMetadataPreservePolicy;
use crate::LocalPaths;
use crate::LocalRenameFailure;
use crate::LocalRenameFailureState;
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

/// Host-wide native local filesystem service.
pub(crate) struct HostLocalFileSystem {
    /// Prevents construction of this stateless service type.
    _private: (),
}

impl HostLocalFileSystem {
    /// Returns the native protocols compiled for the current host platform.
    #[inline(always)]
    pub const fn protocols() -> LocalFileSystemProtocols {
        LocalFileSystemProtocols::detect_host()
    }
}

include!("host_local_file_system/copy.rs");
include!("host_local_file_system/directory.rs");
include!("host_local_file_system/temp.rs");
include!("host_local_file_system/delete.rs");
include!("host_local_file_system/rename.rs");
include!("host_local_file_system/support.rs");
