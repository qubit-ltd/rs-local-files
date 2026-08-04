// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Configured Host or Rooted local filesystem service.
// qubit-style: allow source-test-pair

use std::{
    fs::File,
    path::Path,
};

use crate::local::{
    HostLocalFileSystem,
    LocalNamespace,
};
use crate::rooted_local_file_system::RootedLocalFileSystem;
use crate::{
    LocalCopyOptions,
    LocalCopyResult,
    LocalCreateDirectoryOptions,
    LocalCreateDirectoryOutcome,
    LocalDeleteOptions,
    LocalDeleteOutcome,
    LocalDirectoryWalker,
    LocalFileError,
    LocalFileMetadata,
    LocalFileOperation,
    LocalFileReader,
    LocalFileSystemCapabilities,
    LocalFileSystemLimits,
    LocalFileSystemScope,
    LocalFileSystemSpace,
    LocalFileWriter,
    LocalListOptions,
    LocalReadOptions,
    LocalRenameOptions,
    LocalRenameResult,
    LocalResult,
    LocalSymlinkPolicy,
    LocalTempDirectory,
    LocalTempDirectoryOptions,
    LocalTempFile,
    LocalTempFileOptions,
    LocalWriteOptions,
};

/// Synchronous local filesystem configured for Host or Rooted path access.
///
/// Every operation inherits [`Self::symlink_policy`]. Host instances default
/// to [`LocalSymlinkPolicy::FollowAcrossScope`], while rooted instances default
/// to [`LocalSymlinkPolicy::FollowWithinScope`]. The policy controls
/// non-final path components; final-link behavior remains operation-specific.
#[derive(Debug)]
pub struct LocalFileSystem {
    /// Native namespace used by every operation.
    namespace: LocalNamespace,
    /// Build capability snapshot retained for stable reporting.
    capabilities: LocalFileSystemCapabilities,
    /// Symbolic-link resolution policy inherited by operations.
    symlink_policy: LocalSymlinkPolicy,
}

impl LocalFileSystem {
    /// Creates a filesystem over the process-visible Host namespace.
    ///
    /// Host defaults to [`LocalSymlinkPolicy::FollowAcrossScope`].
    ///
    /// # Returns
    ///
    /// A filesystem whose paths are interpreted by the process-visible Host
    /// namespace.
    #[must_use]
    #[inline(always)]
    pub const fn host() -> Self {
        Self {
            namespace: LocalNamespace::Host,
            capabilities: HostLocalFileSystem::capabilities(),
            symlink_policy: LocalSymlinkPolicy::FollowAcrossScope,
        }
    }

    /// Opens a descriptor- or handle-authoritative Rooted namespace.
    ///
    /// Rooted defaults to [`LocalSymlinkPolicy::FollowWithinScope`].
    ///
    /// # Parameters
    ///
    /// - `root`: Existing native directory used as the authority root.
    ///
    /// # Returns
    ///
    /// A filesystem whose later paths are validated descendants of `root`.
    ///
    /// # Errors
    ///
    /// Returns `LocalFileError` when the root is not a directory or cannot be
    /// opened securely on the current platform.
    pub fn rooted(root: &Path) -> LocalResult<Self> {
        Self::rooted_with_symlink_policy(
            root,
            LocalSymlinkPolicy::FollowWithinScope,
        )
    }

    /// Opens a rooted filesystem with an explicit symbolic-link policy.
    ///
    /// # Parameters
    ///
    /// - `root`: Existing native directory used as the authority root.
    /// - `symlink_policy`: Policy applied to intermediate symbolic links.
    ///
    /// # Returns
    ///
    /// A filesystem whose later paths are validated descendants of `root`.
    ///
    /// # Errors
    ///
    /// Returns `LocalFileError` when the root is not a directory or cannot be
    /// opened securely on the current platform.
    pub fn rooted_with_symlink_policy(
        root: &Path,
        symlink_policy: LocalSymlinkPolicy,
    ) -> LocalResult<Self> {
        let rooted = RootedLocalFileSystem::open(root)?;
        Ok(Self {
            capabilities: rooted.capabilities(),
            namespace: LocalNamespace::Rooted(rooted),
            symlink_policy,
        })
    }

    /// Returns a copy of this filesystem using another symbolic-link policy.
    ///
    /// The policy applies to subsequent operations made through the returned
    /// value. For rooted filesystems, `FollowAcrossScope` intentionally permits
    /// reads and mutations through links that resolve outside the opened root.
    ///
    /// # Parameters
    ///
    /// - `symlink_policy`: Policy to inherit for subsequent operations.
    ///
    /// # Returns
    ///
    /// This filesystem with the requested policy.
    #[must_use]
    #[inline(always)]
    pub fn with_symlink_policy(
        mut self,
        symlink_policy: LocalSymlinkPolicy,
    ) -> Self {
        self.symlink_policy = symlink_policy;
        self
    }

    /// Returns the symbolic-link policy inherited by operations.
    ///
    /// # Returns
    ///
    /// The policy applied to intermediate symbolic links by default.
    #[must_use = "the filesystem symlink policy must be used"]
    #[inline(always)]
    pub const fn symlink_policy(&self) -> LocalSymlinkPolicy {
        self.symlink_policy
    }

    /// Returns the namespace in which this filesystem interprets paths.
    ///
    /// # Returns
    ///
    /// [`LocalFileSystemScope::Host`] for a Host filesystem or
    /// [`LocalFileSystemScope::Rooted`] for a rooted filesystem.
    #[must_use = "the filesystem scope must be used"]
    #[inline(always)]
    pub fn scope(&self) -> LocalFileSystemScope {
        match &self.namespace {
            LocalNamespace::Host => LocalFileSystemScope::Host,
            LocalNamespace::Rooted(_) => LocalFileSystemScope::Rooted,
        }
    }

    /// Returns the non-authoritative root path retained for diagnostics.
    ///
    /// Returns `Some` for rooted filesystems and `None` for the process-visible
    /// Host namespace. The returned path is diagnostic context, not an
    /// authority for later operations.
    #[must_use]
    #[inline(always)]
    pub fn diagnostic_root(&self) -> Option<&Path> {
        match &self.namespace {
            LocalNamespace::Host => None,
            LocalNamespace::Rooted(rooted) => Some(rooted.diagnostic_path()),
        }
    }

    /// Returns the build capability snapshot captured by this filesystem.
    ///
    /// # Returns
    ///
    /// A copy of the immutable capability snapshot for this build and
    /// authority type.
    #[must_use = "the filesystem capabilities must be used"]
    #[inline(always)]
    pub const fn capabilities(&self) -> LocalFileSystemCapabilities {
        self.capabilities
    }

    /// Returns path limits cached for this filesystem authority.
    ///
    /// Host spans multiple filesystems and therefore reports no global
    /// preflight limit. Rooted instances retain the best-effort observation
    /// made while opening their authority.
    #[inline(always)]
    pub const fn limits(&self) -> LocalFileSystemLimits {
        match &self.namespace {
            LocalNamespace::Host => LocalFileSystemLimits::new(
                crate::SizeLimit::VariesByPath,
                crate::SizeLimit::VariesByPath,
            ),
            LocalNamespace::Rooted(rooted) => rooted.limits(),
        }
    }

    /// Observes path limits at `path` or its nearest existing ancestor.
    ///
    /// # Errors
    ///
    /// Returns `LocalFileError` only when the path cannot be interpreted in
    /// this filesystem authority. Probe failures yield `Unknown` dimensions.
    pub fn limits_at(&self, path: &Path) -> LocalResult<LocalFileSystemLimits> {
        match &self.namespace {
            LocalNamespace::Host => {
                probe_file(path, LocalFileOperation::Metadata).map(|file| {
                    file.map_or_else(
                        || {
                            LocalFileSystemLimits::new(
                                crate::SizeLimit::Unknown,
                                crate::SizeLimit::Unknown,
                            )
                        },
                        |file| crate::capability::probe_limits(&file),
                    )
                })
            }
            LocalNamespace::Rooted(rooted) => {
                let _ = crate::local::LocalRelativePath::new(path).map_err(
                    |error| {
                        LocalFileError::from_io(
                            LocalFileOperation::Metadata,
                            Some(path.to_path_buf()),
                            None,
                            error,
                        )
                    },
                )?;
                Ok(rooted.limits())
            }
        }
    }

    /// Observes dynamic space at `path` or its nearest existing ancestor.
    ///
    /// # Errors
    ///
    /// Returns `LocalFileError` only when the path cannot be interpreted in
    /// this filesystem authority. Probe failures yield absent observations.
    pub fn space_at(&self, path: &Path) -> LocalResult<LocalFileSystemSpace> {
        match &self.namespace {
            LocalNamespace::Host => {
                probe_file(path, LocalFileOperation::Metadata).map(|file| {
                    file.map_or_else(
                        || LocalFileSystemSpace::new(None, None, None),
                        |file| crate::capability::probe_space(&file),
                    )
                })
            }
            LocalNamespace::Rooted(rooted) => {
                let _ = crate::local::LocalRelativePath::new(path).map_err(
                    |error| {
                        LocalFileError::from_io(
                            LocalFileOperation::Metadata,
                            Some(path.to_path_buf()),
                            None,
                            error,
                        )
                    },
                )?;
                Ok(rooted.space())
            }
        }
    }

    /// Reads metadata without following the final symbolic link.
    ///
    /// Intermediate path components follow the filesystem policy.
    ///
    /// # Parameters
    ///
    /// - `path`: Path to inspect in this filesystem's namespace.
    ///
    /// # Returns
    ///
    /// Metadata for the addressed entry, including the final link entry when
    /// `path` names a symbolic link.
    ///
    /// # Errors
    ///
    /// Returns `LocalFileError` when path validation or native inspection
    /// fails.
    #[inline]
    pub fn metadata(&self, path: &Path) -> LocalResult<LocalFileMetadata> {
        match &self.namespace {
            LocalNamespace::Host => HostLocalFileSystem::metadata_with_policy(
                path,
                self.symlink_policy,
            ),
            LocalNamespace::Rooted(rooted) => {
                rooted.metadata(path, self.symlink_policy)
            }
        }
    }

    /// Opens a synchronous regular-file reader.
    ///
    /// The final symbolic link is followed according to the filesystem policy.
    ///
    /// # Parameters
    ///
    /// - `path`: Regular-file path to open.
    /// - `options`: Reader behavior and retry configuration.
    ///
    /// # Returns
    ///
    /// A reader that owns the opened file handle.
    ///
    /// # Errors
    ///
    /// Returns `LocalFileError` for invalid paths, entry kinds, options, or
    /// native open failures.
    pub fn open_reader(
        &self,
        path: &Path,
        options: &LocalReadOptions,
    ) -> LocalResult<LocalFileReader> {
        match &self.namespace {
            LocalNamespace::Host => {
                HostLocalFileSystem::open_reader_with_policy(
                    path,
                    options,
                    self.symlink_policy,
                )
            }
            LocalNamespace::Rooted(rooted) => {
                rooted.open_reader(path, options, self.symlink_policy)
            }
        }
    }

    /// Opens a synchronous writer publication session.
    ///
    /// `Append` and `CreateOrReplace` follow a final symbolic link and modify
    /// its target. `CreateNew` treats a final link as an existing entry.
    ///
    /// # Parameters
    ///
    /// - `path`: Publication target path.
    /// - `options`: Writer mode, replacement, durability, and retry policy.
    ///
    /// # Returns
    ///
    /// A writer session whose bytes are published only when committed.
    ///
    /// # Errors
    ///
    /// Returns `LocalFileError` for invalid paths, conflicts, unsupported
    /// requirements, or native open failures.
    pub fn open_writer(
        &self,
        path: &Path,
        options: &LocalWriteOptions,
    ) -> LocalResult<LocalFileWriter> {
        match &self.namespace {
            LocalNamespace::Host => {
                HostLocalFileSystem::open_writer_with_policy(
                    path,
                    options,
                    self.symlink_policy,
                )
            }
            LocalNamespace::Rooted(rooted) => {
                rooted.open_writer(path, options, self.symlink_policy)
            }
        }
    }

    /// Opens a lazy directory walker.
    ///
    /// Directory links are followed according to the inherited policy and any
    /// option override. Returned paths remain logical paths through a link.
    ///
    /// # Parameters
    ///
    /// - `path`: Directory from which to begin the walk.
    /// - `options`: Recursion, depth, link, and error-handling policies.
    ///
    /// # Returns
    ///
    /// A lazy walker that opens descendants as they are iterated.
    ///
    /// # Errors
    ///
    /// Returns `LocalFileError` for invalid paths, unsupported policies, or
    /// native directory-open failures.
    pub fn list(
        &self,
        path: &Path,
        options: &LocalListOptions,
    ) -> LocalResult<LocalDirectoryWalker> {
        match &self.namespace {
            LocalNamespace::Host => HostLocalFileSystem::list_with_policy(
                path,
                options,
                self.symlink_policy,
            ),
            LocalNamespace::Rooted(rooted) => {
                rooted.list(path, options, self.symlink_policy)
            }
        }
    }

    /// Copies one regular file or directory tree.
    ///
    /// Intermediate source and target components follow the inherited policy.
    /// A final source link is copied as a link entry, while a final target link
    /// is replaced as an entry.
    ///
    /// # Parameters
    ///
    /// - `source`: File or directory tree to copy.
    /// - `destination`: Destination path to create or replace.
    /// - `options`: Conflict, source-kind, and durability policy.
    ///
    /// # Returns
    ///
    /// Copy statistics and publication guarantees when successful.
    ///
    /// # Errors
    ///
    /// Returns `LocalCopyFailure` with the strongest known destination state
    /// when validation or native copying fails.
    #[allow(clippy::result_large_err)]
    pub fn copy(
        &self,
        source: &Path,
        destination: &Path,
        options: &LocalCopyOptions,
    ) -> LocalCopyResult {
        match &self.namespace {
            LocalNamespace::Host => HostLocalFileSystem::copy_with_policy(
                source,
                destination,
                options,
                self.symlink_policy,
            ),
            LocalNamespace::Rooted(rooted) => {
                rooted.copy(source, destination, options, self.symlink_policy)
            }
        }
    }

    /// Creates a directory with the selected parent policy.
    ///
    /// # Parameters
    ///
    /// - `path`: Directory path to create.
    /// - `options`: Parent creation and existing-entry policy.
    ///
    /// # Returns
    ///
    /// An outcome indicating whether a new directory was created.
    ///
    /// # Errors
    ///
    /// Returns `LocalFileError` for invalid paths, type conflicts, or native
    /// creation failures.
    pub fn create_directory(
        &self,
        path: &Path,
        options: &LocalCreateDirectoryOptions,
    ) -> LocalResult<LocalCreateDirectoryOutcome> {
        match &self.namespace {
            LocalNamespace::Host => {
                HostLocalFileSystem::create_directory_with_policy(
                    path,
                    options,
                    self.symlink_policy,
                )
            }
            LocalNamespace::Rooted(rooted) => {
                rooted.create_directory(path, options, self.symlink_policy)
            }
        }
    }

    /// Deletes a non-directory entry.
    ///
    /// A final symbolic link is deleted as an entry; intermediate components
    /// follow the inherited policy.
    ///
    /// # Parameters
    ///
    /// - `path`: Non-directory entry to remove.
    /// - `options`: Missing-entry policy.
    ///
    /// # Returns
    ///
    /// An outcome indicating whether an entry was deleted.
    ///
    /// # Errors
    ///
    /// Returns `LocalFileError` for invalid paths, type conflicts, or native
    /// deletion failures.
    pub fn delete_file(
        &self,
        path: &Path,
        options: &LocalDeleteOptions,
    ) -> LocalResult<LocalDeleteOutcome> {
        match &self.namespace {
            LocalNamespace::Host => {
                HostLocalFileSystem::delete_file_with_policy(
                    path,
                    options,
                    self.symlink_policy,
                )
            }
            LocalNamespace::Rooted(rooted) => {
                rooted.delete_file(path, options, self.symlink_policy)
            }
        }
    }

    /// Deletes a directory according to the recursion policy.
    ///
    /// A final symbolic link is never recursively deleted through its target.
    ///
    /// # Parameters
    ///
    /// - `path`: Directory entry to remove.
    /// - `options`: Recursion and missing-entry policy.
    ///
    /// # Returns
    ///
    /// An outcome indicating whether an entry was deleted.
    ///
    /// # Errors
    ///
    /// Returns `LocalFileError` for invalid paths, type conflicts, non-empty
    /// directories, or native deletion failures.
    pub fn delete_directory(
        &self,
        path: &Path,
        options: &LocalDeleteOptions,
    ) -> LocalResult<LocalDeleteOutcome> {
        match &self.namespace {
            LocalNamespace::Host => {
                HostLocalFileSystem::delete_directory_with_policy(
                    path,
                    options,
                    self.symlink_policy,
                )
            }
            LocalNamespace::Rooted(rooted) => {
                rooted.delete_directory(path, options, self.symlink_policy)
            }
        }
    }

    /// Renames one entry using the configured symbolic-link policy.
    ///
    /// Intermediate components may resolve across the root when explicitly
    /// configured. Final source and destination links are renamed as entries.
    ///
    /// # Parameters
    ///
    /// - `source`: Entry to rename.
    /// - `destination`: New entry path.
    /// - `options`: Overwrite and durability policy.
    ///
    /// # Returns
    ///
    /// Rename publication state and any achieved durability guarantee.
    ///
    /// # Errors
    ///
    /// Returns `LocalRenameFailure` with the strongest proven namespace state
    /// when validation or native rename fails.
    #[allow(clippy::result_large_err)]
    pub fn rename(
        &self,
        source: &Path,
        destination: &Path,
        options: &LocalRenameOptions,
    ) -> LocalRenameResult {
        match &self.namespace {
            LocalNamespace::Host => HostLocalFileSystem::rename_with_policy(
                source,
                destination,
                options,
                self.symlink_policy,
            ),
            LocalNamespace::Rooted(rooted) => {
                rooted.rename(source, destination, options, self.symlink_policy)
            }
        }
    }

    /// Creates a cleanup-owned temporary file in the configured namespace.
    ///
    /// The returned resource retains this filesystem's symbolic-link policy
    /// for later persistence targets.
    ///
    /// # Parameters
    ///
    /// - `options`: Parent, name-affix, and retry configuration.
    ///
    /// # Returns
    ///
    /// A cleanup-owned temporary file bound to this namespace.
    ///
    /// # Errors
    ///
    /// Returns `LocalFileError` for invalid options, collisions, or native
    /// creation failures.
    pub fn create_temp_file(
        &self,
        options: &LocalTempFileOptions,
    ) -> LocalResult<LocalTempFile> {
        match &self.namespace {
            LocalNamespace::Host => {
                HostLocalFileSystem::create_temp_file_with_policy(
                    options,
                    self.symlink_policy,
                )
            }
            LocalNamespace::Rooted(rooted) => {
                rooted.create_temp_file(options, self.symlink_policy)
            }
        }
    }

    /// Creates a cleanup-owned temporary directory in the configured
    /// namespace.
    ///
    /// The returned resource retains this filesystem's symbolic-link policy
    /// for later persistence targets.
    ///
    /// # Parameters
    ///
    /// - `options`: Parent, name-affix, and retry configuration.
    ///
    /// # Returns
    ///
    /// A cleanup-owned temporary directory bound to this namespace.
    ///
    /// # Errors
    ///
    /// Returns `LocalFileError` for invalid options, collisions, or native
    /// creation failures.
    pub fn create_temp_directory(
        &self,
        options: &LocalTempDirectoryOptions,
    ) -> LocalResult<LocalTempDirectory> {
        match &self.namespace {
            LocalNamespace::Host => {
                HostLocalFileSystem::create_temp_directory_with_policy(
                    options,
                    self.symlink_policy,
                )
            }
            LocalNamespace::Rooted(rooted) => {
                rooted.create_temp_directory(options, self.symlink_policy)
            }
        }
    }
}

/// Opens `path` or its nearest existing ancestor for a non-authoritative probe.
///
/// # Errors
///
/// Returns a structured error when an absolute host path cannot be formed or
/// no existing ancestor can be opened.
fn probe_file(
    path: &Path,
    operation: LocalFileOperation,
) -> LocalResult<Option<File>> {
    let mut candidate = std::path::absolute(path).map_err(|error| {
        LocalFileError::from_io(
            operation,
            Some(path.to_path_buf()),
            None,
            error,
        )
    })?;
    loop {
        match File::open(&candidate) {
            Ok(file) => return Ok(Some(file)),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                if !candidate.pop() {
                    return Ok(None);
                }
            }
            Err(_) => return Ok(None),
        }
    }
}
