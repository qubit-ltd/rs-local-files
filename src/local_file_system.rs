// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Stateful Host or Rooted local filesystem service.
// qubit-style: allow source-test-pair

// Implements capability and filesystem-space observations.
mod capability;
// Implements instance scope, policy, and default-option configuration.
mod configuration;
// Implements copy and rename operations.
mod copy;
// Implements listing, creation, and deletion operations.
mod directory;
// Implements metadata, reader, and writer operations.
mod io;
// Implements temporary-file and temporary-directory operations.
mod temp;

use std::fs;
use std::io::Read;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use crate::LocalCopyOptions;
use crate::LocalCopyResult;
use crate::LocalCreateDirectoryOptions;
use crate::LocalCreateDirectoryOutcome;
use crate::LocalDeleteOptions;
use crate::LocalDeleteOutcome;
use crate::LocalDirectoryWalker;
use crate::LocalFileError;
use crate::LocalFileErrorKind;
use crate::LocalFileKind;
use crate::LocalFileMetadata;
use crate::LocalFileOperation;
use crate::LocalFileReader;
use crate::LocalFileSystemCapabilities;
use crate::LocalFileSystemLimits;
use crate::LocalFileSystemScope;
use crate::LocalFileSystemSpace;
use crate::LocalFileWriter;
use crate::LocalListOptions;
use crate::LocalReadOptions;
use crate::LocalRenameOptions;
use crate::LocalRenameResult;
use crate::LocalResult;
use crate::LocalSymlinkPolicy;
use crate::LocalTempDirectory;
use crate::LocalTempDirectoryOptions;
use crate::LocalTempFile;
use crate::LocalTempFileOptions;
use crate::LocalWriteOptions;
use crate::file_system::LocalCurrentDirectory;
use crate::file_system::LocalFileSystemCore;
use crate::file_system::LocalFileSystemDefaults;
use crate::local::HostLocalFileSystem;
use crate::local::LocalNamespace;
use crate::local::copy_failure_unchanged;
use crate::local::rename_failure_unchanged;
use crate::local_file_system_validation::reject_directory_qualified_file;
use crate::local_file_system_validation::validate_copy_options;
use crate::local_file_system_validation::validate_list_options;
use crate::local_file_system_validation::validate_scope_symlink_policy;
use crate::local_file_system_validation::validate_temp_attempts;
use crate::path::LocalNamespacePath;
use crate::path::LocalPathResolver;
use crate::rooted_local_file_system::RootedLocalFileSystem;

/// Synchronous local filesystem with native or virtual PWD semantics.
///
/// Clones share only immutable authority state. Each clone receives an
/// independent snapshot of the virtual PWD, symlink policy, and all default
/// Options. Host instances read the process PWD when an operation binds a
/// relative path and therefore observe process-global PWD changes.
/// The type provides no contract for concurrent configuration mutation;
/// callers that share one mutable instance may add their own lock.
///
/// # Examples
///
/// ```no_run
/// use std::path::Path;
///
/// use qubit_local_files::LocalFileSystem;
///
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let filesystem = LocalFileSystem::host()?;
/// let metadata = filesystem.metadata(Path::new("Cargo.toml"))?;
/// assert!(metadata.len() > 0);
/// # Ok(())
/// # }
/// ```
#[derive(Clone, Debug)]
pub struct LocalFileSystem {
    /// Immutable authority and capability state shared by opened resources.
    pub(crate) core: Arc<LocalFileSystemCore>,
    /// Process-backed Host PWD or retained Rooted virtual PWD.
    current_directory: LocalCurrentDirectory,
    /// Default symlink policy used by operations without an override.
    symlink_policy: LocalSymlinkPolicy,
    /// Per-instance operation defaults, copied by value on clone.
    defaults: LocalFileSystemDefaults,
}

impl LocalFileSystem {
    /// Creates a filesystem bound to the host namespace.
    ///
    /// Construction does not read or snapshot the process working directory.
    /// Each later relative-path operation reads the process working directory
    /// for that operation, so external calls to `std::env::set_current_dir`
    /// remain observable.
    ///
    /// # Returns
    ///
    /// A Host-scoped filesystem with `FollowAcrossScope` symlink behavior and
    /// the default operation options.
    ///
    /// # Errors
    ///
    /// Host construction is currently infallible. The `Result` return type is
    /// retained so construction failures can be reported without a future API
    /// break if a target requires native initialization.
    pub fn host() -> LocalResult<Self> {
        let capabilities = HostLocalFileSystem::capabilities();
        Ok(Self {
            core: Arc::new(LocalFileSystemCore {
                namespace: LocalNamespace::Host,
                capabilities,
                limits: LocalFileSystemLimits::new(
                    crate::SizeLimit::VariesByPath,
                    crate::SizeLimit::VariesByPath,
                    crate::LocalPathLengthUnit::native(),
                ),
                #[cfg(feature = "test-support")]
                test_faults: None,
            }),
            current_directory: LocalCurrentDirectory::Process,
            symlink_policy: LocalSymlinkPolicy::FollowAcrossScope,
            defaults: LocalFileSystemDefaults::default(),
        })
    }

    /// Opens a descriptor- or handle-authoritative Rooted filesystem.
    ///
    /// The root path is resolved only while opening the authority. Later
    /// operations remain anchored to the opened directory even if its original
    /// path is renamed or replaced. Absolute operation paths are interpreted
    /// inside this virtual root.
    ///
    /// # Parameters
    ///
    /// - `root`: Existing native directory to bind as the virtual root. A
    ///   symbolic link is followed once during construction.
    ///
    /// # Returns
    ///
    /// A Rooted filesystem whose initial virtual working directory is `/` and
    /// whose default symlink policy is `FollowWithinScope`.
    ///
    /// # Errors
    ///
    /// Returns [`LocalFileError`] when `root` cannot be opened, does not name a
    /// directory, or the target cannot provide the required rooted authority.
    pub fn rooted(root: &Path) -> LocalResult<Self> {
        let rooted = RootedLocalFileSystem::open(root)?;
        let capabilities = rooted.capabilities();
        let limits = rooted.limits();
        Ok(Self {
            core: Arc::new(LocalFileSystemCore {
                namespace: LocalNamespace::Rooted(rooted),
                capabilities,
                limits,
                #[cfg(feature = "test-support")]
                test_faults: None,
            }),
            current_directory: LocalCurrentDirectory::Virtual(PathBuf::from(std::path::MAIN_SEPARATOR_STR)),
            symlink_policy: LocalSymlinkPolicy::FollowWithinScope,
            defaults: LocalFileSystemDefaults::default(),
        })
    }

    /// Creates a resolver using one operation's PWD snapshot when required.
    fn resolver_for(&self, path: &Path, operation: LocalFileOperation) -> LocalResult<LocalPathResolver> {
        if self.scope() == LocalFileSystemScope::Host && path.is_absolute() {
            return Ok(LocalPathResolver::absolute_host());
        }
        let current_directory = self.current_directory.snapshot(operation, Some(path))?;
        LocalPathResolver::new(self.scope(), &current_directory).map_err(|error| error.with_operation(operation))
    }

    /// Creates one resolver for a two-path operation from a single PWD
    /// snapshot.
    fn resolver_for_pair(
        &self,
        source: &Path,
        target: &Path,
        operation: LocalFileOperation,
    ) -> LocalResult<LocalPathResolver> {
        if self.scope() == LocalFileSystemScope::Host && source.is_absolute() && target.is_absolute() {
            return Ok(LocalPathResolver::absolute_host());
        }
        let current_directory = self.current_directory.snapshot(operation, Some(source))?;
        LocalPathResolver::new(self.scope(), &current_directory).map_err(|error| error.with_operation(operation))
    }

    /// Validates a directory using native lookup and the configured policy.
    fn validate_directory(&self, path: &LocalNamespacePath) -> LocalResult<()> {
        match &self.core.namespace {
            LocalNamespace::Host => HostLocalFileSystem::list_with_policy(
                path.authority_relative(),
                &LocalListOptions::new(),
                self.symlink_policy,
            )
            .map(|_| ()),
            LocalNamespace::Rooted(rooted) => rooted.validate_directory(path.authority_relative(), self.symlink_policy),
        }
    }

    /// Enforces directory-qualified native syntax before a namespace change.
    fn validate_directory_requirement(
        &self,
        path: &LocalNamespacePath,
        operation: LocalFileOperation,
        current_directory: Option<&Path>,
    ) -> LocalResult<()> {
        if !path.directory_required() {
            return Ok(());
        }
        let metadata = match &self.core.namespace {
            LocalNamespace::Host => {
                HostLocalFileSystem::metadata_with_policy(path.authority_relative(), self.symlink_policy)
            }
            LocalNamespace::Rooted(rooted) => rooted.metadata(path.authority_relative(), self.symlink_policy),
        }
        .map_err(|error| operation_error(error, operation, path.namespace_absolute(), None, current_directory))?;
        if metadata.kind() == LocalFileKind::Directory {
            return Ok(());
        }
        let error = LocalFileError::new(LocalFileErrorKind::NotDirectory, operation)
            .with_path(path.namespace_absolute().to_path_buf());
        Err(with_current_directory(error, current_directory))
    }

    /// Reports whether a path denotes the protected Rooted virtual root.
    fn is_root_operand(&self, path: &LocalNamespacePath) -> bool {
        self.scope() == LocalFileSystemScope::Rooted && path.authority_relative().as_os_str().is_empty()
    }

    /// Rejects an operation that may remove or replace the Rooted virtual root.
    fn reject_root_operand(
        &self,
        path: &LocalNamespacePath,
        operation: LocalFileOperation,
        current_directory: Option<&Path>,
    ) -> LocalResult<()> {
        if !self.is_root_operand(path) {
            return Ok(());
        }
        let error = LocalFileError::new(LocalFileErrorKind::InvalidPath, operation)
            .with_reason("the Rooted virtual root cannot be removed or replaced")
            .with_path(path.namespace_absolute().to_path_buf());
        Err(with_current_directory(error, current_directory))
    }
}

/// Resolves one path and attaches the operation input and PWD snapshot on
/// lexical failure.
fn resolve_operation_path(
    resolver: &LocalPathResolver,
    path: &Path,
    operation: LocalFileOperation,
) -> LocalResult<LocalNamespacePath> {
    resolver.resolve(path).map_err(|error| {
        let error = error.with_operation(operation).with_path(path.to_path_buf());
        with_current_directory(error, resolver.current_directory())
    })
}

/// Rewrites a backend error into the public namespace coordinate system.
fn operation_error(
    error: LocalFileError,
    operation: LocalFileOperation,
    path: &Path,
    target: Option<&Path>,
    current_directory: Option<&Path>,
) -> LocalFileError {
    let error = with_current_directory(
        error.with_operation(operation).with_path(path.to_path_buf()),
        current_directory,
    );
    if let Some(target) = target {
        error.with_target(target.to_path_buf())
    } else {
        error
    }
}

/// Attaches a PWD snapshot when path binding actually required one.
fn with_current_directory(error: LocalFileError, current_directory: Option<&Path>) -> LocalFileError {
    match current_directory {
        Some(current_directory) => error.with_current_directory(current_directory.to_path_buf()),
        None => error,
    }
}

/// Selects the public path for an operation that may partially publish.
fn operation_failure_path(error: &LocalFileError, scope: LocalFileSystemScope, fallback: &Path) -> PathBuf {
    if error.kind() != LocalFileErrorKind::PublicationIncomplete {
        return fallback.to_path_buf();
    }
    let Some(path) = error.path() else {
        return fallback.to_path_buf();
    };
    match scope {
        LocalFileSystemScope::Host => path.to_path_buf(),
        LocalFileSystemScope::Rooted => {
            let mut public = PathBuf::from(std::path::MAIN_SEPARATOR_STR);
            public.push(path);
            public
        }
    }
}
