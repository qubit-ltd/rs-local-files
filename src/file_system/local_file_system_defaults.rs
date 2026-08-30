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

/// Options copied independently by [`crate::LocalFileSystem::clone`].
#[derive(Clone, Debug, Default)]
pub(crate) struct LocalFileSystemDefaults {
    pub(crate) read: LocalReadOptions,
    pub(crate) write: LocalWriteOptions,
    pub(crate) list: LocalListOptions,
    pub(crate) copy: LocalCopyOptions,
    pub(crate) create_directory: LocalCreateDirectoryOptions,
    pub(crate) delete: LocalDeleteOptions,
    pub(crate) rename: LocalRenameOptions,
    pub(crate) temp_file: LocalTempFileOptions,
    pub(crate) temp_directory: LocalTempDirectoryOptions,
}
