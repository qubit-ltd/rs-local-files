// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Per-instance operation defaults.

use crate::LocalCopyOptions;
use crate::LocalCreateDirectoryOptions;
use crate::LocalDeleteOptions;
use crate::LocalListOptions;
use crate::LocalReadOptions;
use crate::LocalRenameOptions;
use crate::LocalTempDirectoryOptions;
use crate::LocalTempFileOptions;
use crate::LocalWriteOptions;

/// Replaceable convenience configuration, not mandatory resource ceilings.
///
/// Explicit per-operation Options replace these values completely.
/// Options copied independently by [`crate::LocalFileSystem::clone`].
#[derive(Clone, Debug, Default)]
pub(crate) struct LocalFileSystemDefaults {
    /// Defaults used by reader-opening convenience methods.
    pub(crate) read: LocalReadOptions,
    /// Defaults used by writer-opening convenience methods.
    pub(crate) write: LocalWriteOptions,
    /// Defaults used by directory-listing convenience methods.
    pub(crate) list: LocalListOptions,
    /// Defaults used by file and tree copy convenience methods.
    pub(crate) copy: LocalCopyOptions,
    /// Defaults used by directory-creation convenience methods.
    pub(crate) create_directory: LocalCreateDirectoryOptions,
    /// Defaults used by file and directory deletion convenience methods.
    pub(crate) delete: LocalDeleteOptions,
    /// Defaults used by rename convenience methods.
    pub(crate) rename: LocalRenameOptions,
    /// Defaults used by temporary-file convenience methods.
    pub(crate) temp_file: LocalTempFileOptions,
    /// Defaults used by temporary-directory convenience methods.
    pub(crate) temp_directory: LocalTempDirectoryOptions,
}
