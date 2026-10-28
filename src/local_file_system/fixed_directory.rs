// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;

use super::LocalCurrentDirectory;
use super::LocalFileSystem;
use super::LocalFileSystemCore;
use super::LocalFileSystemDefaults;
use super::LocalNamespace;
use crate::LocalResult;
use crate::directory::LocalDirectoryHandle;
use crate::options::LocalDirectoryOpenOptions;

impl LocalFileSystem {
    pub(crate) fn from_root_authority(
        root: crate::rooted::Root,
        symlink_policy: crate::LocalSymlinkPolicy,
    ) -> LocalResult<Self> {
        let rooted = crate::rooted_local_file_system::RootedLocalFileSystem::from_root(root)?;
        let capabilities = rooted.capabilities();
        let limits = rooted.limits();
        let resolution = rooted.root_authority().resolution_contract().clone();
        Ok(Self {
            core: Arc::new(LocalFileSystemCore {
                namespace: LocalNamespace::Rooted(rooted),
                capabilities,
                limits,
                resolution,
                #[cfg(feature = "test-support")]
                test_faults: None,
                #[cfg(all(feature = "digest", feature = "test-support"))]
                conditional_hooks: None,
            }),
            current_directory: LocalCurrentDirectory::Virtual(PathBuf::from(std::path::MAIN_SEPARATOR_STR)),
            symlink_policy,
            defaults: LocalFileSystemDefaults::default(),
        })
    }

    /// Opens a fixed directory capability from this filesystem namespace.
    ///
    /// The returned handle retains the opened directory authority, so later
    /// relative operations stay attached if its pathname is renamed.
    ///
    /// On Windows Host scope, a non-allow-all component policy is rejected
    /// because expanded names behind reparse points cannot be inspected safely
    /// during the initial opening path. Operations through an allow-all handle
    /// retain their rooted authority.
    /// Windows Host scope also rejects path binding until the path can be
    /// anchored from its drive root and all ancestors retained.
    ///
    /// # Errors
    /// Returns path, policy, type, native I/O, or resource-limit errors.
    pub fn open_directory_with_options(
        &self,
        path: &Path,
        options: &LocalDirectoryOpenOptions,
    ) -> LocalResult<LocalDirectoryHandle> {
        LocalDirectoryHandle::open(self, path, options)
    }
}
