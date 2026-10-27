// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

// Host temp operations.

use std::error::Error;
use std::fmt;
use std::fs::File;
use std::ffi::OsStr;
use std::path::PathBuf;
use std::time::Instant;

use super::HostLocalFileSystem;
use super::LocalCopyOptions;
use super::LocalFileError;
use super::LocalFileErrorKind;
use super::LocalFileOperation;
use super::LocalMetadataPreservePolicy;
use super::LocalResult;
use super::LocalSymlinkPolicy;
use super::LocalTempDirectory;
use super::LocalTempDirectoryOptions;
use super::LocalTempFile;
use super::LocalTempFileOptions;
use crate::policy::LocalPrivacyRequirement;
use super::LocalWriteMode;
use super::LocalWriteOptions;
use super::Path;
use super::fs;
use super::io;
use super::resolve_host_path;
use super::validate_temp_affixes;

impl HostLocalFileSystem {
    /// Creates a Host cleanup-owned temporary file.
    ///
    /// The selected parent is bound before entry creation, and affixes are
    /// validated before any temporary entry is left behind.
    /// # Parameters
    ///
    /// - `options`: Parent directory, filename affixes, and collision limit.
    /// - `symlink_policy`: Policy for the temporary resource parent.
    ///
    /// # Returns
    ///
    /// An open temporary file that removes its path unless kept or persisted.
    ///
    /// # Errors
    ///
    /// Returns `LocalFileError` when the parent cannot be bound or created,
    /// affixes are invalid, or a unique file cannot be created.
    pub fn create_temp_file_with_policy(
        options: &LocalTempFileOptions,
        symlink_policy: LocalSymlinkPolicy,
    ) -> LocalResult<LocalTempFile> {
        let parent = options.parent().map_or_else(std::env::temp_dir, Path::to_path_buf);
        let parent = resolve_host_path(&parent, symlink_policy, true)?;
        if options.creates_parent() {
            create_host_temp_parent(&parent, LocalFileOperation::CreateTempFile)?;
        }
        validate_host_temp_parent(&parent, LocalFileOperation::CreateTempFile)?;
        validate_temp_affixes(options.prefix(), options.suffix()).map_err(|error| {
            LocalFileError::from_io(LocalFileOperation::CreateTempFile, Some(parent.clone()), None, error)
                .with_kind(LocalFileErrorKind::InvalidOptions)
        })?;
        if options.privacy() == LocalPrivacyRequirement::Required {
            return create_private_host_temp_file(&parent, options, symlink_policy);
        }
        let sandbox =
            crate::local::create_temp_dir_in_dir_with_affixes(&parent, Some("sandbox-"), None, options.max_attempts())
                .map_err(|error| {
                    let invalid_options = error.kind() == io::ErrorKind::InvalidInput;
                    let error =
                        LocalFileError::from_io(LocalFileOperation::CreateTempFile, Some(parent.clone()), None, error);
                    if invalid_options {
                        error.with_kind(LocalFileErrorKind::InvalidOptions)
                    } else {
                        error
                    }
                })?;
        let created =
            crate::local::create_temp_file_in_dir(&sandbox, options.prefix(), options.suffix(), options.max_attempts());
        let result = created.and_then(|(path, file)| LocalTempFile::host(path, sandbox.clone(), file, symlink_policy));
        let result = result.map_err(|error| cleanup_failed_temp_sandbox(&sandbox, error));
        result.map_err(|error| {
            let invalid_options = error.kind() == io::ErrorKind::InvalidInput;
            let error = LocalFileError::from_io(LocalFileOperation::CreateTempFile, Some(parent), None, error);
            if invalid_options {
                error.with_kind(LocalFileErrorKind::InvalidOptions)
            } else {
                error
            }
        })
    }

    /// Creates a Host cleanup-owned temporary directory.
    ///
    /// # Parameters
    ///
    /// - `options`: Parent directory, directory-name affixes, and collision
    ///   limit.
    /// - `symlink_policy`: Policy for the temporary resource parent.
    ///
    /// # Returns
    ///
    /// A temporary directory that recursively removes itself unless kept or
    /// persisted.
    ///
    /// # Errors
    ///
    /// Returns `LocalFileError` when the parent cannot be bound or created,
    /// affixes are invalid, or a unique directory cannot be created.
    pub fn create_temp_directory_with_policy(
        options: &LocalTempDirectoryOptions,
        symlink_policy: LocalSymlinkPolicy,
    ) -> LocalResult<LocalTempDirectory> {
        let parent = options.parent().map_or_else(std::env::temp_dir, Path::to_path_buf);
        let parent = resolve_host_path(&parent, symlink_policy, true)?;
        if options.creates_parent() {
            create_host_temp_parent(&parent, LocalFileOperation::CreateTempDirectory)?;
        }
        validate_host_temp_parent(&parent, LocalFileOperation::CreateTempDirectory)?;
        validate_temp_affixes(options.prefix(), options.suffix()).map_err(|error| {
            LocalFileError::from_io(
                LocalFileOperation::CreateTempDirectory,
                Some(parent.clone()),
                None,
                error,
            )
            .with_kind(LocalFileErrorKind::InvalidOptions)
        })?;
        if options.privacy() == LocalPrivacyRequirement::Required {
            return create_private_host_temp_directory(&parent, options, symlink_policy);
        }
        let sandbox =
            crate::local::create_temp_dir_in_dir_with_affixes(&parent, Some("sandbox-"), None, options.max_attempts())
                .map_err(|error| {
                    let invalid_options = error.kind() == io::ErrorKind::InvalidInput;
                    let error = LocalFileError::from_io(
                        LocalFileOperation::CreateTempDirectory,
                        Some(parent.clone()),
                        None,
                        error,
                    );
                    if invalid_options {
                        error.with_kind(LocalFileErrorKind::InvalidOptions)
                    } else {
                        error
                    }
                })?;
        let created = crate::local::create_temp_dir_in_dir_with_affixes(
            &sandbox,
            options.prefix(),
            options.suffix(),
            options.max_attempts(),
        );
        let result = created
            .and_then(|path| LocalTempDirectory::host(path, sandbox.clone(), symlink_policy, options.cleanup_limits()));
        let result = result.map_err(|error| cleanup_failed_temp_sandbox(&sandbox, error));
        result.map_err(|error| {
            let invalid_options = error.kind() == io::ErrorKind::InvalidInput;
            let error = LocalFileError::from_io(LocalFileOperation::CreateTempDirectory, Some(parent), None, error);
            if invalid_options {
                error.with_kind(LocalFileErrorKind::InvalidOptions)
            } else {
                error
            }
        })
    }
}

/// Removes only the sandbox created by this failed temporary-resource build.
fn cleanup_failed_temp_sandbox(sandbox: &Path, primary: io::Error) -> io::Error {
    #[cfg(feature = "test-support")]
    let cleanup = if crate::local::test_support_enabled("temp-identity-file-cleanup") {
        Err(io::Error::other("injected sandbox cleanup failure"))
    } else {
        fs::remove_dir_all(sandbox)
    };
    #[cfg(not(feature = "test-support"))]
    let cleanup = fs::remove_dir_all(sandbox);

    match cleanup {
        Ok(()) => primary,
        Err(cleanup_error) => io::Error::new(
            primary.kind(),
            TempSandboxCleanupError {
                primary,
                sandbox: sandbox.to_path_buf(),
                cleanup: cleanup_error,
            },
        ),
    }
}

fn create_private_host_temp_file(
    parent: &Path,
    options: &LocalTempFileOptions,
    symlink_policy: LocalSymlinkPolicy,
) -> LocalResult<LocalTempFile> {
    #[cfg(not(any(unix, windows)))]
    {
        let _ = (parent, options, symlink_policy);
        return Err(LocalFileError::new(LocalFileErrorKind::RequirementNotMet, LocalFileOperation::CreateTempFile));
    }
    #[cfg(any(unix, windows))]
    {
        if options.max_attempts() == Some(0) {
            return Err(LocalFileError::from_io(
                LocalFileOperation::CreateTempFile,
                Some(parent.to_path_buf()),
                None,
                io::Error::new(io::ErrorKind::InvalidInput, "temporary entry retry count must be positive"),
            ).with_kind(LocalFileErrorKind::InvalidOptions));
        }
        let parent_handle = open_host_directory_handle(parent).map_err(|error| {
            LocalFileError::from_io(LocalFileOperation::CreateTempFile, Some(parent.to_path_buf()), None, error)
        })?;
        let maximum = options.max_attempts().unwrap_or(128);
        for _ in 0..maximum {
            let recovery_parent = reserve_private_parent(&parent_handle).map_err(|error| {
                LocalFileError::from_io(LocalFileOperation::CreateTempFile, Some(parent.to_path_buf()), None, error)
            })?;
            let sandbox_name = crate::local::try_random_file_name("sandbox-", None, None).map_err(|error| {
                LocalFileError::from_io(LocalFileOperation::CreateTempFile, Some(parent.to_path_buf()), None, error)
            })?;
            let sandbox_os = std::ffi::OsStr::new(&sandbox_name);
            let sandbox_handle = match private_create_directory_at(&parent_handle, sandbox_os) {
                Ok(handle) => handle,
                Err(error) if error.retained.is_none() && error.source.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(private_create_error(LocalFileOperation::CreateTempFile, parent.join(&sandbox_name), error, None)),
            };
            let sandbox = parent.join(&sandbox_name);
            let sandbox_guard = match reserve_private_directory_guard(recovery_parent, &sandbox_name, &sandbox_handle) {
                Ok(guard) => guard,
                Err((error, recovery_parent)) => {
                    let owner = native_temp_recovery(sandbox.clone(), recovery_parent, sandbox_name, sandbox_handle, true, None);
                    return Err(LocalFileError::from_io(LocalFileOperation::CreateTempFile, Some(sandbox), None, error).with_temp_recovery(owner));
                }
            };
            let file_name = match crate::local::try_random_file_name("qubit-local-files-", options.prefix(), options.suffix()) {
                Ok(name) => name,
                Err(error) => return Err(sandbox_failure(LocalFileOperation::CreateTempFile, sandbox.clone(), error, Some(sandbox_guard))),
            };
            let file = match private_create_file_at(&sandbox_handle, OsStr::new(&file_name)) {
                Ok(file) => file,
                Err(error) if error.retained.is_none() && error.source.kind() == io::ErrorKind::AlreadyExists => {
                    cleanup_private_sandbox(&sandbox, sandbox_guard)?;
                    continue;
                }
                Err(error) => {
                    return Err(private_create_error(LocalFileOperation::CreateTempFile, sandbox.join(&file_name), error, Some(sandbox_guard)));
                }
            };
            let target = sandbox.join(&file_name);
            let owner_file = match file.try_clone() {
                Ok(file) => file,
                Err(error) => {
                    let recovery = native_temp_recovery(
                        target.clone(),
                        sandbox_handle,
                        file_name.clone(),
                        file,
                        false,
                        Some(sandbox_guard),
                    );
                    return Err(LocalFileError::from_io(
                        LocalFileOperation::CreateTempFile,
                        Some(target),
                        None,
                        error,
                    ).with_temp_recovery(recovery));
                }
            };
            return match LocalTempFile::host(target.clone(), sandbox, owner_file, symlink_policy) {
                Ok(resource) => Ok(resource),
                Err(error) => {
                    Err(LocalFileError::from_io(
                        LocalFileOperation::CreateTempFile,
                        Some(target.clone()),
                        None,
                        error,
                    )
                    .with_temp_recovery(native_temp_recovery(target, sandbox_handle, file_name, file, false, Some(sandbox_guard))))
                }
            };
        }
        Err(LocalFileError::from_io(
            LocalFileOperation::CreateTempFile,
            Some(parent.to_path_buf()),
            None,
            io::Error::new(io::ErrorKind::AlreadyExists, "private temp name attempts exhausted"),
        ))
    }
}

fn create_private_host_temp_directory(
    parent: &Path,
    options: &LocalTempDirectoryOptions,
    symlink_policy: LocalSymlinkPolicy,
) -> LocalResult<LocalTempDirectory> {
    #[cfg(not(any(unix, windows)))]
    {
        let _ = (parent, options, symlink_policy);
        return Err(LocalFileError::new(LocalFileErrorKind::RequirementNotMet, LocalFileOperation::CreateTempDirectory));
    }
    #[cfg(any(unix, windows))]
    {
        if options.max_attempts() == Some(0) {
            return Err(LocalFileError::from_io(
                LocalFileOperation::CreateTempDirectory,
                Some(parent.to_path_buf()),
                None,
                io::Error::new(io::ErrorKind::InvalidInput, "temporary entry retry count must be positive"),
            ).with_kind(LocalFileErrorKind::InvalidOptions));
        }
        let parent_handle = open_host_directory_handle(parent).map_err(|error| {
            LocalFileError::from_io(LocalFileOperation::CreateTempDirectory, Some(parent.to_path_buf()), None, error)
        })?;
        let maximum = options.max_attempts().unwrap_or(128);
        for _ in 0..maximum {
            let recovery_parent = reserve_private_parent(&parent_handle).map_err(|error| {
                LocalFileError::from_io(LocalFileOperation::CreateTempDirectory, Some(parent.to_path_buf()), None, error)
            })?;
            let sandbox_name = crate::local::try_random_file_name("sandbox-", None, None).map_err(|error| {
                LocalFileError::from_io(LocalFileOperation::CreateTempDirectory, Some(parent.to_path_buf()), None, error)
            })?;
            let sandbox = parent.join(&sandbox_name);
            let sandbox_handle = match private_create_directory_at(&parent_handle, OsStr::new(&sandbox_name)) {
                Ok(handle) => handle,
                Err(error) if error.retained.is_none() && error.source.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(error) => return Err(private_create_error(LocalFileOperation::CreateTempDirectory, sandbox, error, None)),
            };
            let sandbox_guard = match reserve_private_directory_guard(recovery_parent, &sandbox_name, &sandbox_handle) {
                Ok(guard) => guard,
                Err((error, recovery_parent)) => {
                    let owner = native_temp_recovery(sandbox.clone(), recovery_parent, sandbox_name, sandbox_handle, true, None);
                    return Err(LocalFileError::from_io(LocalFileOperation::CreateTempDirectory, Some(sandbox), None, error).with_temp_recovery(owner));
                }
            };
            let name = match crate::local::try_random_file_name("qubit-local-files-", options.prefix(), options.suffix()) {
                Ok(name) => name,
                Err(error) => return Err(sandbox_failure(LocalFileOperation::CreateTempDirectory, sandbox.clone(), error, Some(sandbox_guard))),
            };
            #[cfg(feature = "test-support")]
            let target_created = if crate::local::take_test_support("temp-private-directory-target-collision") {
                Err(crate::temp::types::NativePrivateCreateError {
                    source: io::Error::from(io::ErrorKind::AlreadyExists),
                    retained: None,
                })
            } else {
                private_create_directory_at(&sandbox_handle, OsStr::new(&name))
            };
            #[cfg(not(feature = "test-support"))]
            let target_created = private_create_directory_at(&sandbox_handle, OsStr::new(&name));
            match target_created {
                Ok(target_handle) => {
                    let target = sandbox.join(&name);
                    return match LocalTempDirectory::host(target.clone(), sandbox, symlink_policy, options.cleanup_limits()) {
                        Ok(resource) => Ok(resource),
                        Err(error) => {
                            Err(LocalFileError::from_io(
                                LocalFileOperation::CreateTempDirectory,
                                Some(target.clone()),
                                None,
                                error,
                            )
                            .with_temp_recovery(native_temp_recovery(target, sandbox_handle, name, target_handle, true, Some(sandbox_guard))))
                        }
                    };
                }
                Err(error) if error.retained.is_none() && error.source.kind() == io::ErrorKind::AlreadyExists => {
                    cleanup_private_sandbox(&sandbox, sandbox_guard)?;
                }
                Err(error) => {
                    return Err(private_create_error(LocalFileOperation::CreateTempDirectory, sandbox.join(name), error, Some(sandbox_guard)));
                }
            }
        }
        Err(LocalFileError::from_io(
            LocalFileOperation::CreateTempDirectory,
            Some(parent.to_path_buf()),
            None,
            io::Error::new(io::ErrorKind::AlreadyExists, "private temp name attempts exhausted"),
        ))
    }
}

fn open_host_directory_handle(path: &Path) -> io::Result<File> {
    #[cfg(unix)]
    { File::open(path) }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;
        use windows_sys::Win32::Storage::FileSystem::FILE_FLAG_BACKUP_SEMANTICS;
        std::fs::OpenOptions::new().read(true).custom_flags(FILE_FLAG_BACKUP_SEMANTICS).open(path)
    }
    #[cfg(not(any(unix, windows)))]
    { let _ = path; Err(io::Error::new(io::ErrorKind::Unsupported, "native private temp creation is unsupported")) }
}

#[cfg(any(unix, windows))]
fn private_create_file_at(parent: &File, name: &OsStr) -> Result<File, crate::temp::types::NativePrivateCreateError> {
    #[cfg(feature = "test-support")]
    if crate::local::take_test_support("temp-private-file-collision") {
        return Err(crate::temp::types::NativePrivateCreateError {
            source: io::Error::from(io::ErrorKind::AlreadyExists),
            retained: None,
        });
    }
    #[cfg(unix)]
    { crate::temp::internal::privacy_unix::create_private_file_at(parent, name) }
    #[cfg(windows)]
    { crate::temp::internal::privacy_windows::create_private_file_at(parent, name) }
}

#[cfg(any(unix, windows))]
fn private_create_directory_at(parent: &File, name: &OsStr) -> Result<File, crate::temp::types::NativePrivateCreateError> {
    #[cfg(feature = "test-support")]
    if crate::local::take_test_support("temp-private-directory-collision") {
        return Err(crate::temp::types::NativePrivateCreateError {
            source: io::Error::from(io::ErrorKind::AlreadyExists),
            retained: None,
        });
    }
    #[cfg(unix)]
    { crate::temp::internal::privacy_unix::create_private_directory_at(parent, name) }
    #[cfg(windows)]
    { crate::temp::internal::privacy_windows::create_private_directory_at(parent, name) }
}

#[cfg(any(unix, windows))]
fn private_create_error(
    operation: LocalFileOperation,
    path: PathBuf,
    error: crate::temp::types::NativePrivateCreateError,
    extra: Option<crate::temp::types::NativePrivateEntry>,
) -> LocalFileError {
    let mut result = LocalFileError::from_io(operation, Some(path.clone()), None, error.source);
    let mut entries = error.retained.into_iter().collect::<Vec<_>>();
    if let Some(extra) = extra { entries.push(extra); }
    if !entries.is_empty() {
        result = result.with_temp_recovery(crate::LocalTempRecovery::from_native_entries(path, entries));
    }
    result
}

#[cfg(any(unix, windows))]
fn native_temp_recovery(
    path: PathBuf,
    parent: File,
    name: String,
    file: File,
    directory: bool,
    sandbox: Option<crate::temp::types::NativePrivateEntry>,
) -> crate::LocalTempRecovery {
    let identity = crate::observation::query::query_observation(
        &file,
        crate::observation::LocalObservationRequirement::Optional,
    )
    .ok()
    .and_then(|observation| observation.identity().cloned());
    let primary = crate::temp::types::NativePrivateEntry {
            parent,
            name: name.into(),
            file: Some(file),
            identity,
            directory,
        };
    let mut entries = vec![primary];
    if let Some(sandbox) = sandbox { entries.push(sandbox); }
    crate::LocalTempRecovery::from_native_entries(path, entries)
}

#[cfg(any(unix, windows))]
fn reserve_private_parent(parent: &File) -> io::Result<File> {
    #[cfg(feature = "test-support")]
    if crate::local::take_test_support("temp-private-parent-clone") {
        return Err(io::Error::new(io::ErrorKind::ResourceBusy, "injected handle reservation failure"));
    }
    parent.try_clone()
}

#[cfg(any(unix, windows))]
fn reserve_private_directory_guard(
    parent: File,
    name: &str,
    directory: &File,
) -> Result<crate::temp::types::NativePrivateEntry, (io::Error, File)> {
    #[cfg(feature = "test-support")]
    if crate::local::take_test_support("temp-private-sandbox-clone") {
        return Err((io::Error::new(io::ErrorKind::ResourceBusy, "injected handle reservation failure"), parent));
    }
    let directory_handle = match directory.try_clone() {
        Ok(handle) => handle,
        Err(error) => return Err((error, parent)),
    };
    let identity = crate::observation::query::query_observation(
        directory,
        crate::observation::LocalObservationRequirement::Optional,
    ).ok().and_then(|observation| observation.identity().cloned());
    Ok(crate::temp::types::NativePrivateEntry {
        parent,
        name: name.into(),
        file: Some(directory_handle),
        identity,
        directory: true,
    })
}

#[cfg(any(unix, windows))]
fn sandbox_failure(
    operation: LocalFileOperation,
    path: PathBuf,
    source: io::Error,
    entry: Option<crate::temp::types::NativePrivateEntry>,
) -> LocalFileError {
    let error = LocalFileError::from_io(operation, Some(path.clone()), None, source);
    match entry {
        Some(entry) => error.with_temp_recovery(crate::LocalTempRecovery::from_native(path, entry)),
        None => error,
    }
}

#[cfg(any(unix, windows))]
fn cleanup_private_sandbox(
    path: &Path,
    entry: crate::temp::types::NativePrivateEntry,
) -> LocalResult<()> {
    let mut owner = crate::LocalTempRecovery::from_native(path.to_path_buf(), entry);
    match owner.cleanup() {
        Ok(()) => Ok(()),
        Err(error) => Err(error.with_temp_recovery(owner)),
    }
}

/// Retains the owner-construction error while describing failed sandbox
/// cleanup.
#[derive(Debug)]
struct TempSandboxCleanupError {
    primary: io::Error,
    sandbox: std::path::PathBuf,
    cleanup: io::Error,
}

impl fmt::Display for TempSandboxCleanupError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{}; failed to remove temporary sandbox {}: {}",
            self.primary,
            self.sandbox.display(),
            self.cleanup
        )
    }
}

impl Error for TempSandboxCleanupError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        Some(&self.primary)
    }
}

/// Opens the existing robust same-directory staged writer implementation.
///
/// # Parameters
///
/// - `path`: Bound destination path.
/// - `options`: Unified writer options.
///
/// # Returns
///
/// Open staged writer.
///
/// # Errors
///
/// Returns `LocalFileError` when staging cannot be prepared.
pub(crate) fn open_staged_writer(
    path: &Path,
    options: &LocalWriteOptions,
) -> LocalResult<crate::local::LocalAtomicWriter> {
    let mut native_options = crate::local::LocalAtomicWriteOptions::new()
        .with_durability(options.durability())
        .with_file_sync(options.file_sync())
        .with_metadata_policy(options.metadata_policy());
    if options.mode() == LocalWriteMode::CreateNew {
        native_options = native_options.with_create_new();
    }
    if options.creates_parent() {
        native_options = native_options.with_create_parent();
    }
    if let Some(timeout) = options.open_retry_timeout() {
        native_options = native_options.with_open_retry_timeout(timeout);
    }
    crate::local::LocalAtomicWriter::new(path, native_options).map_err(|error| {
        let kind = error.kind();
        LocalFileError::from_io(
            LocalFileOperation::OpenWriter,
            Some(path.to_path_buf()),
            None,
            io::Error::new(kind, error),
        )
    })
}

/// Converts unified copy policy to the existing shared native implementation.
///
/// # Parameters
///
/// - `options`: Unified public copy options.
/// - `symlink_policy`: Owning filesystem policy used without an override.
/// - `started_at`: Original operation start, including preflight time, used for
///   the tree-copy deadline.
///
/// # Returns
///
/// Tree traversal, conflict, metadata, and durability options. The facade
/// handles source mode, atomicity, and top-level parent creation separately.
pub(crate) fn internal_copy_options(
    options: &LocalCopyOptions,
    symlink_policy: LocalSymlinkPolicy,
    started_at: Instant,
) -> crate::local::LocalCopyDirOptions {
    let symlink_policy = options.symlink_policy_override().unwrap_or(symlink_policy);
    let mut result = crate::local::LocalCopyDirOptions::new()
        .with_conflict(options.conflict())
        .with_type_conflict(options.type_conflict())
        .with_symlink_policy(symlink_policy)
        .with_durability(options.durability())
        .with_started_at(started_at);
    if let Some(value) = options.max_depth() {
        result = result.with_max_depth(value);
    }
    if let Some(value) = options.max_entries() {
        result = result.with_max_entries(value);
    }
    if let Some(value) = options.max_bytes() {
        result = result.with_max_bytes(value);
    }
    if let Some(value) = options.max_open_directories() {
        result = result.with_max_open_directories(value);
    }
    if let Some(value) = options.deadline() {
        result = result.with_deadline(value);
    }
    if options.preserve_metadata() == LocalMetadataPreservePolicy::Permissions {
        result = result.preserve_permissions();
    }
    result
}

/// Confirms that a host temporary-resource parent is an existing directory.
///
/// Returns `NotDirectory` for another entry kind and propagates metadata
/// failures with the supplied operation and parent path.
#[inline]
fn validate_host_temp_parent(parent: &Path, operation: LocalFileOperation) -> LocalResult<()> {
    let metadata = match fs::metadata(parent) {
        Ok(metadata) => metadata,
        Err(error) => {
            return Err(LocalFileError::from_io(
                operation,
                Some(parent.to_path_buf()),
                None,
                error,
            ));
        }
    };
    if !metadata.is_dir() {
        return Err(LocalFileError::new(LocalFileErrorKind::NotDirectory, operation).with_path(parent.to_path_buf()));
    }
    Ok(())
}

/// Creates a missing Host temporary parent while deferring an existing-file
/// collision to the common type validator.
fn create_host_temp_parent(parent: &Path, operation: LocalFileOperation) -> LocalResult<()> {
    match fs::create_dir_all(parent) {
        Ok(()) => Ok(()),
        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => Ok(()),
        Err(error) => Err(LocalFileError::from_io(
            operation,
            Some(parent.to_path_buf()),
            None,
            error,
        )),
    }
}
