// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Exhaustive dispatch across Host and Rooted authorities.

use std::fs::File;
use std::path::Path;

use super::AuthorityPath;
use super::HostAuthority;
use super::RootedAuthority;
use crate::LocalFileErrorKind;
use crate::LocalFileMetadata;
use crate::LocalFileSystemLimits;
use crate::LocalFileSystemScope;
use crate::LocalFileSystemSpace;
use crate::LocalResult;
use crate::platform::DirectoryCursor;
use crate::platform::EntryIdentity;
use crate::platform::OpenedFile;
use crate::platform::StagedFile;

/// The concrete namespace authority used by a local filesystem instance.
#[derive(Debug)]
pub(crate) enum Authority {
    /// Host-wide authority with a bound cwd handle.
    Host(HostAuthority),
    /// Authority confined beneath one opened root handle.
    Rooted(RootedAuthority),
}

impl Authority {
    /// Returns the namespace scope represented by this authority.
    pub(crate) const fn scope(&self) -> LocalFileSystemScope {
        match self {
            Self::Host(_) => LocalFileSystemScope::Host,
            Self::Rooted(_) => LocalFileSystemScope::Rooted,
        }
    }

    /// Returns the non-authoritative Rooted diagnostic path, when present.
    #[must_use]
    pub(crate) fn diagnostic_root(&self) -> Option<&Path> {
        match self {
            Self::Host(_) => None,
            Self::Rooted(authority) => Some(authority.diagnostic_root()),
        }
    }

    /// Validates and seals a path for this authority.
    pub(crate) fn resolve(&self, path: &Path) -> LocalResult<AuthorityPath> {
        match self {
            Self::Host(authority) => authority.resolve(path),
            Self::Rooted(authority) => authority.resolve(path),
        }
    }

    /// Reads metadata through the concrete authority.
    pub(crate) fn metadata(
        &self,
        path: &AuthorityPath,
    ) -> LocalResult<LocalFileMetadata> {
        match self {
            Self::Host(authority) => authority.metadata(path),
            Self::Rooted(authority) => authority.metadata(path),
        }
    }

    pub(crate) fn resolve_metadata(
        &self,
        path: &Path,
    ) -> LocalResult<AuthorityPath> {
        match self {
            Self::Host(authority) => authority.resolve_metadata(path),
            Self::Rooted(authority) => authority.resolve_metadata(path),
        }
    }

    /// Opens a file reader through the concrete authority.
    pub(crate) fn open_reader(
        &self,
        path: &AuthorityPath,
    ) -> LocalResult<OpenedFile> {
        match self {
            Self::Host(authority) => authority.open_reader(path),
            Self::Rooted(authority) => authority.open_reader(path),
        }
    }

    /// Opens a directory cursor through the concrete authority.
    pub(crate) fn open_directory(
        &self,
        path: &AuthorityPath,
    ) -> LocalResult<DirectoryCursor> {
        match self {
            Self::Host(authority) => authority.open_directory(path),
            Self::Rooted(authority) => authority.open_directory(path),
        }
    }

    /// Creates a directory, accepting an existing directory.
    pub(crate) fn create_directory(
        &self,
        path: &AuthorityPath,
    ) -> LocalResult<()> {
        match self {
            Self::Host(authority) => authority.create_directory(path),
            Self::Rooted(authority) => authority.create_directory(path),
        }
    }

    /// Creates a directory only when the final entry is absent.
    pub(crate) fn create_directory_new(
        &self,
        path: &AuthorityPath,
    ) -> LocalResult<()> {
        match self {
            Self::Host(authority) => authority.create_directory_new(path),
            Self::Rooted(authority) => authority.create_directory_new(path),
        }
    }

    /// Creates and opens a new private regular file.
    #[allow(dead_code)]
    pub(crate) fn create_file_new(
        &self,
        path: &AuthorityPath,
    ) -> LocalResult<File> {
        match self {
            Self::Host(authority) => authority.create_file_new(path),
            Self::Rooted(authority) => authority.create_file_new(path),
        }
    }

    /// Deletes a file or non-directory entry without following it.
    pub(crate) fn delete_file(&self, path: &AuthorityPath) -> LocalResult<()> {
        match self {
            Self::Host(authority) => authority.delete_file(path),
            Self::Rooted(authority) => authority.delete_file(path),
        }
    }

    /// Deletes an empty directory without following it.
    pub(crate) fn delete_directory(
        &self,
        path: &AuthorityPath,
    ) -> LocalResult<()> {
        match self {
            Self::Host(authority) => authority.delete_directory(path),
            Self::Rooted(authority) => authority.delete_directory(path),
        }
    }

    /// Renames an entry through the concrete authority.
    pub(crate) fn rename(
        &self,
        source: &AuthorityPath,
        target: &AuthorityPath,
        overwrite: bool,
    ) -> LocalResult<()> {
        match self {
            Self::Host(authority) => {
                authority.rename(source, target, overwrite)
            }
            Self::Rooted(authority) => {
                authority.rename(source, target, overwrite)
            }
        }
    }

    /// Creates a private staging file beside `target`.
    #[allow(dead_code)]
    pub(crate) fn create_staged_file(
        &self,
        target: &AuthorityPath,
    ) -> LocalResult<StagedFile> {
        match self {
            Self::Host(authority) => authority.create_staged_file(target),
            Self::Rooted(authority) => authority.create_staged_file(target),
        }
    }

    /// Reads the native identity of an entry.
    pub(crate) fn entry_identity(
        &self,
        path: &AuthorityPath,
    ) -> LocalResult<EntryIdentity> {
        match self {
            Self::Host(authority) => authority.entry_identity(path),
            Self::Rooted(authority) => authority.entry_identity(path),
        }
    }

    /// Synchronizes the directory containing `path`.
    pub(crate) fn sync_parent(&self, path: &AuthorityPath) -> LocalResult<()> {
        match self {
            Self::Host(authority) => authority.sync_parent(path),
            Self::Rooted(authority) => authority.sync_parent(path),
        }
    }

    /// Reads filesystem path limits through the concrete authority.
    pub(crate) fn filesystem_limits(
        &self,
        path: &AuthorityPath,
    ) -> LocalResult<LocalFileSystemLimits> {
        match self {
            Self::Host(authority) => authority.filesystem_limits(path),
            Self::Rooted(authority) => authority.filesystem_limits(path),
        }
    }

    /// Reads filesystem capacity through the concrete authority.
    pub(crate) fn filesystem_space(
        &self,
        path: &AuthorityPath,
    ) -> LocalResult<LocalFileSystemSpace> {
        match self {
            Self::Host(authority) => authority.filesystem_space(path),
            Self::Rooted(authority) => authority.filesystem_space(path),
        }
    }

    /// Reports whether two paths name the same lexical or native entry.
    ///
    /// A missing target is reported as `false`; all other identity failures
    /// retain their structured error.
    pub(crate) fn same_entry(
        &self,
        first: &AuthorityPath,
        second: &AuthorityPath,
    ) -> LocalResult<bool> {
        if first == second {
            return Ok(true);
        }
        let first = self.entry_identity(first)?;
        match self.entry_identity(second) {
            Ok(second) => Ok(first == second),
            Err(error) if error.kind() == LocalFileErrorKind::NotFound => {
                Ok(false)
            }
            Err(error) => Err(error),
        }
    }
}
