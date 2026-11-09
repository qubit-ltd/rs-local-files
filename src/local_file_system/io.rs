// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Stateful Host or Rooted local filesystem service.

use super::HostLocalFileSystem;
use super::LocalFileError;
use super::LocalFileErrorKind;
use super::LocalFileKind;
use super::LocalFileMetadata;
use super::LocalFileOperation;
use super::LocalFileReader;
use super::LocalFileSystem;
use super::LocalFileWriter;
use super::LocalNamespace;
use super::LocalReadOptions;
use super::LocalResult;
use super::LocalWriteOptions;
use super::Path;
use super::operation_error;
use super::reject_directory_qualified_file;
use super::resolve_operation_path;
use super::validate_scope_symlink_policy;
use super::with_current_directory;
use crate::LocalSymlinkPolicy;
use crate::local_file_system_validation::validate_write_options;
use crate::path::LocalNamespacePath;
use crate::test_support::TestFaultPoint;

impl LocalFileSystem {
    /// Opens a reader using this instance's default reader options.
    ///
    /// Returns the owned reader or error described by
    /// [`Self::open_reader_with_options`].
    ///
    /// # Parameters
    ///
    /// - `path`: Namespace path of the regular file to open.
    ///
    /// # Returns
    ///
    /// An owned reader for the opened regular file.
    ///
    /// # Errors
    ///
    /// Returns path-resolution, policy, type, or native opening errors.
    pub fn open_reader(&self, path: &Path) -> LocalResult<LocalFileReader> {
        self.open_reader_with_options(path, &self.defaults.read)
    }

    /// Opens a reader using one complete explicit options value.
    ///
    /// Requires a regular file after applying this instance's symlink policy.
    /// Returns path-resolution, type, or native open errors, including retry
    /// timeout failures. The returned reader owns the opened native handle.
    ///
    /// # Parameters
    ///
    /// - `path`: Namespace path of the regular file to open.
    /// - `options`: Complete reader policy for this operation.
    ///
    /// # Returns
    ///
    /// An owned reader for the opened regular file.
    ///
    /// # Errors
    ///
    /// Returns path-resolution, policy, type, timeout, or native opening
    /// errors.
    pub fn open_reader_with_options(&self, path: &Path, options: &LocalReadOptions) -> LocalResult<LocalFileReader> {
        let symlink_policy = self.read_symlink_policy(options, None);
        validate_scope_symlink_policy(self.scope(), symlink_policy, LocalFileOperation::OpenReader, Some(path))?;
        let resolver = self.resolver_for(path, LocalFileOperation::OpenReader)?;
        let resolved = resolve_operation_path(&resolver, path, LocalFileOperation::OpenReader)?;
        self.open_resolved_reader(
            &resolved,
            options,
            LocalFileOperation::OpenReader,
            resolver.current_directory(),
        )
    }

    /// Opens a writer using this instance's default writer options.
    ///
    /// Returns the owned writer or error described by
    /// [`Self::open_writer_with_options`].
    ///
    /// # Parameters
    ///
    /// - `path`: Namespace path of the file to create or append.
    ///
    /// # Returns
    ///
    /// An owned writer configured with this instance's default options.
    ///
    /// # Errors
    ///
    /// Returns path-resolution, validation, or native preparation errors.
    pub fn open_writer(&self, path: &Path) -> LocalResult<LocalFileWriter> {
        self.open_writer_with_options(path, &self.defaults.write)
    }

    /// Opens a writer using one complete explicit options value.
    ///
    /// Create modes return an unpublished staging writer; append opens an
    /// existing regular file directly. Returns path, type, requirement, or
    /// native preparation errors. Requested parent creation may remain after
    /// an error. Commit or abort a staging writer explicitly when its final
    /// publication or cleanup result must be observed.
    ///
    /// # Parameters
    ///
    /// - `path`: Namespace path of the file to create or append.
    /// - `options`: Complete writer policy for this operation.
    ///
    /// # Returns
    ///
    /// An owned writer configured with `options`.
    ///
    /// # Errors
    ///
    /// Returns path-resolution, validation, or native preparation errors.
    pub fn open_writer_with_options(&self, path: &Path, options: &LocalWriteOptions) -> LocalResult<LocalFileWriter> {
        validate_write_options(options, self.capabilities(), LocalFileOperation::OpenWriter).map_err(|error| {
            with_current_directory(
                error.with_path(path.to_path_buf()),
                self.current_directory.virtual_path(),
            )
        })?;
        let resolver = self.resolver_for(path, LocalFileOperation::OpenWriter)?;
        let resolved = resolve_operation_path(&resolver, path, LocalFileOperation::OpenWriter)?;
        self.reject_root_operand(&resolved, LocalFileOperation::OpenWriter, resolver.current_directory())?;
        reject_directory_qualified_file(&resolved, LocalFileOperation::OpenWriter, resolver.current_directory())?;
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
                resolver.current_directory().map(Path::to_path_buf),
            )
        })
        .map_err(|error| {
            operation_error(
                error,
                LocalFileOperation::OpenWriter,
                resolved.namespace_absolute(),
                None,
                resolver.current_directory(),
            )
        })
    }

    /// Reads metadata for the final target while honoring this filesystem's
    /// link policy.
    ///
    /// Returns file or directory metadata. Link rejection, missing targets,
    /// resolution errors, and native metadata failures are returned as
    /// `LocalFileError` with metadata-operation context.
    ///
    /// # Parameters
    ///
    /// - `path`: Namespace path whose final target metadata is requested.
    ///
    /// # Returns
    ///
    /// Metadata for the followed target entry.
    ///
    /// # Errors
    ///
    /// Returns resolution, symlink-policy, or native metadata errors.
    pub fn target_metadata(&self, path: &Path) -> LocalResult<LocalFileMetadata> {
        let resolver = self.resolver_for(path, LocalFileOperation::Metadata)?;
        let resolved = resolve_operation_path(&resolver, path, LocalFileOperation::Metadata)?;
        let metadata = match &self.core.namespace {
            LocalNamespace::Host => {
                let entry =
                    HostLocalFileSystem::metadata_with_policy(resolved.authority_relative(), self.symlink_policy)
                        .map_err(|error| {
                            operation_error(
                                error,
                                LocalFileOperation::Metadata,
                                resolved.namespace_absolute(),
                                None,
                                resolver.current_directory(),
                            )
                        })?;
                if entry.kind() == LocalFileKind::Symlink && !self.symlink_policy.follows() {
                    return Err(operation_error(
                        LocalFileError::new(LocalFileErrorKind::Unsupported, LocalFileOperation::Metadata)
                            .with_reason("target metadata requires following a symbolic link"),
                        LocalFileOperation::Metadata,
                        resolved.namespace_absolute(),
                        None,
                        resolver.current_directory(),
                    ));
                }
                if entry.kind() == LocalFileKind::Symlink && self.symlink_policy.follows() {
                    std::fs::metadata(resolved.authority_relative())
                        .map(|metadata| LocalFileMetadata::from_native(&metadata))
                        .map_err(|error| {
                            LocalFileError::from_io(
                                LocalFileOperation::Metadata,
                                Some(resolved.namespace_absolute().to_path_buf()),
                                None,
                                error,
                            )
                        })
                } else {
                    Ok(entry)
                }
            }
            LocalNamespace::Rooted(rooted) => {
                rooted.target_metadata(resolved.authority_relative(), self.symlink_policy)
            }
        }
        .map_err(|error| {
            operation_error(
                error,
                LocalFileOperation::Metadata,
                resolved.namespace_absolute(),
                None,
                resolver.current_directory(),
            )
        })?;
        if resolved.directory_required() && metadata.kind() != LocalFileKind::Directory {
            return Err(operation_error(
                LocalFileError::new(LocalFileErrorKind::NotDirectory, LocalFileOperation::Metadata),
                LocalFileOperation::Metadata,
                resolved.namespace_absolute(),
                None,
                resolver.current_directory(),
            ));
        }
        Ok(metadata)
    }

    /// Resolves an existing operand to its namespace coordinate.
    ///
    /// Host results are native canonical paths; Rooted results are virtual
    /// absolute paths and do not grant authority for a later operation.
    /// Returns path-policy, missing-target, or native resolution errors.
    ///
    /// # Parameters
    ///
    /// - `path`: Existing namespace path to resolve.
    ///
    /// # Returns
    ///
    /// The namespace coordinate of the existing operand.
    ///
    /// # Errors
    ///
    /// Returns path-policy, missing-target, or native resolution errors.
    pub fn canonicalize(&self, path: &Path) -> LocalResult<std::path::PathBuf> {
        let resolver = self.resolver_for(path, LocalFileOperation::Metadata)?;
        let resolved = resolve_operation_path(&resolver, path, LocalFileOperation::Metadata)?;
        let canonical = match &self.core.namespace {
            LocalNamespace::Host => {
                let entry =
                    HostLocalFileSystem::metadata_with_policy(resolved.authority_relative(), self.symlink_policy)
                        .map_err(|error| {
                            operation_error(
                                error,
                                LocalFileOperation::Metadata,
                                resolved.namespace_absolute(),
                                None,
                                resolver.current_directory(),
                            )
                        })?;
                if entry.kind() == LocalFileKind::Symlink && !self.symlink_policy.follows() {
                    return Err(operation_error(
                        LocalFileError::new(LocalFileErrorKind::Unsupported, LocalFileOperation::Metadata)
                            .with_reason("canonicalization requires following a symbolic link"),
                        LocalFileOperation::Metadata,
                        resolved.namespace_absolute(),
                        None,
                        resolver.current_directory(),
                    ));
                }
                std::fs::canonicalize(resolved.authority_relative()).map_err(|source| {
                    LocalFileError::from_io(
                        LocalFileOperation::Metadata,
                        Some(resolved.namespace_absolute().to_path_buf()),
                        None,
                        source,
                    )
                })
            }
            LocalNamespace::Rooted(rooted) => {
                rooted.canonicalize_virtual(resolved.authority_relative(), self.symlink_policy)
            }
        }
        .map_err(|error| {
            operation_error(
                error,
                LocalFileOperation::Metadata,
                resolved.namespace_absolute(),
                None,
                resolver.current_directory(),
            )
        })?;
        if resolved.directory_required() {
            let metadata = match &self.core.namespace {
                LocalNamespace::Host => self
                    .core
                    .fail_if_requested(TestFaultPoint::Metadata)
                    .and_then(|()| std::fs::metadata(resolved.authority_relative()))
                    .map(|metadata| LocalFileMetadata::from_native(&metadata))
                    .map_err(|error| {
                        LocalFileError::from_io(
                            LocalFileOperation::Metadata,
                            Some(resolved.namespace_absolute().to_path_buf()),
                            None,
                            error,
                        )
                    }),
                LocalNamespace::Rooted(rooted) => {
                    rooted.target_metadata(resolved.authority_relative(), self.symlink_policy)
                }
            }
            .map_err(|error| {
                operation_error(
                    error,
                    LocalFileOperation::Metadata,
                    resolved.namespace_absolute(),
                    None,
                    resolver.current_directory(),
                )
            })?;
            if metadata.kind() != LocalFileKind::Directory {
                return Err(operation_error(
                    LocalFileError::new(LocalFileErrorKind::NotDirectory, LocalFileOperation::Metadata),
                    LocalFileOperation::Metadata,
                    resolved.namespace_absolute(),
                    None,
                    resolver.current_directory(),
                ));
            }
        }
        Ok(canonical)
    }

    /// Reads final-entry metadata without following the final symlink.
    ///
    /// Returns resolution, policy, or native metadata errors. A
    /// directory-qualified operand returns `NotDirectory` unless the observed
    /// final entry is a real directory.
    ///
    /// # Parameters
    ///
    /// - `path`: Namespace path whose final entry metadata is requested.
    ///
    /// # Returns
    ///
    /// Metadata for the final directory entry without following its symlink.
    ///
    /// # Errors
    ///
    /// Returns resolution, symlink-policy, or native metadata errors.
    pub fn metadata(&self, path: &Path) -> LocalResult<LocalFileMetadata> {
        let resolver = self.resolver_for(path, LocalFileOperation::Metadata)?;
        let resolved = resolve_operation_path(&resolver, path, LocalFileOperation::Metadata)?;
        self.core.fail_if_requested(TestFaultPoint::Metadata).map_err(|error| {
            with_current_directory(
                LocalFileError::from_io(LocalFileOperation::Metadata, Some(path.to_path_buf()), None, error),
                resolver.current_directory(),
            )
        })?;
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
                resolver.current_directory(),
            )
        })?;
        if resolved.directory_required() && metadata.kind() != LocalFileKind::Directory {
            let error = LocalFileError::new(LocalFileErrorKind::NotDirectory, LocalFileOperation::Metadata)
                .with_path(resolved.namespace_absolute().to_path_buf());
            return Err(with_current_directory(error, resolver.current_directory()));
        }
        Ok(metadata)
    }

    /// Selects the outer override, then the reader option, then the instance
    /// default.
    ///
    /// # Parameters
    ///
    /// - `options`: Reader options whose symlink policy is the second choice.
    /// - `outer_override`: Higher-priority policy selected by the enclosing
    ///   operation.
    ///
    /// # Returns
    ///
    /// The first available policy in the order outer override, reader option,
    /// then this filesystem's default.
    #[inline]
    pub(crate) const fn read_symlink_policy(
        &self,
        options: &LocalReadOptions,
        outer_override: Option<LocalSymlinkPolicy>,
    ) -> LocalSymlinkPolicy {
        match outer_override {
            Some(policy) => policy,
            None => match options.symlink_policy() {
                Some(policy) => policy,
                None => self.symlink_policy,
            },
        }
    }

    /// Opens an already-bound operand without querying the process PWD again.
    ///
    /// Returns an opened regular-file reader or a structured opening error
    /// carrying the same namespace path and PWD snapshot as the caller.
    ///
    /// # Parameters
    ///
    /// - `resolved`: Namespace coordinate and authority-relative operand.
    /// - `options`: Reader policy for the opened file.
    /// - `operation`: Operation recorded in any structured error.
    /// - `current_directory`: PWD snapshot associated with the original
    ///   resolution. The supplied current-directory snapshot is reused for
    ///   errors, avoiding a second process PWD lookup for relative Host
    ///   operations.
    ///
    /// # Returns
    ///
    /// An owned reader for the resolved regular file.
    ///
    /// # Errors
    ///
    /// Returns policy, type, resolution, or native opening errors.
    pub(super) fn open_resolved_reader(
        &self,
        resolved: &LocalNamespacePath,
        options: &LocalReadOptions,
        operation: LocalFileOperation,
        current_directory: Option<&Path>,
    ) -> LocalResult<LocalFileReader> {
        let symlink_policy = self.read_symlink_policy(options, None);
        self.open_resolved_reader_with_policy(resolved, options, operation, current_directory, symlink_policy)
    }

    /// Opens a resolved operand after applying the selected symlink policy.
    ///
    /// # Parameters
    ///
    /// - `resolved`: Namespace coordinate and authority-relative operand.
    /// - `options`: Reader policy for the opened file.
    /// - `operation`: Operation recorded in any structured error.
    /// - `current_directory`: PWD snapshot associated with the original
    ///   resolution.
    /// - `symlink_policy`: Policy governing the final symlink.
    ///
    /// # Returns
    ///
    /// An owned reader for the resolved regular file.
    ///
    /// # Errors
    ///
    /// Returns policy, type, resolution, or native opening errors.
    #[inline]
    pub(super) fn open_resolved_reader_with_policy(
        &self,
        resolved: &LocalNamespacePath,
        options: &LocalReadOptions,
        operation: LocalFileOperation,
        current_directory: Option<&Path>,
        symlink_policy: crate::LocalSymlinkPolicy,
    ) -> LocalResult<LocalFileReader> {
        self.open_resolved_reader_with_policy_and_contract(
            resolved,
            options,
            operation,
            current_directory,
            symlink_policy,
            None,
            None,
        )
    }

    /// Opens a resolved operand with optional resolution and binding
    /// constraints.
    ///
    /// # Parameters
    ///
    /// - `resolved`: Namespace coordinate and authority-relative operand.
    /// - `options`: Reader policy for the opened file.
    /// - `operation`: Operation recorded in any structured error.
    /// - `current_directory`: PWD snapshot associated with the original
    ///   resolution.
    /// - `symlink_policy`: Policy governing the final symlink.
    /// - `resolution`: Optional authority resolution contract.
    /// - `deadline`: Optional path-binding deadline.
    ///
    /// # Returns
    ///
    /// An owned reader for the resolved regular file.
    ///
    /// # Errors
    ///
    /// Returns policy, path-type, resolution, or native opening errors with
    /// the supplied operation and path context.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn open_resolved_reader_with_policy_and_contract(
        &self,
        resolved: &LocalNamespacePath,
        options: &LocalReadOptions,
        operation: LocalFileOperation,
        current_directory: Option<&Path>,
        symlink_policy: crate::LocalSymlinkPolicy,
        resolution: Option<crate::resolution::ResolutionContract>,
        deadline: Option<std::time::Instant>,
    ) -> LocalResult<LocalFileReader> {
        validate_scope_symlink_policy(
            self.scope(),
            symlink_policy,
            operation,
            Some(resolved.namespace_absolute()),
        )?;
        reject_directory_qualified_file(resolved, operation, current_directory)?;
        match &self.core.namespace {
            LocalNamespace::Host => {
                HostLocalFileSystem::open_reader_with_policy(resolved.authority_relative(), options, symlink_policy)
            }
            LocalNamespace::Rooted(rooted) => match resolution {
                Some(contract) => rooted.open_reader_with_contract_and_deadline(
                    resolved.authority_relative(),
                    options,
                    symlink_policy,
                    contract,
                    deadline,
                ),
                None => rooted.open_reader(resolved.authority_relative(), options, symlink_policy),
            },
        }
        .map_err(|error| operation_error(error, operation, resolved.namespace_absolute(), None, current_directory))
    }
}

/// Detects a trailing separator or final `.` or `..` component that requires a
/// directory.
///
/// # Parameters
///
/// - `path`: Operand to inspect without normalizing it.
///
/// # Returns
///
/// Whether the operand names a directory-qualified path.
#[must_use]
pub(super) fn has_directory_qualification(path: &Path) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::ffi::OsStrExt;
        let bytes = path.as_os_str().as_bytes();
        if bytes.is_empty() || bytes.last() == Some(&b'/') {
            return true;
        }
        let component = bytes.rsplit(|byte| *byte == b'/').next().unwrap_or_default();
        component == b"." || component == b".."
    }
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;
        let units = path.as_os_str().encode_wide().collect::<Vec<_>>();
        let is_separator = |unit: &u16| *unit == b'/' as u16 || *unit == b'\\' as u16;
        if units.is_empty() || units.last().is_some_and(is_separator) {
            return true;
        }
        let component = units.rsplit(is_separator).next().unwrap_or_default();
        component == [b'.' as u16] || component == [b'.' as u16, b'.' as u16]
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = path;
        false
    }
}
