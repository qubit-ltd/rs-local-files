// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Configured Host or Rooted local filesystem service.
// qubit-style: allow source-test-pair

use std::io::Read;
use std::path::Path;
use std::sync::Arc;
use std::time::Instant;

use crate::LocalCopyOptions;
use crate::LocalCopyResult;
use crate::LocalCreateDirectoryOptions;
use crate::LocalCreateDirectoryOutcome;
use crate::LocalDeleteOptions;
use crate::LocalDeleteOutcome;
use crate::LocalDirectoryWalker;
use crate::LocalDurabilityRequirement;
use crate::LocalFileError;
use crate::LocalFileErrorKind;
use crate::LocalFileKind;
use crate::LocalFileMetadata;
use crate::LocalFileOperation;
use crate::LocalFileReader;
use crate::LocalFileSystemLimits;
use crate::LocalFileSystemProtocols;
use crate::LocalFileSystemScope;
use crate::LocalFileSystemSpace;
use crate::LocalFileWriter;
use crate::LocalListOptions;
use crate::LocalReadOptions;
use crate::LocalRenameOptions;
use crate::LocalRenameOutcome;
use crate::LocalRenameResult;
use crate::LocalResult;
use crate::LocalSymlinkPolicy;
use crate::LocalTempDirectory;
use crate::LocalTempDirectoryOptions;
use crate::LocalTempFile;
use crate::LocalTempFileOptions;
use crate::LocalWriteOptions;
use crate::file_system::LocalCopyLimits;
use crate::file_system::LocalFileSystemCore;
use crate::file_system::LocalWalkLimits;
use crate::local::HostLocalFileSystem;
use crate::local::LocalNamespace;
use crate::local::copy_failure_unchanged;
use crate::local::rename_failure_renamed;
use crate::local::rename_failure_unchanged;
use crate::rooted_local_file_system::RootedLocalFileSystem;

/// Synchronous local filesystem configured for Host or Rooted path access.
///
/// Every operation inherits [`Self::symlink_policy`]. Host instances default
/// to [`LocalSymlinkPolicy::FollowAcrossScope`], while Rooted instances default
/// to [`LocalSymlinkPolicy::FollowWithinScope`]. Rooted instances reject
/// [`LocalSymlinkPolicy::FollowAcrossScope`]; the policy controls non-final
/// path components and final-link behavior remains operation-specific.
///
/// Cloning a filesystem handle is cheap: Host handles copy their stateless
/// configuration, while Rooted handles share the opened authority.
#[derive(Clone, Debug)]
pub struct LocalFileSystem {
    /// Native namespace used by every operation.
    namespace: LocalNamespace,
    /// Build protocol snapshot retained for stable reporting.
    capabilities: LocalFileSystemProtocols,
    /// Symbolic-link resolution policy inherited by operations.
    symlink_policy: LocalSymlinkPolicy,
    /// Instance-level traversal limits.
    walk_limits: LocalWalkLimits,
    /// Instance-level copy limits.
    copy_limits: LocalCopyLimits,
    /// Immutable authority and configuration shared by filesystem clones.
    pub(crate) core: Arc<LocalFileSystemCore>,
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
    pub fn host() -> Self {
        Self {
            namespace: LocalNamespace::Host,
            capabilities: HostLocalFileSystem::protocols(),
            symlink_policy: LocalSymlinkPolicy::FollowAcrossScope,
            walk_limits: LocalWalkLimits::default(),
            copy_limits: LocalCopyLimits::default(),
            core: Arc::new(LocalFileSystemCore {
                authority: None,
                paths: crate::LocalPaths::host(),
                protocols: HostLocalFileSystem::protocols(),
                limits: LocalFileSystemLimits::new(
                    crate::SizeLimit::VariesByPath,
                    crate::SizeLimit::VariesByPath,
                ),
                walk_limits: LocalWalkLimits::default(),
                copy_limits: LocalCopyLimits::default(),
                #[cfg(feature = "test-support")]
                test_faults: None,
            }),
        }
    }

    /// Creates a Host filesystem after binding the current directory handle.
    pub(crate) fn try_host() -> LocalResult<Self> {
        let authority = crate::authority::HostAuthority::bind_current(
            LocalSymlinkPolicy::FollowAcrossScope,
        )?;
        let mut filesystem = Self::host();
        filesystem.core = Arc::new(LocalFileSystemCore {
            authority: Some(Arc::new(crate::authority::Authority::Host(
                authority,
            ))),
            paths: crate::LocalPaths::host(),
            protocols: filesystem.capabilities,
            limits: filesystem.limits(),
            walk_limits: filesystem.walk_limits,
            copy_limits: filesystem.copy_limits,
            #[cfg(feature = "test-support")]
            test_faults: None,
        });
        Ok(filesystem)
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
        validate_rooted_symlink_policy(
            symlink_policy,
            LocalFileOperation::OpenRoot,
            Some(root),
        )?;
        let rooted = RootedLocalFileSystem::open(root)?;
        let capabilities = rooted.protocols();
        let limits = rooted.limits();
        let authority =
            crate::authority::RootedAuthority::open(root, symlink_policy)?;
        Ok(Self {
            capabilities,
            namespace: LocalNamespace::Rooted(rooted),
            symlink_policy,
            walk_limits: LocalWalkLimits::default(),
            copy_limits: LocalCopyLimits::default(),
            core: Arc::new(LocalFileSystemCore {
                authority: Some(Arc::new(crate::authority::Authority::Rooted(
                    authority,
                ))),
                paths: crate::LocalPaths::rooted(),
                protocols: capabilities,
                limits,
                walk_limits: LocalWalkLimits::default(),
                copy_limits: LocalCopyLimits::default(),
                #[cfg(feature = "test-support")]
                test_faults: None,
            }),
        })
    }

    /// Returns a copy of this filesystem using another symbolic-link policy.
    ///
    /// The policy applies to subsequent operations made through the returned
    /// value. Rooted filesystems reject
    /// [`LocalSymlinkPolicy::FollowAcrossScope`].
    ///
    /// # Parameters
    ///
    /// - `symlink_policy`: Policy to inherit for subsequent operations.
    ///
    /// # Returns
    ///
    /// This filesystem with the requested policy, or an error when the policy
    /// is incompatible with its namespace.
    ///
    /// # Errors
    ///
    /// Returns [`LocalFileErrorKind::InvalidOptions`] when a Rooted filesystem
    /// is configured with [`LocalSymlinkPolicy::FollowAcrossScope`].
    pub fn with_symlink_policy(
        mut self,
        symlink_policy: LocalSymlinkPolicy,
    ) -> LocalResult<Self> {
        if let LocalNamespace::Rooted(rooted) = &self.namespace {
            validate_rooted_symlink_policy(
                symlink_policy,
                LocalFileOperation::Configure,
                Some(rooted.diagnostic_path()),
            )?;
        }
        self.symlink_policy = symlink_policy;
        Ok(self)
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

    /// Returns the immutable traversal limits configured for this instance.
    #[inline(always)]
    pub fn walk_limits(&self) -> LocalWalkLimits {
        self.core.walk_limits
    }

    /// Returns the immutable copy limits configured for this instance.
    #[inline(always)]
    pub fn copy_limits(&self) -> LocalCopyLimits {
        self.core.copy_limits
    }

    /// Applies validated immutable resource limits during construction.
    pub(crate) fn with_limits(
        mut self,
        walk_limits: LocalWalkLimits,
        copy_limits: LocalCopyLimits,
    ) -> Self {
        self.walk_limits = walk_limits;
        self.copy_limits = copy_limits;
        self.core = Arc::new(LocalFileSystemCore {
            authority: self.core.authority.clone(),
            paths: self.core.paths,
            protocols: self.core.protocols,
            limits: self.core.limits,
            walk_limits,
            copy_limits,
            #[cfg(feature = "test-support")]
            test_faults: self.core.test_faults.clone(),
        });
        self
    }

    /// Returns the retained authority for operations migrated to the core.
    pub(crate) fn authority(&self) -> Option<&crate::authority::Authority> {
        self.core.authority.as_deref()
    }

    /// Replaces the immutable test-fault plan while constructing an instance.
    #[cfg(feature = "test-support")]
    pub(crate) fn with_test_faults(
        mut self,
        test_faults: Option<crate::TestFaultPlan>,
    ) -> Self {
        self.core = Arc::new(LocalFileSystemCore {
            authority: self.core.authority.clone(),
            paths: self.core.paths,
            protocols: self.core.protocols,
            limits: self.core.limits,
            walk_limits: self.core.walk_limits,
            copy_limits: self.core.copy_limits,
            test_faults,
        });
        self
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
            LocalNamespace::Rooted(rooted) => rooted.authority().scope(),
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
            LocalNamespace::Rooted(rooted) => {
                rooted.authority().diagnostic_root()
            }
        }
    }

    /// Returns the native protocol snapshot captured by this filesystem.
    ///
    /// # Returns
    ///
    /// A copy of the immutable protocol snapshot for this build and
    /// authority type.
    #[must_use = "the filesystem protocols must be used"]
    #[inline(always)]
    pub const fn protocols(&self) -> LocalFileSystemProtocols {
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
        if let Some(authority) = self.authority() {
            let resolved = authority.resolve(path)?;
            return authority.filesystem_limits(&resolved);
        }
        match &self.namespace {
            LocalNamespace::Host => migrated_host_probe(
                self.symlink_policy,
                path,
                crate::authority::Authority::filesystem_limits,
            ),
            LocalNamespace::Rooted(rooted) => {
                let resolved = rooted.authority().resolve(path)?;
                rooted.authority().filesystem_limits(&resolved)
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
        if let Some(authority) = self.authority() {
            let resolved = authority.resolve(path)?;
            return authority.filesystem_space(&resolved);
        }
        match &self.namespace {
            LocalNamespace::Host => migrated_host_probe(
                self.symlink_policy,
                path,
                crate::authority::Authority::filesystem_space,
            ),
            LocalNamespace::Rooted(rooted) => {
                let resolved = rooted.authority().resolve(path)?;
                rooted.authority().filesystem_space(&resolved)
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
        self.core
            .fail_if_requested(crate::test_support::TestFaultPoint::Metadata)
            .map_err(|error| {
                LocalFileError::from_io(
                    LocalFileOperation::Metadata,
                    Some(path.to_path_buf()),
                    None,
                    error,
                )
            })?;
        if let Some(authority) = self.authority() {
            let resolved =
                authority.resolve_metadata(path).map_err(|error| {
                    error.with_operation(LocalFileOperation::Metadata)
                })?;
            return authority.metadata(&resolved);
        }
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
    /// The final symbolic link is followed according to the filesystem policy
    /// on Unix; Windows rejects a final name-surrogate reparse point.
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
        self.core
            .fail_if_requested(crate::test_support::TestFaultPoint::Metadata)
            .map_err(|error| {
                LocalFileError::from_io(
                    LocalFileOperation::OpenReader,
                    Some(path.to_path_buf()),
                    None,
                    error,
                )
            })?;
        if let Some(authority) = self.authority() {
            let resolved = authority.resolve(path)?;
            let opened = authority.open_reader(&resolved)?;
            return Ok(LocalFileReader::from_opened(opened));
        }
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

    /// Reads at most max_bytes from a regular file.
    ///
    /// The file is opened and validated even when max_bytes is zero. Bytes
    /// are read incrementally so the method never allocates the requested
    /// limit up front.
    pub fn read_prefix(
        &self,
        path: &Path,
        options: &LocalReadOptions,
        max_bytes: usize,
    ) -> LocalResult<Vec<u8>> {
        let mut reader = self.open_reader(path, options)?;
        if max_bytes == 0 {
            return Ok(Vec::new());
        }
        let mut result = Vec::with_capacity(max_bytes.min(8192));
        let mut buffer = [0_u8; 8192];
        while result.len() < max_bytes {
            let read_len = (max_bytes - result.len()).min(buffer.len());
            let count =
                crate::local::test_io_error("local-fs-read-prefix-read")
                    .map_or_else(|| reader.read(&mut buffer[..read_len]), Err)
                    .map_err(|source| {
                        LocalFileError::from_io(
                            LocalFileOperation::Read,
                            Some(path.to_path_buf()),
                            None,
                            source,
                        )
                    })?;
            if count == 0 {
                break;
            }
            result.extend_from_slice(&buffer[..count]);
        }
        Ok(result)
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
    /// A writer session. Staged modes publish only when committed; `Append`
    /// modifies the existing file directly as bytes are written.
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
        if let LocalNamespace::Rooted(_) = &self.namespace {
            let symlink_policy =
                options.symlink_policy().unwrap_or(self.symlink_policy);
            validate_rooted_symlink_policy(
                symlink_policy,
                LocalFileOperation::List,
                Some(path),
            )?;
        }
        if let Some(authority) = self.authority() {
            let resolved = authority.resolve(path)?;
            let mut directory = authority.open_directory(&resolved)?;
            let _ = directory.next_entry()?;
        }
        match &self.namespace {
            LocalNamespace::Host => HostLocalFileSystem::list_with_policy(
                path,
                options,
                self.symlink_policy,
            ),
            LocalNamespace::Rooted(rooted) => {
                let symlink_policy =
                    options.symlink_policy().unwrap_or(self.symlink_policy);
                rooted.list(path, options, symlink_policy)
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
        if options.deadline().is_some_and(|duration| {
            Instant::now().checked_add(duration).is_none()
        }) {
            return Err(copy_failure_unchanged(
                LocalFileError::new(
                    LocalFileErrorKind::InvalidOptions,
                    LocalFileOperation::Copy,
                )
                .with_reason("copy deadline exceeds the monotonic clock range")
                .with_path(source.to_path_buf())
                .with_target(destination.to_path_buf()),
            ));
        }
        match &self.namespace {
            LocalNamespace::Host => HostLocalFileSystem::copy_with_policy(
                source,
                destination,
                options,
                self.symlink_policy,
            ),
            LocalNamespace::Rooted(rooted) => {
                let symlink_policy = options
                    .symlink_policy_override()
                    .unwrap_or(self.symlink_policy);
                validate_rooted_symlink_policy(
                    symlink_policy,
                    LocalFileOperation::Copy,
                    Some(source),
                )
                .map_err(copy_failure_unchanged)?;
                rooted.copy(source, destination, options, symlink_policy)
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
        if let Some(authority) = self.authority()
            && !options.recursive()
        {
            let resolved = authority.resolve(path)?;
            let existed = authority
                .metadata(&resolved)
                .map(|metadata| metadata.kind() == LocalFileKind::Directory)
                .unwrap_or(false);
            if options.exists_ok() {
                authority.create_directory(&resolved)?;
            } else {
                authority.create_directory_new(&resolved)?;
            }
            return Ok(LocalCreateDirectoryOutcome::new(!existed));
        }
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
        if let Some(authority) = self.authority() {
            let resolved = authority.resolve(path)?;
            match authority.delete_file(&resolved) {
                Ok(()) => return Ok(LocalDeleteOutcome::new(true)),
                Err(error)
                    if options.missing_ok()
                        && error.kind() == LocalFileErrorKind::NotFound =>
                {
                    return Ok(LocalDeleteOutcome::new(false));
                }
                Err(error) => return Err(error),
            }
        }
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
        if let Some(authority) = self.authority()
            && !options.recursive()
        {
            let resolved = authority.resolve(path)?;
            match authority.delete_directory(&resolved) {
                Ok(()) => return Ok(LocalDeleteOutcome::new(true)),
                Err(error)
                    if options.missing_ok()
                        && error.kind() == LocalFileErrorKind::NotFound =>
                {
                    return Ok(LocalDeleteOutcome::new(false));
                }
                Err(error) => return Err(error),
            }
        }
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
    /// Intermediate components remain within the opened root. Final source and
    /// destination links are renamed as entries.
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
        if let Some(authority) = self.authority() {
            let source = authority
                .resolve(source)
                .map_err(rename_failure_unchanged)?;
            let destination = authority
                .resolve(destination)
                .map_err(rename_failure_unchanged)?;
            if authority
                .same_entry(&source, &destination)
                .map_err(rename_failure_unchanged)?
            {
                return Ok(LocalRenameOutcome::new(true, false));
            }
            authority
                .rename(&source, &destination, options.overwrite())
                .map_err(rename_failure_unchanged)?;
            let durable = match options.durability() {
                LocalDurabilityRequirement::NotRequired => false,
                LocalDurabilityRequirement::Preferred
                | LocalDurabilityRequirement::Required => authority
                    .sync_parent(&destination)
                    .map_err(rename_failure_renamed)
                    .map(|()| true)?,
            };
            return Ok(LocalRenameOutcome::new(true, durable));
        }
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

/// Runs one migrated Host probe through a construction-time cwd authority.
fn migrated_host_probe<T>(
    symlink_policy: LocalSymlinkPolicy,
    path: &Path,
    probe: fn(
        &crate::authority::Authority,
        &crate::authority::AuthorityPath,
    ) -> LocalResult<T>,
) -> LocalResult<T> {
    let authority = crate::authority::Authority::Host(
        crate::authority::HostAuthority::bind_current(symlink_policy)?,
    );
    let resolved = authority.resolve(path)?;
    probe(&authority, &resolved)
}

/// Rejects a symbolic-link policy that cannot preserve Rooted authority.
fn validate_rooted_symlink_policy(
    symlink_policy: LocalSymlinkPolicy,
    operation: LocalFileOperation,
    path: Option<&Path>,
) -> LocalResult<()> {
    if symlink_policy != LocalSymlinkPolicy::FollowAcrossScope {
        return Ok(());
    }
    let mut error = LocalFileError::new(LocalFileErrorKind::InvalidOptions, operation)
        .with_reason(
            "FollowAcrossScope is not supported by Rooted filesystems because Rooted authority cannot escape its opened root",
        );
    if let Some(path) = path {
        error = error.with_path(path.to_path_buf());
    }
    Err(error)
}
