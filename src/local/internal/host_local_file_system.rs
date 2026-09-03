// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

// Implements Host copy operations.
mod copy;
// Implements Host deletion operations.
mod delete;
// Tracks Host deletion work.
mod delete_work;
// Implements Host directory operations.
mod directory;
// Implements Host metadata, reader, writer, and listing operations.
#[path = "host_local_file_system/io.rs"]
mod io_operations;
// Resolves Host paths against the process namespace.
mod path_resolution;
// Implements Host rename operations.
mod rename;
// Provides Host authority and fault-injection helpers.
mod support;
// Implements Host temporary-resource operations.
mod temp;

pub(super) use std::fs;
pub(super) use std::io;
pub(super) use std::path::Path;
pub(super) use std::path::PathBuf;

use path_resolution::bind_host_path;
use path_resolution::bind_host_paths;
pub(crate) use path_resolution::resolve_host_path;
pub(super) use rename::destination_is_directory;
pub(super) use rename::sync_parent_directory;
pub(super) use support::test_io_fault;
pub(crate) use temp::internal_copy_options;
pub(super) use temp::open_staged_writer;

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
pub(super) use crate::LocalFileError;
pub(super) use crate::LocalFileErrorKind;
pub(super) use crate::LocalFileOperation;
pub(super) use crate::LocalFileSystemCapabilities;
pub(super) use crate::LocalMetadataPreservePolicy;
pub(super) use crate::LocalRenameFailure;
pub(super) use crate::LocalRenameFailureState;
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
pub(super) use crate::local::CopyDestinationAction;
pub(super) use crate::local::copy_failure_indeterminate;
pub(super) use crate::local::copy_failure_published;
pub(super) use crate::local::copy_failure_unchanged;
pub(super) use crate::local::decide_copy_destination;
pub(super) use crate::local::ensure_required_directory_durability;
pub(super) use crate::local::published_durability;
pub(super) use crate::local::rename_failure_after_native_attempt;
pub(super) use crate::local::rename_failure_renamed;
pub(super) use crate::local::rename_failure_unchanged;
pub(super) use crate::local::validate_temp_affixes;

/// Host-wide native local filesystem service.
pub(crate) struct HostLocalFileSystem {
    /// Prevents construction of this stateless service type.
    _private: (),
}

impl HostLocalFileSystem {
    /// Returns the native capabilities compiled for the current host platform.
    #[must_use = "inspect the host capability snapshot"]
    // qubit-style: allow coverage-cfg
    #[cfg_attr(not(coverage), inline(always))]
    #[cfg_attr(coverage, inline(never))]
    pub const fn capabilities() -> LocalFileSystemCapabilities {
        LocalFileSystemCapabilities::detect_host()
    }
}
