// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Stateful Host or Rooted local filesystem service.
// qubit-style: allow source-test-pair

mod capability;
mod configuration;
mod copy;
mod directory;
mod io;
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

/// Synchronous local filesystem with per-instance PWD and operation defaults.
///
/// Clones share only immutable authority state. Each clone receives an
/// independent snapshot of the PWD, symlink policy, and all default Options.
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
    /// Normalized namespace-absolute current directory.
    current_directory: PathBuf,
    /// Default symlink policy used by operations without an override.
    symlink_policy: LocalSymlinkPolicy,
    /// Per-instance operation defaults, copied by value on clone.
    defaults: LocalFileSystemDefaults,
}

impl LocalFileSystem {
    /// Captures the process current directory and creates a Host filesystem.
    pub fn host() -> LocalResult<Self> {
        let current_directory = std::env::current_dir().map_err(|error| {
            LocalFileError::from_io(LocalFileOperation::Configure, None, None, error)
                .with_reason("failed to capture the Host filesystem current directory")
        })?;
        LocalPathResolver::new(LocalFileSystemScope::Host, &current_directory)?;
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
            current_directory,
            symlink_policy: LocalSymlinkPolicy::FollowAcrossScope,
            defaults: LocalFileSystemDefaults::default(),
        })
    }

    /// Opens one descriptor- or handle-authoritative Rooted filesystem.
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
            current_directory: PathBuf::from(std::path::MAIN_SEPARATOR_STR),
            symlink_policy: LocalSymlinkPolicy::FollowWithinScope,
            defaults: LocalFileSystemDefaults::default(),
        })
    }

    /// Creates a resolver from one operation's PWD snapshot.
    fn resolver(&self) -> LocalPathResolver {
        LocalPathResolver::new(self.scope(), &self.current_directory)
            .expect("LocalFileSystem stores a normalized namespace-absolute PWD")
    }

    /// Resolves one operation path with stable public error context.
    fn resolve(&self, path: &Path, operation: LocalFileOperation) -> LocalResult<LocalNamespacePath> {
        resolve_operation_path(&self.resolver(), path, operation)
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
        .map_err(|error| {
            operation_error(
                error,
                operation,
                path.namespace_absolute(),
                None,
                self.current_directory(),
            )
        })?;
        if metadata.kind() == LocalFileKind::Directory {
            return Ok(());
        }
        Err(LocalFileError::new(LocalFileErrorKind::NotDirectory, operation)
            .with_path(path.namespace_absolute().to_path_buf())
            .with_current_directory(self.current_directory.clone()))
    }

    /// Reports whether a path denotes the protected Rooted virtual root.
    fn is_root_operand(&self, path: &LocalNamespacePath) -> bool {
        self.scope() == LocalFileSystemScope::Rooted && path.authority_relative().as_os_str().is_empty()
    }

    /// Rejects an operation that may remove or replace the Rooted virtual root.
    fn reject_root_operand(&self, path: &LocalNamespacePath, operation: LocalFileOperation) -> LocalResult<()> {
        if !self.is_root_operand(path) {
            return Ok(());
        }
        Err(LocalFileError::new(LocalFileErrorKind::InvalidPath, operation)
            .with_reason("the Rooted virtual root cannot be removed or replaced")
            .with_path(path.namespace_absolute().to_path_buf())
            .with_current_directory(self.current_directory.clone()))
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
        error
            .with_operation(operation)
            .with_path(path.to_path_buf())
            .with_current_directory(resolver.current_directory().to_path_buf())
    })
}

/// Rewrites a backend error into the public namespace coordinate system.
fn operation_error(
    error: LocalFileError,
    operation: LocalFileOperation,
    path: &Path,
    target: Option<&Path>,
    current_directory: &Path,
) -> LocalFileError {
    let error = error
        .with_operation(operation)
        .with_path(path.to_path_buf())
        .with_current_directory(current_directory.to_path_buf());
    if let Some(target) = target {
        error.with_target(target.to_path_buf())
    } else {
        error
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
