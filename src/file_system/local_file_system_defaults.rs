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
#[derive(Clone, Default)]
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

/// Formats temporary-file defaults without exposing their parent path.
struct TempFileDebug<'a>(&'a LocalTempFileOptions);

impl std::fmt::Debug for TempFileDebug<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LocalTempFileOptions")
            .field("parent", &self.0.parent().map(crate::local_file_system::redacted_path))
            .field("prefix", &self.0.prefix())
            .field("suffix", &self.0.suffix())
            .field("max_attempts", &self.0.max_attempts())
            .field("create_parent", &self.0.creates_parent())
            .field("privacy", &self.0.privacy())
            .finish()
    }
}

/// Formats temporary-directory defaults without exposing their parent path.
struct TempDirectoryDebug<'a>(&'a LocalTempDirectoryOptions);

impl std::fmt::Debug for TempDirectoryDebug<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LocalTempDirectoryOptions")
            .field("parent", &self.0.parent().map(crate::local_file_system::redacted_path))
            .field("prefix", &self.0.prefix())
            .field("suffix", &self.0.suffix())
            .field("max_attempts", &self.0.max_attempts())
            .field("create_parent", &self.0.creates_parent())
            .field("privacy", &self.0.privacy())
            .field("cleanup_limits", &self.0.cleanup_limits())
            .field("owner_access_policy", &self.0.owner_access_policy())
            .finish()
    }
}

impl std::fmt::Debug for LocalFileSystemDefaults {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LocalFileSystemDefaults")
            .field("read", &self.read)
            .field("write", &self.write)
            .field("list", &self.list)
            .field("copy", &self.copy)
            .field("create_directory", &self.create_directory)
            .field("delete", &self.delete)
            .field("rename", &self.rename)
            .field("temp_file", &TempFileDebug(&self.temp_file))
            .field("temp_directory", &TempDirectoryDebug(&self.temp_directory))
            .finish()
    }
}
