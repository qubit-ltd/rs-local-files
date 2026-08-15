// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Authority anchored to an opened root directory handle.

use std::env;
use std::fs::File;
#[cfg(test)]
use std::io::Read;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;

use super::AuthorityPath;
use super::symlink_resolver;
use crate::LocalFileError;
use crate::LocalFileErrorKind;
use crate::LocalFileMetadata;
use crate::LocalFileOperation;
use crate::LocalFileSystemLimits;
use crate::LocalFileSystemSpace;
use crate::LocalResult;
use crate::LocalSymlinkPolicy;
use crate::RelativePath;
use crate::platform::DirectoryCursor;
use crate::platform::EntryIdentity;
use crate::platform::NamespaceHandle;
use crate::platform::OpenedFile;
use crate::platform::StagedFile;

/// A namespace authority retained independently of its diagnostic root path.
#[derive(Debug)]
pub(crate) struct RootedAuthority {
    /// Open handle that is the sole namespace authority.
    root: Arc<NamespaceHandle>,
    /// Construction-time absolute path used only for diagnostics.
    diagnostic_root: PathBuf,
    /// Symbolic-link policy applied to descendants.
    symlink_policy: LocalSymlinkPolicy,
}

impl RootedAuthority {
    /// Opens `root` and retains its handle as the namespace authority.
    ///
    /// # Errors
    ///
    /// Returns an open-root or current-directory error when the diagnostic
    /// path cannot be formed or the directory handle cannot be opened.
    pub(crate) fn open(
        root: &Path,
        symlink_policy: LocalSymlinkPolicy,
    ) -> LocalResult<Self> {
        let diagnostic_root = absolute_diagnostic_path(root)?;
        let root = NamespaceHandle::open_root(root)?;
        Ok(Self {
            root: Arc::new(root),
            diagnostic_root,
            symlink_policy,
        })
    }

    /// Returns the construction-time root path used for diagnostics only.
    #[must_use]
    pub(crate) fn diagnostic_root(&self) -> &Path {
        &self.diagnostic_root
    }

    /// Validates a path and seals it as belonging to this Rooted authority.
    ///
    /// # Errors
    ///
    /// Returns an invalid-path error for absolute, dot, parent, prefixed, or
    /// NUL-containing paths, and rejects an across-scope symlink policy.
    pub(crate) fn resolve(&self, path: &Path) -> LocalResult<AuthorityPath> {
        let path = RelativePath::parse(path)?;
        if self.symlink_policy == LocalSymlinkPolicy::FollowAcrossScope {
            return Err(LocalFileError::new(
                LocalFileErrorKind::InvalidOptions,
                LocalFileOperation::BindPath,
            )
            .with_path(path.as_path().to_path_buf())
            .with_reason(
                "FollowAcrossScope is incompatible with Rooted authority",
            ));
        }
        symlink_resolver::resolve(&self.root, &path, self.symlink_policy)
            .map(AuthorityPath::Rooted)
    }

    /// Reads metadata through the retained root handle.
    pub(crate) fn metadata(
        &self,
        path: &AuthorityPath,
    ) -> LocalResult<LocalFileMetadata> {
        self.root.metadata(rooted_path(path)?)
    }

    pub(crate) fn resolve_metadata(
        &self,
        path: &Path,
    ) -> LocalResult<AuthorityPath> {
        let path = RelativePath::parse(path)?;
        if self.symlink_policy == LocalSymlinkPolicy::FollowAcrossScope {
            return Err(LocalFileError::new(
                LocalFileErrorKind::InvalidOptions,
                LocalFileOperation::BindPath,
            )
            .with_path(path.as_path().to_path_buf())
            .with_reason(
                "FollowAcrossScope is incompatible with Rooted authority",
            ));
        }
        symlink_resolver::resolve_parent(&self.root, &path, self.symlink_policy)
            .map(AuthorityPath::Rooted)
    }

    /// Opens a reader through the retained root handle.
    pub(crate) fn open_reader(
        &self,
        path: &AuthorityPath,
    ) -> LocalResult<OpenedFile> {
        self.root.open_reader(rooted_path(path)?)
    }

    /// Reads an entire regular file through the retained root handle.
    #[cfg(test)]
    pub(crate) fn read_all(
        &self,
        path: &AuthorityPath,
    ) -> LocalResult<Vec<u8>> {
        let mut opened = self.open_reader(path)?;
        let mut bytes = Vec::new();
        opened.read_to_end(&mut bytes).map_err(|error| {
            LocalFileError::from_io(
                LocalFileOperation::Read,
                Some(path.diagnostic_path().to_path_buf()),
                None,
                error,
            )
        })?;
        Ok(bytes)
    }

    /// Opens a lazy directory cursor through the retained root handle.
    pub(crate) fn open_directory(
        &self,
        path: &AuthorityPath,
    ) -> LocalResult<DirectoryCursor> {
        self.root.open_directory(rooted_path(path)?)
    }

    /// Creates a directory, accepting an existing directory.
    pub(crate) fn create_directory(
        &self,
        path: &AuthorityPath,
    ) -> LocalResult<()> {
        self.root.create_directory(rooted_path(path)?)
    }

    /// Creates a directory only when the final entry is absent.
    pub(crate) fn create_directory_new(
        &self,
        path: &AuthorityPath,
    ) -> LocalResult<()> {
        self.root.create_directory_new(rooted_path(path)?)
    }

    /// Creates and opens a new private regular file.
    #[allow(dead_code)]
    pub(crate) fn create_file_new(
        &self,
        path: &AuthorityPath,
    ) -> LocalResult<File> {
        self.root.create_file_new(rooted_path(path)?)
    }

    /// Deletes a file or non-directory entry without following it.
    pub(crate) fn delete_file(&self, path: &AuthorityPath) -> LocalResult<()> {
        self.root.delete_file(rooted_path(path)?)
    }

    /// Deletes an empty directory without following it.
    pub(crate) fn delete_directory(
        &self,
        path: &AuthorityPath,
    ) -> LocalResult<()> {
        self.root.delete_directory(rooted_path(path)?)
    }

    /// Renames two paths within the retained root handle.
    pub(crate) fn rename(
        &self,
        source: &AuthorityPath,
        target: &AuthorityPath,
        overwrite: bool,
    ) -> LocalResult<()> {
        self.root
            .rename(rooted_path(source)?, rooted_path(target)?, overwrite)
    }

    /// Creates a private staging file beside `target`.
    #[allow(dead_code)]
    pub(crate) fn create_staged_file(
        &self,
        target: &AuthorityPath,
    ) -> LocalResult<StagedFile> {
        self.root.create_staged_file(rooted_path(target)?)
    }

    /// Reads the native identity of an entry through the root handle.
    pub(crate) fn entry_identity(
        &self,
        path: &AuthorityPath,
    ) -> LocalResult<EntryIdentity> {
        self.root.entry_identity(rooted_path(path)?)
    }

    /// Synchronizes the directory containing `path`.
    pub(crate) fn sync_parent(&self, path: &AuthorityPath) -> LocalResult<()> {
        self.root.sync_parent(rooted_path(path)?)
    }

    /// Reads filesystem path limits through the nearest opened handle.
    pub(crate) fn filesystem_limits(
        &self,
        path: &AuthorityPath,
    ) -> LocalResult<LocalFileSystemLimits> {
        self.root.filesystem_limits(rooted_path(path)?)
    }

    /// Reads filesystem capacity through the nearest opened handle.
    pub(crate) fn filesystem_space(
        &self,
        path: &AuthorityPath,
    ) -> LocalResult<LocalFileSystemSpace> {
        self.root.filesystem_space(rooted_path(path)?)
    }
}

/// Extracts a Rooted relative path and rejects cross-authority values.
fn rooted_path(path: &AuthorityPath) -> LocalResult<&RelativePath> {
    match path {
        AuthorityPath::Rooted(path) => Ok(path),
        AuthorityPath::Host(_) => Err(authority_mismatch(path)),
    }
}

/// Creates a structured error for an AuthorityPath variant mismatch.
fn authority_mismatch(path: &AuthorityPath) -> LocalFileError {
    LocalFileError::new(
        LocalFileErrorKind::InvalidPath,
        LocalFileOperation::BindPath,
    )
    .with_path(path.diagnostic_path().to_path_buf())
    .with_reason("the path was constructed by a different authority kind")
}

/// Forms an absolute diagnostic path without canonicalizing or following it.
fn absolute_diagnostic_path(path: &Path) -> LocalResult<PathBuf> {
    if path.is_absolute() {
        return Ok(path.to_path_buf());
    }
    env::current_dir()
        .map(|cwd| cwd.join(path))
        .map_err(|error| {
            LocalFileError::from_io(
                LocalFileOperation::OpenRoot,
                Some(path.to_path_buf()),
                None,
                error,
            )
        })
}
