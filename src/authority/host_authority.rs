// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Host authority with a construction-time current-directory binding.

use std::env;
use std::fs::File;
#[cfg(test)]
use std::io::Read;
use std::path::Component;
use std::path::Path;
use std::path::PathBuf;

use super::AuthorityPath;
use super::HostPath;
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

/// A Host namespace authority with a stable relative-path binding.
#[derive(Debug)]
pub(crate) struct HostAuthority {
    /// Handle for the cwd captured at construction.
    cwd: NamespaceHandle,
    /// Construction-time cwd retained only for diagnostics.
    diagnostic_cwd: PathBuf,
    /// Symbolic-link policy applied to Host paths.
    symlink_policy: LocalSymlinkPolicy,
}

/// Namespace handle and relative path selected for one Host operation.
struct NamespacePath<'authority> {
    /// Borrowed bound handle or an owned native-root handle.
    namespace: NamespaceSelection<'authority>,
    /// Validated path interpreted relative to `namespace`.
    relative: RelativePath,
}

/// Retains either the bound cwd or a temporary native-root handle.
enum NamespaceSelection<'authority> {
    /// Construction-time cwd handle.
    Bound(&'authority NamespaceHandle),
    /// Native root opened for an absolute Host path.
    NativeRoot(NamespaceHandle),
}

impl NamespaceSelection<'_> {
    /// Returns the selected namespace handle.
    fn handle(&self) -> &NamespaceHandle {
        match self {
            Self::Bound(handle) => handle,
            Self::NativeRoot(handle) => handle,
        }
    }
}

impl HostAuthority {
    /// Captures the current directory path and opens its stable handle.
    ///
    /// # Errors
    ///
    /// Returns a bind-path or open-root error when the current directory
    /// cannot be read or opened.
    pub(crate) fn bind_current(
        symlink_policy: LocalSymlinkPolicy,
    ) -> LocalResult<Self> {
        let diagnostic_cwd = env::current_dir().map_err(|error| {
            LocalFileError::from_io(
                LocalFileOperation::BindPath,
                None,
                None,
                error,
            )
        })?;
        let cwd = NamespaceHandle::open_root(&diagnostic_cwd)?;
        Ok(Self {
            cwd,
            diagnostic_cwd,
            symlink_policy,
        })
    }

    /// Validates and seals a Host path without re-reading the process cwd.
    ///
    /// # Errors
    ///
    /// Returns an invalid-path error for unsafe relative components or an
    /// unsupported Host prefix.
    pub(crate) fn resolve(&self, path: &Path) -> LocalResult<AuthorityPath> {
        if path.is_absolute() {
            let (root, relative) = split_absolute_path(path)?;
            let namespace = NamespaceHandle::open_root(&root)?;
            let relative = symlink_resolver::resolve(
                &namespace,
                &relative,
                self.symlink_policy,
            )?;
            Ok(AuthorityPath::Host(HostPath::Absolute(
                root.join(relative.as_path()),
            )))
        } else {
            let path = RelativePath::parse(path)?;
            symlink_resolver::resolve(&self.cwd, &path, self.symlink_policy)
                .map_err(|error| {
                    error.with_path(self.diagnostic_cwd.join(path.as_path()))
                })
                .map(HostPath::BoundCwd)
                .map(AuthorityPath::Host)
        }
    }

    /// Reads metadata through the selected Host namespace handle.
    pub(crate) fn metadata(
        &self,
        path: &AuthorityPath,
    ) -> LocalResult<LocalFileMetadata> {
        let path = self.namespace_path(path)?;
        path.namespace.handle().metadata(&path.relative)
    }

    pub(crate) fn resolve_metadata(
        &self,
        path: &Path,
    ) -> LocalResult<AuthorityPath> {
        if path.is_absolute() {
            let (root, relative) = split_absolute_path(path)?;
            let namespace = NamespaceHandle::open_root(&root)?;
            let relative = symlink_resolver::resolve_parent(
                &namespace,
                &relative,
                self.symlink_policy,
            )?;
            Ok(AuthorityPath::Host(HostPath::Absolute(
                root.join(relative.as_path()),
            )))
        } else {
            let path = RelativePath::parse(path)?;
            symlink_resolver::resolve_parent(
                &self.cwd,
                &path,
                self.symlink_policy,
            )
            .map_err(|error| {
                error.with_path(self.diagnostic_cwd.join(path.as_path()))
            })
            .map(HostPath::BoundCwd)
            .map(AuthorityPath::Host)
        }
    }

    /// Opens a regular-file reader through the selected Host handle.
    pub(crate) fn open_reader(
        &self,
        path: &AuthorityPath,
    ) -> LocalResult<OpenedFile> {
        let path = self.namespace_path(path)?;
        path.namespace.handle().open_reader(&path.relative)
    }

    /// Reads an entire regular file through the selected Host handle.
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

    /// Opens a lazy directory cursor through the selected Host handle.
    pub(crate) fn open_directory(
        &self,
        path: &AuthorityPath,
    ) -> LocalResult<DirectoryCursor> {
        let path = self.namespace_path(path)?;
        path.namespace.handle().open_directory(&path.relative)
    }

    /// Creates a directory, accepting an existing directory.
    pub(crate) fn create_directory(
        &self,
        path: &AuthorityPath,
    ) -> LocalResult<()> {
        let path = self.namespace_path(path)?;
        path.namespace.handle().create_directory(&path.relative)
    }

    /// Creates a directory only when the final entry is absent.
    pub(crate) fn create_directory_new(
        &self,
        path: &AuthorityPath,
    ) -> LocalResult<()> {
        let path = self.namespace_path(path)?;
        path.namespace.handle().create_directory_new(&path.relative)
    }

    /// Creates and opens a new private regular file.
    #[allow(dead_code)]
    pub(crate) fn create_file_new(
        &self,
        path: &AuthorityPath,
    ) -> LocalResult<File> {
        let path = self.namespace_path(path)?;
        path.namespace.handle().create_file_new(&path.relative)
    }

    /// Deletes a file or non-directory entry without following it.
    pub(crate) fn delete_file(&self, path: &AuthorityPath) -> LocalResult<()> {
        let path = self.namespace_path(path)?;
        path.namespace.handle().delete_file(&path.relative)
    }

    /// Deletes an empty directory without following it.
    pub(crate) fn delete_directory(
        &self,
        path: &AuthorityPath,
    ) -> LocalResult<()> {
        let path = self.namespace_path(path)?;
        path.namespace.handle().delete_directory(&path.relative)
    }

    /// Renames two paths when both use one Host namespace handle.
    ///
    /// # Errors
    ///
    /// Returns an invalid-path error for mixed bound/absolute paths or
    /// absolute paths rooted in different native namespaces.
    pub(crate) fn rename(
        &self,
        source: &AuthorityPath,
        target: &AuthorityPath,
        overwrite: bool,
    ) -> LocalResult<()> {
        let source = self.namespace_path(source)?;
        let target = self.namespace_path(target)?;
        source.namespace.handle().rename_to(
            &source.relative,
            target.namespace.handle(),
            &target.relative,
            overwrite,
        )
    }

    /// Creates a private staging file beside `target`.
    #[allow(dead_code)]
    pub(crate) fn create_staged_file(
        &self,
        target: &AuthorityPath,
    ) -> LocalResult<StagedFile> {
        let target = self.namespace_path(target)?;
        target
            .namespace
            .handle()
            .create_staged_file(&target.relative)
    }

    /// Reads the native identity of an entry through its selected handle.
    pub(crate) fn entry_identity(
        &self,
        path: &AuthorityPath,
    ) -> LocalResult<EntryIdentity> {
        let path = self.namespace_path(path)?;
        path.namespace.handle().entry_identity(&path.relative)
    }

    /// Synchronizes the directory containing `path`.
    pub(crate) fn sync_parent(&self, path: &AuthorityPath) -> LocalResult<()> {
        let path = self.namespace_path(path)?;
        path.namespace.handle().sync_parent(&path.relative)
    }

    /// Reads filesystem path limits through the nearest selected handle.
    pub(crate) fn filesystem_limits(
        &self,
        path: &AuthorityPath,
    ) -> LocalResult<LocalFileSystemLimits> {
        let path = self.namespace_path(path)?;
        path.namespace.handle().filesystem_limits(&path.relative)
    }

    /// Reads filesystem capacity through the nearest selected handle.
    pub(crate) fn filesystem_space(
        &self,
        path: &AuthorityPath,
    ) -> LocalResult<LocalFileSystemSpace> {
        let path = self.namespace_path(path)?;
        path.namespace.handle().filesystem_space(&path.relative)
    }

    /// Selects the bound cwd or opens the native root for an absolute path.
    fn namespace_path<'authority>(
        &'authority self,
        path: &AuthorityPath,
    ) -> LocalResult<NamespacePath<'authority>> {
        match host_path(path)? {
            HostPath::BoundCwd(relative) => Ok(NamespacePath {
                namespace: NamespaceSelection::Bound(&self.cwd),
                relative: relative.clone(),
            }),
            HostPath::Absolute(path) => {
                let (root, relative) = split_absolute_path(path)?;
                Ok(NamespacePath {
                    namespace: NamespaceSelection::NativeRoot(
                        NamespaceHandle::open_root(&root)?,
                    ),
                    relative,
                })
            }
        }
    }
}

/// Extracts a Host path and rejects cross-authority values.
fn host_path(path: &AuthorityPath) -> LocalResult<&HostPath> {
    match path {
        AuthorityPath::Host(path) => Ok(path),
        AuthorityPath::Rooted(_) => {
            Err(authority_mismatch(path.diagnostic_path()))
        }
    }
}

/// Splits an absolute path into its native root and validated relative suffix.
fn split_absolute_path(path: &Path) -> LocalResult<(PathBuf, RelativePath)> {
    if !path.is_absolute() {
        return Err(authority_mismatch(path));
    }
    let mut root = PathBuf::new();
    let mut relative = PathBuf::new();
    let mut in_relative = false;
    for component in path.components() {
        match component {
            Component::Prefix(_) | Component::RootDir if !in_relative => {
                root.push(component.as_os_str());
            }
            Component::Normal(_) => {
                in_relative = true;
                relative.push(component.as_os_str());
            }
            Component::CurDir | Component::ParentDir => {
                return Err(authority_mismatch(path));
            }
            Component::Prefix(_) | Component::RootDir => {
                return Err(authority_mismatch(path));
            }
        }
    }
    if root.as_os_str().is_empty() {
        return Err(authority_mismatch(path));
    }
    RelativePath::parse(&relative).map(|relative| (root, relative))
}

/// Creates a structured invalid-path error for a Host authority mismatch.
fn authority_mismatch(path: &Path) -> LocalFileError {
    LocalFileError::new(
        LocalFileErrorKind::InvalidPath,
        LocalFileOperation::BindPath,
    )
    .with_path(path.to_path_buf())
    .with_reason("the path cannot be represented by one Host authority")
}
