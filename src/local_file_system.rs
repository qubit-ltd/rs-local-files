// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Stateful Host or Rooted local filesystem service.
// qubit-style: allow source-test-pair

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
use crate::LocalFileSystemLimits;
use crate::LocalFileSystemProtocols;
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
        let protocols = HostLocalFileSystem::protocols();
        Ok(Self {
            core: Arc::new(LocalFileSystemCore {
                namespace: LocalNamespace::Host,
                protocols,
                limits: LocalFileSystemLimits::new(crate::SizeLimit::VariesByPath, crate::SizeLimit::VariesByPath),
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
        let protocols = rooted.protocols();
        let limits = rooted.limits();
        Ok(Self {
            core: Arc::new(LocalFileSystemCore {
                namespace: LocalNamespace::Rooted(rooted),
                protocols,
                limits,
                #[cfg(feature = "test-support")]
                test_faults: None,
            }),
            current_directory: PathBuf::from(std::path::MAIN_SEPARATOR_STR),
            symlink_policy: LocalSymlinkPolicy::FollowWithinScope,
            defaults: LocalFileSystemDefaults::default(),
        })
    }

    /// Returns the namespace kind interpreted by this instance.
    #[inline]
    pub fn scope(&self) -> LocalFileSystemScope {
        match &self.core.namespace {
            LocalNamespace::Host => LocalFileSystemScope::Host,
            LocalNamespace::Rooted(_) => LocalFileSystemScope::Rooted,
        }
    }

    /// Returns this instance's normalized namespace-absolute PWD.
    #[inline]
    pub fn current_directory(&self) -> &Path {
        &self.current_directory
    }

    /// Changes this instance's PWD after resolving and validating a directory.
    pub fn set_current_directory(&mut self, path: &Path) -> LocalResult<()> {
        let resolver = self.resolver();
        let resolved = resolve_operation_path(&resolver, path, LocalFileOperation::SetCurrentDirectory)?;
        self.validate_directory(&resolved).map_err(|error| {
            operation_error(
                error,
                LocalFileOperation::SetCurrentDirectory,
                resolved.namespace_absolute(),
                None,
                self.current_directory(),
            )
        })?;
        self.current_directory = resolved.namespace_absolute().to_path_buf();
        Ok(())
    }

    /// Returns the default symlink policy inherited by operations.
    #[cfg_attr(feature = "test-support", inline(never))]

    pub const fn symlink_policy(&self) -> LocalSymlinkPolicy {
        self.symlink_policy
    }

    /// Changes this instance's default symlink policy transactionally.
    pub fn set_symlink_policy(&mut self, policy: LocalSymlinkPolicy) -> LocalResult<()> {
        validate_scope_symlink_policy(self.scope(), policy, LocalFileOperation::Configure, None)
            .map_err(|error| error.with_current_directory(self.current_directory.clone()))?;
        self.symlink_policy = policy;
        Ok(())
    }

    /// Returns the construction-time Rooted path used only for diagnostics.
    #[cfg_attr(feature = "test-support", inline(never))]

    pub fn diagnostic_root(&self) -> Option<&Path> {
        match &self.core.namespace {
            LocalNamespace::Host => None,
            LocalNamespace::Rooted(rooted) => Some(rooted.diagnostic_path()),
        }
    }

    /// Returns the immutable protocol snapshot for this authority.
    #[cfg_attr(feature = "test-support", inline(never))]

    pub fn protocols(&self) -> LocalFileSystemProtocols {
        self.core.protocols
    }

    /// Returns authority-level objective path-limit observations.
    #[cfg_attr(feature = "test-support", inline(never))]

    pub fn limits(&self) -> LocalFileSystemLimits {
        self.core.limits
    }

    /// Observes path limits at the requested path or nearest existing ancestor.
    pub fn limits_at(&self, path: &Path) -> LocalResult<LocalFileSystemLimits> {
        let resolved = self.resolve(path, LocalFileOperation::Capabilities)?;
        match &self.core.namespace {
            LocalNamespace::Host => host_probe(&resolved, self.symlink_policy, crate::capability::probe_limits)
                .map_err(|error| {
                    operation_error(
                        error,
                        LocalFileOperation::Capabilities,
                        resolved.namespace_absolute(),
                        None,
                        self.current_directory(),
                    )
                }),
            LocalNamespace::Rooted(rooted) => rooted
                .limits_at(resolved.authority_relative(), self.symlink_policy)
                .map_err(|error| {
                    operation_error(
                        error,
                        LocalFileOperation::Capabilities,
                        resolved.namespace_absolute(),
                        None,
                        self.current_directory(),
                    )
                }),
        }
    }

    /// Observes dynamic filesystem capacity at a path or existing ancestor.
    pub fn space_at(&self, path: &Path) -> LocalResult<LocalFileSystemSpace> {
        let resolved = self.resolve(path, LocalFileOperation::Capabilities)?;
        match &self.core.namespace {
            LocalNamespace::Host => {
                host_probe(&resolved, self.symlink_policy, crate::capability::probe_space).map_err(|error| {
                    operation_error(
                        error,
                        LocalFileOperation::Capabilities,
                        resolved.namespace_absolute(),
                        None,
                        self.current_directory(),
                    )
                })
            }
            LocalNamespace::Rooted(rooted) => rooted
                .space_at(resolved.authority_relative(), self.symlink_policy)
                .map_err(|error| {
                    operation_error(
                        error,
                        LocalFileOperation::Capabilities,
                        resolved.namespace_absolute(),
                        None,
                        self.current_directory(),
                    )
                }),
        }
    }

    /// Returns the default reader options.
    #[cfg_attr(feature = "test-support", inline(never))]

    pub const fn default_read_options(&self) -> &LocalReadOptions {
        &self.defaults.read
    }

    /// Replaces the default reader options.
    pub fn set_default_read_options(&mut self, options: LocalReadOptions) -> LocalResult<()> {
        self.defaults.read = options;
        Ok(())
    }

    /// Returns the default writer options.
    #[cfg_attr(feature = "test-support", inline(never))]

    pub const fn default_write_options(&self) -> &LocalWriteOptions {
        &self.defaults.write
    }

    /// Replaces the default writer options.
    pub fn set_default_write_options(&mut self, options: LocalWriteOptions) -> LocalResult<()> {
        self.defaults.write = options;
        Ok(())
    }

    /// Returns the default listing options.
    #[cfg_attr(feature = "test-support", inline(never))]

    pub const fn default_list_options(&self) -> &LocalListOptions {
        &self.defaults.list
    }

    /// Replaces the default listing options after structural validation.
    pub fn set_default_list_options(&mut self, options: LocalListOptions) -> LocalResult<()> {
        validate_list_options(self.scope(), self.symlink_policy, &options, None)
            .map_err(|error| error.with_current_directory(self.current_directory.clone()))?;
        self.defaults.list = options;
        Ok(())
    }

    /// Returns the default copy options.
    #[cfg_attr(feature = "test-support", inline(never))]

    pub const fn default_copy_options(&self) -> &LocalCopyOptions {
        &self.defaults.copy
    }

    /// Replaces the default copy options after structural validation.
    pub fn set_default_copy_options(&mut self, options: LocalCopyOptions) -> LocalResult<()> {
        validate_copy_options(self.scope(), self.symlink_policy, &options, None, None)
            .map_err(|error| error.with_current_directory(self.current_directory.clone()))?;
        self.defaults.copy = options;
        Ok(())
    }

    /// Returns the default directory-creation options.
    #[cfg_attr(feature = "test-support", inline(never))]

    pub const fn default_create_directory_options(&self) -> &LocalCreateDirectoryOptions {
        &self.defaults.create_directory
    }

    /// Replaces the default directory-creation options.
    pub fn set_default_create_directory_options(&mut self, options: LocalCreateDirectoryOptions) -> LocalResult<()> {
        self.defaults.create_directory = options;
        Ok(())
    }

    /// Returns the default deletion options.
    #[cfg_attr(feature = "test-support", inline(never))]

    pub const fn default_delete_options(&self) -> &LocalDeleteOptions {
        &self.defaults.delete
    }

    /// Replaces the default deletion options.
    pub fn set_default_delete_options(&mut self, options: LocalDeleteOptions) -> LocalResult<()> {
        self.defaults.delete = options;
        Ok(())
    }

    /// Returns the default rename options.
    #[cfg_attr(feature = "test-support", inline(never))]

    pub const fn default_rename_options(&self) -> &LocalRenameOptions {
        &self.defaults.rename
    }

    /// Replaces the default rename options.
    pub fn set_default_rename_options(&mut self, options: LocalRenameOptions) -> LocalResult<()> {
        self.defaults.rename = options;
        Ok(())
    }

    /// Returns the default temporary-file options.
    #[cfg_attr(feature = "test-support", inline(never))]

    pub const fn default_temp_file_options(&self) -> &LocalTempFileOptions {
        &self.defaults.temp_file
    }

    /// Replaces the default temporary-file options after validation.
    pub fn set_default_temp_file_options(&mut self, options: LocalTempFileOptions) -> LocalResult<()> {
        validate_temp_attempts(options.max_attempts(), LocalFileOperation::Configure)
            .map_err(|error| error.with_current_directory(self.current_directory.clone()))?;
        self.defaults.temp_file = options;
        Ok(())
    }

    /// Returns the default temporary-directory options.
    #[cfg_attr(feature = "test-support", inline(never))]

    pub const fn default_temp_directory_options(&self) -> &LocalTempDirectoryOptions {
        &self.defaults.temp_directory
    }

    /// Replaces the default temporary-directory options after validation.
    pub fn set_default_temp_directory_options(&mut self, options: LocalTempDirectoryOptions) -> LocalResult<()> {
        validate_temp_attempts(options.max_attempts(), LocalFileOperation::Configure)
            .map_err(|error| error.with_current_directory(self.current_directory.clone()))?;
        self.defaults.temp_directory = options;
        Ok(())
    }

    /// Reads final-entry metadata without following the final symlink.
    pub fn metadata(&self, path: &Path) -> LocalResult<LocalFileMetadata> {
        self.core
            .fail_if_requested(crate::test_support::TestFaultPoint::Metadata)
            .map_err(|error| {
                LocalFileError::from_io(LocalFileOperation::Metadata, Some(path.to_path_buf()), None, error)
                    .with_current_directory(self.current_directory.clone())
            })?;
        let resolved = self.resolve(path, LocalFileOperation::Metadata)?;
        let metadata = match &self.core.namespace {
            LocalNamespace::Host => {
                HostLocalFileSystem::metadata_with_policy(resolved.authority_relative(), self.symlink_policy)
            }
            LocalNamespace::Rooted(rooted) => rooted.metadata(resolved.authority_relative(), self.symlink_policy),
        }
        .map_err(|error| {
            operation_error(
                error,
                LocalFileOperation::Metadata,
                resolved.namespace_absolute(),
                None,
                self.current_directory(),
            )
        })?;
        if resolved.directory_required() && metadata.kind() != LocalFileKind::Directory {
            return Err(
                LocalFileError::new(LocalFileErrorKind::NotDirectory, LocalFileOperation::Metadata)
                    .with_path(resolved.namespace_absolute().to_path_buf())
                    .with_current_directory(self.current_directory.clone()),
            );
        }
        Ok(metadata)
    }

    /// Opens a reader using this instance's default reader options.
    pub fn open_reader(&self, path: &Path) -> LocalResult<LocalFileReader> {
        self.open_reader_with_options(path, &self.defaults.read)
    }

    /// Opens a reader using one complete explicit options value.
    pub fn open_reader_with_options(&self, path: &Path, options: &LocalReadOptions) -> LocalResult<LocalFileReader> {
        let resolved = self.resolve(path, LocalFileOperation::OpenReader)?;
        reject_directory_qualified_file(&resolved, LocalFileOperation::OpenReader, self.current_directory())?;
        match &self.core.namespace {
            LocalNamespace::Host => HostLocalFileSystem::open_reader_with_policy(
                resolved.authority_relative(),
                options,
                self.symlink_policy,
            ),
            LocalNamespace::Rooted(rooted) => {
                rooted.open_reader(resolved.authority_relative(), options, self.symlink_policy)
            }
        }
        .map_err(|error| {
            operation_error(
                error,
                LocalFileOperation::OpenReader,
                resolved.namespace_absolute(),
                None,
                self.current_directory(),
            )
        })
    }

    /// Reads at most `max_bytes` using the default reader options.
    pub fn read_prefix(&self, path: &Path, max_bytes: usize) -> LocalResult<Vec<u8>> {
        self.read_prefix_with_options(path, max_bytes, &self.defaults.read)
    }

    /// Reads at most `max_bytes` using one complete explicit options value.
    pub fn read_prefix_with_options(
        &self,
        path: &Path,
        max_bytes: usize,
        options: &LocalReadOptions,
    ) -> LocalResult<Vec<u8>> {
        let error_path = self
            .resolve(path, LocalFileOperation::Read)?
            .namespace_absolute()
            .to_path_buf();
        let mut reader = self.open_reader_with_options(path, options)?;
        if max_bytes == 0 {
            return Ok(Vec::new());
        }
        let mut result = Vec::with_capacity(max_bytes.min(8192));
        let mut buffer = [0_u8; 8192];
        while result.len() < max_bytes {
            let read_len = (max_bytes - result.len()).min(buffer.len());
            #[cfg(feature = "internal-test-support")]
            if crate::local::take_test_support("local-fs-read-prefix-read") {
                return Err(LocalFileError::from_io(
                    LocalFileOperation::Read,
                    Some(error_path),
                    None,
                    std::io::Error::other("injected prefix read failure"),
                )
                .with_current_directory(self.current_directory.clone()));
            }
            let count = reader.read(&mut buffer[..read_len]).map_err(|source| {
                LocalFileError::from_io(LocalFileOperation::Read, Some(error_path.clone()), None, source)
                    .with_current_directory(self.current_directory.clone())
            })?;
            if count == 0 {
                break;
            }
            result.extend_from_slice(&buffer[..count]);
        }
        Ok(result)
    }

    /// Opens a writer using this instance's default writer options.
    pub fn open_writer(&self, path: &Path) -> LocalResult<LocalFileWriter> {
        self.open_writer_with_options(path, &self.defaults.write)
    }

    /// Opens a writer using one complete explicit options value.
    pub fn open_writer_with_options(&self, path: &Path, options: &LocalWriteOptions) -> LocalResult<LocalFileWriter> {
        let resolved = self.resolve(path, LocalFileOperation::OpenWriter)?;
        self.reject_root_operand(&resolved, LocalFileOperation::OpenWriter)?;
        reject_directory_qualified_file(&resolved, LocalFileOperation::OpenWriter, self.current_directory())?;
        match &self.core.namespace {
            LocalNamespace::Host => HostLocalFileSystem::open_writer_with_policy(
                resolved.authority_relative(),
                options,
                self.symlink_policy,
            ),
            LocalNamespace::Rooted(rooted) => {
                rooted.open_writer(resolved.authority_relative(), options, self.symlink_policy)
            }
        }
        .map(|writer| {
            writer.bind_namespace(
                resolved.namespace_absolute().to_path_buf(),
                self.current_directory.clone(),
            )
        })
        .map_err(|error| {
            operation_error(
                error,
                LocalFileOperation::OpenWriter,
                resolved.namespace_absolute(),
                None,
                self.current_directory(),
            )
        })
    }

    /// Opens a walker using this instance's default listing options.
    pub fn list(&self, path: &Path) -> LocalResult<LocalDirectoryWalker> {
        self.list_with_options(path, &self.defaults.list)
    }

    /// Opens a walker using one complete explicit options value.
    pub fn list_with_options(&self, path: &Path, options: &LocalListOptions) -> LocalResult<LocalDirectoryWalker> {
        let resolved = self.resolve(path, LocalFileOperation::List)?;
        validate_list_options(
            self.scope(),
            self.symlink_policy,
            options,
            Some(resolved.namespace_absolute()),
        )
        .map_err(|error| error.with_current_directory(self.current_directory.clone()))?;
        match &self.core.namespace {
            LocalNamespace::Host => {
                HostLocalFileSystem::list_with_policy(resolved.authority_relative(), options, self.symlink_policy)
            }
            LocalNamespace::Rooted(rooted) => rooted.list(
                resolved.authority_relative(),
                resolved.namespace_absolute(),
                options,
                self.symlink_policy,
            ),
        }
        .map(|walker| walker.bind_current_directory(self.current_directory.clone()))
        .map_err(|error| {
            operation_error(
                error,
                LocalFileOperation::List,
                resolved.namespace_absolute(),
                None,
                self.current_directory(),
            )
        })
    }

    /// Copies an entry using this instance's default copy options.
    pub fn copy(&self, source: &Path, destination: &Path) -> LocalCopyResult {
        self.copy_with_options(source, destination, &self.defaults.copy)
    }

    /// Copies an entry using one complete explicit options value.
    #[allow(clippy::result_large_err)]
    pub fn copy_with_options(&self, source: &Path, destination: &Path, options: &LocalCopyOptions) -> LocalCopyResult {
        let started_at = Instant::now();
        let request_source = source;
        let request_destination = destination;
        validate_copy_options(
            self.scope(),
            self.symlink_policy,
            options,
            Some(request_source),
            Some(request_destination),
        )
        .map_err(|error| {
            copy_failure_unchanged(
                error
                    .with_path(request_source.to_path_buf())
                    .with_target(request_destination.to_path_buf())
                    .with_current_directory(self.current_directory.clone()),
            )
        })?;
        let resolver = self.resolver();
        let source = resolve_operation_path(&resolver, request_source, LocalFileOperation::Copy)
            .map_err(|error| copy_failure_unchanged(error.with_target(request_destination.to_path_buf())))?;
        let destination =
            resolve_operation_path(&resolver, request_destination, LocalFileOperation::Copy).map_err(|error| {
                copy_failure_unchanged(
                    error
                        .with_path(request_source.to_path_buf())
                        .with_target(request_destination.to_path_buf()),
                )
            })?;
        self.reject_root_operand(&source, LocalFileOperation::Copy)
            .map_err(|error| {
                copy_failure_unchanged(error.with_target(destination.namespace_absolute().to_path_buf()))
            })?;
        self.reject_root_operand(&destination, LocalFileOperation::Copy)
            .map_err(|error| {
                copy_failure_unchanged(
                    error
                        .with_path(source.namespace_absolute().to_path_buf())
                        .with_target(destination.namespace_absolute().to_path_buf()),
                )
            })?;
        let directory_qualified = source.directory_required() || destination.directory_required();
        let directory_options;
        let options = if directory_qualified {
            match options.source_mode() {
                crate::LocalCopySourceMode::Auto => {
                    directory_options = (*options).with_tree_source();
                    &directory_options
                }
                crate::LocalCopySourceMode::Tree => options,
                crate::LocalCopySourceMode::File => {
                    return Err(copy_failure_unchanged(
                        LocalFileError::new(LocalFileErrorKind::NotDirectory, LocalFileOperation::Copy)
                            .with_reason("directory-qualified copy paths are incompatible with file source mode")
                            .with_path(source.namespace_absolute().to_path_buf())
                            .with_target(destination.namespace_absolute().to_path_buf())
                            .with_current_directory(self.current_directory.clone()),
                    ));
                }
            }
        } else {
            options
        };
        let result = match &self.core.namespace {
            LocalNamespace::Host => HostLocalFileSystem::copy_with_policy(
                source.authority_relative(),
                destination.authority_relative(),
                options,
                self.symlink_policy,
                started_at,
            ),
            LocalNamespace::Rooted(rooted) => rooted.copy(
                source.authority_relative(),
                destination.authority_relative(),
                options,
                self.symlink_policy,
                started_at,
            ),
        };
        result.map_err(|failure| {
            failure.remap_namespace(
                source.namespace_absolute(),
                destination.namespace_absolute(),
                source.authority_relative(),
                destination.authority_relative(),
                self.scope() == LocalFileSystemScope::Rooted,
                resolver.current_directory(),
            )
        })
    }

    /// Creates a directory using this instance's default options.
    pub fn create_directory(&self, path: &Path) -> LocalResult<LocalCreateDirectoryOutcome> {
        self.create_directory_with_options(path, &self.defaults.create_directory)
    }

    /// Creates a directory using one complete explicit options value.
    pub fn create_directory_with_options(
        &self,
        path: &Path,
        options: &LocalCreateDirectoryOptions,
    ) -> LocalResult<LocalCreateDirectoryOutcome> {
        let resolved = self.resolve(path, LocalFileOperation::CreateDirectory)?;
        if self.is_root_operand(&resolved) {
            if options.exists_ok() {
                return Ok(LocalCreateDirectoryOutcome::new(false));
            }
            return Err(
                LocalFileError::new(LocalFileErrorKind::AlreadyExists, LocalFileOperation::CreateDirectory)
                    .with_path(resolved.namespace_absolute().to_path_buf())
                    .with_current_directory(self.current_directory.clone()),
            );
        }
        match &self.core.namespace {
            LocalNamespace::Host => HostLocalFileSystem::create_directory_with_policy(
                resolved.authority_relative(),
                options,
                self.symlink_policy,
            ),
            LocalNamespace::Rooted(rooted) => {
                rooted.create_directory(resolved.authority_relative(), options, self.symlink_policy)
            }
        }
        .map_err(|error| {
            let path = operation_failure_path(&error, self.scope(), resolved.namespace_absolute());
            operation_error(
                error,
                LocalFileOperation::CreateDirectory,
                &path,
                None,
                self.current_directory(),
            )
        })
    }

    /// Deletes a non-directory entry using this instance's default options.
    pub fn delete_file(&self, path: &Path) -> LocalResult<LocalDeleteOutcome> {
        self.delete_file_with_options(path, &self.defaults.delete)
    }

    /// Deletes a non-directory entry using complete explicit options.
    pub fn delete_file_with_options(
        &self,
        path: &Path,
        options: &LocalDeleteOptions,
    ) -> LocalResult<LocalDeleteOutcome> {
        let resolved = self.resolve(path, LocalFileOperation::DeleteFile)?;
        self.reject_root_operand(&resolved, LocalFileOperation::DeleteFile)?;
        reject_directory_qualified_file(&resolved, LocalFileOperation::DeleteFile, self.current_directory())?;
        match &self.core.namespace {
            LocalNamespace::Host => HostLocalFileSystem::delete_file_with_policy(
                resolved.authority_relative(),
                options,
                self.symlink_policy,
            ),
            LocalNamespace::Rooted(rooted) => {
                rooted.delete_file(resolved.authority_relative(), options, self.symlink_policy)
            }
        }
        .map_err(|error| {
            operation_error(
                error,
                LocalFileOperation::DeleteFile,
                resolved.namespace_absolute(),
                None,
                self.current_directory(),
            )
        })
    }

    /// Deletes a directory using this instance's default options.
    pub fn delete_directory(&self, path: &Path) -> LocalResult<LocalDeleteOutcome> {
        self.delete_directory_with_options(path, &self.defaults.delete)
    }

    /// Deletes a directory using complete explicit options.
    pub fn delete_directory_with_options(
        &self,
        path: &Path,
        options: &LocalDeleteOptions,
    ) -> LocalResult<LocalDeleteOutcome> {
        let resolved = self.resolve(path, LocalFileOperation::DeleteDirectory)?;
        self.reject_root_operand(&resolved, LocalFileOperation::DeleteDirectory)?;
        match &self.core.namespace {
            LocalNamespace::Host => HostLocalFileSystem::delete_directory_with_policy(
                resolved.authority_relative(),
                options,
                self.symlink_policy,
            ),
            LocalNamespace::Rooted(rooted) => {
                rooted.delete_directory(resolved.authority_relative(), options, self.symlink_policy)
            }
        }
        .map_err(|error| {
            let path = operation_failure_path(&error, self.scope(), resolved.namespace_absolute());
            operation_error(
                error,
                LocalFileOperation::DeleteDirectory,
                &path,
                None,
                self.current_directory(),
            )
        })
    }

    /// Renames an entry using this instance's default options.
    pub fn rename(&self, source: &Path, destination: &Path) -> LocalRenameResult {
        self.rename_with_options(source, destination, &self.defaults.rename)
    }

    /// Renames an entry using one complete explicit options value.
    #[allow(clippy::result_large_err)]
    pub fn rename_with_options(
        &self,
        source: &Path,
        destination: &Path,
        options: &LocalRenameOptions,
    ) -> LocalRenameResult {
        let request_source = source;
        let request_destination = destination;
        let resolver = self.resolver();
        let source = resolve_operation_path(&resolver, request_source, LocalFileOperation::Rename)
            .map_err(|error| rename_failure_unchanged(error.with_target(request_destination.to_path_buf())))?;
        let destination =
            resolve_operation_path(&resolver, request_destination, LocalFileOperation::Rename).map_err(|error| {
                rename_failure_unchanged(
                    error
                        .with_path(request_source.to_path_buf())
                        .with_target(request_destination.to_path_buf()),
                )
            })?;
        self.reject_root_operand(&source, LocalFileOperation::Rename)
            .map_err(|error| {
                rename_failure_unchanged(error.with_target(destination.namespace_absolute().to_path_buf()))
            })?;
        self.reject_root_operand(&destination, LocalFileOperation::Rename)
            .map_err(|error| {
                rename_failure_unchanged(
                    error
                        .with_path(source.namespace_absolute().to_path_buf())
                        .with_target(destination.namespace_absolute().to_path_buf()),
                )
            })?;
        self.validate_directory_requirement(&source, LocalFileOperation::Rename)
            .map_err(|error| {
                rename_failure_unchanged(error.with_target(destination.namespace_absolute().to_path_buf()))
            })?;
        self.validate_directory_requirement(&destination, LocalFileOperation::Rename)
            .map_err(|error| {
                rename_failure_unchanged(
                    error
                        .with_path(source.namespace_absolute().to_path_buf())
                        .with_target(destination.namespace_absolute().to_path_buf()),
                )
            })?;
        let result = match &self.core.namespace {
            LocalNamespace::Host => HostLocalFileSystem::rename_with_policy(
                source.authority_relative(),
                destination.authority_relative(),
                options,
                self.symlink_policy,
            ),
            LocalNamespace::Rooted(rooted) => rooted.rename(
                source.authority_relative(),
                destination.authority_relative(),
                options,
                self.symlink_policy,
            ),
        };
        result.map_err(|failure| {
            failure.remap_namespace(
                source.namespace_absolute(),
                destination.namespace_absolute(),
                resolver.current_directory(),
            )
        })
    }

    /// Creates a temporary file using this instance's default options.
    pub fn create_temp_file(&self) -> LocalResult<LocalTempFile> {
        self.create_temp_file_with_options(&self.defaults.temp_file)
    }

    /// Creates a temporary file using one complete explicit options value.
    pub fn create_temp_file_with_options(&self, options: &LocalTempFileOptions) -> LocalResult<LocalTempFile> {
        validate_temp_attempts(options.max_attempts(), LocalFileOperation::CreateTempFile)
            .map_err(|error| error.with_current_directory(self.current_directory.clone()))?;
        let parent = options.parent().unwrap_or_else(|| Path::new(""));
        let resolver = self.resolver();
        let resolved = resolve_operation_path(&resolver, parent, LocalFileOperation::CreateTempFile)?;
        let options = options.clone().with_parent(resolved.authority_relative());
        let resource = match &self.core.namespace {
            LocalNamespace::Host => HostLocalFileSystem::create_temp_file_with_policy(&options, self.symlink_policy),
            LocalNamespace::Rooted(rooted) => rooted.create_temp_file(&options, self.symlink_policy),
        }
        .map_err(|error| {
            operation_error(
                error,
                LocalFileOperation::CreateTempFile,
                resolved.namespace_absolute(),
                None,
                self.current_directory(),
            )
        })?;
        resource.bind_namespace(resolver)
    }

    /// Creates a temporary directory using this instance's default options.
    pub fn create_temp_directory(&self) -> LocalResult<LocalTempDirectory> {
        self.create_temp_directory_with_options(&self.defaults.temp_directory)
    }

    /// Creates a temporary directory using one complete explicit options value.
    pub fn create_temp_directory_with_options(
        &self,
        options: &LocalTempDirectoryOptions,
    ) -> LocalResult<LocalTempDirectory> {
        validate_temp_attempts(options.max_attempts(), LocalFileOperation::CreateTempDirectory)
            .map_err(|error| error.with_current_directory(self.current_directory.clone()))?;
        let parent = options.parent().unwrap_or_else(|| Path::new(""));
        let resolver = self.resolver();
        let resolved = resolve_operation_path(&resolver, parent, LocalFileOperation::CreateTempDirectory)?;
        let options = options.clone().with_parent(resolved.authority_relative());
        let resource = match &self.core.namespace {
            LocalNamespace::Host => {
                HostLocalFileSystem::create_temp_directory_with_policy(&options, self.symlink_policy)
            }
            LocalNamespace::Rooted(rooted) => rooted.create_temp_directory(&options, self.symlink_policy),
        }
        .map_err(|error| {
            operation_error(
                error,
                LocalFileOperation::CreateTempDirectory,
                resolved.namespace_absolute(),
                None,
                self.current_directory(),
            )
        })?;
        resource.bind_namespace(resolver)
    }

    /// Installs an instance-local fault plan in test-support builds.
    #[cfg(feature = "test-support")]
    #[doc(hidden)]
    #[must_use]
    #[inline]
    pub fn with_test_faults(mut self, test_faults: Option<crate::TestFaultPlan>) -> Self {
        self.core = Arc::new(LocalFileSystemCore {
            namespace: self.core.namespace.clone(),
            protocols: self.core.protocols,
            limits: self.core.limits,
            test_faults,
        });
        self
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

/// Probes the nearest existing Host path after applying symlink policy.
fn host_probe<T>(
    path: &LocalNamespacePath,
    symlink_policy: LocalSymlinkPolicy,
    probe: fn(&fs::File) -> T,
) -> LocalResult<T> {
    let mut candidate = crate::local::resolve_host_path(path.authority_relative(), symlink_policy, true)?;
    loop {
        match open_host_probe(&candidate) {
            Ok(file) => return Ok(probe(&file)),
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                if !candidate.pop() {
                    return Err(LocalFileError::from_io(
                        LocalFileOperation::Capabilities,
                        Some(path.namespace_absolute().to_path_buf()),
                        None,
                        error,
                    ));
                }
            }
            Err(error) => {
                return Err(LocalFileError::from_io(
                    LocalFileOperation::Capabilities,
                    Some(path.namespace_absolute().to_path_buf()),
                    None,
                    error,
                ));
            }
        }
    }
}

/// Opens a Host file or directory for handle-based capability probing.
fn open_host_probe(path: &Path) -> std::io::Result<fs::File> {
    if fs::metadata(path)?.is_dir() {
        crate::local::open_root_directory(path)
    } else {
        fs::File::open(path)
    }
}
