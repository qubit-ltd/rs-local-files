// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

// Host temp operations.

use std::ffi::OsStr;
use std::fs::File;
use std::path::PathBuf;
use std::sync::Arc;

use super::HostLocalFileSystem;
use super::LocalFileError;
use super::LocalFileErrorKind;
use super::LocalFileOperation;
use super::LocalResult;
use super::LocalSymlinkPolicy;
use super::LocalTempDirectory;
use super::LocalTempDirectoryOptions;
use super::LocalTempFile;
use super::LocalTempFileOptions;
use super::Path;
use super::fs;
use super::io;
use super::resolve_host_path;
use super::validate_temp_affixes;
use crate::policy::LocalPrivacyRequirement;

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
        let parent = prepare_host_temp_parent(
            options.parent().map_or_else(std::env::temp_dir, Path::to_path_buf),
            options.creates_parent(),
            options.prefix(),
            options.suffix(),
            LocalFileOperation::CreateTempFile,
            symlink_policy,
        )?;
        if options.privacy() == LocalPrivacyRequirement::Required {
            return create_private_host_temp_file(&parent, options, symlink_policy);
        }
        let (sandbox, sandbox_entry) =
            create_host_temp_sandbox(&parent, LocalFileOperation::CreateTempFile, options.max_attempts())?;
        let created = crate::local::internal::create_temp_file_in_dir_with_delete_access(
            &sandbox,
            options.prefix(),
            options.suffix(),
            options.max_attempts(),
        );
        match created {
            Ok((path, file)) => {
                let recovery_child = match clone_host_temp_file(&file) {
                    Ok(file) => file,
                    Err(error) => {
                        let primary = LocalFileError::from_io(
                            LocalFileOperation::CreateTempFile,
                            Some(path.clone()),
                            None,
                            error,
                        );
                        let (recovery_path, recovery_entries) =
                            host_temp_recovery_entries(&sandbox, sandbox_entry, Some((&path, file)), false)?;
                        let recovery =
                            crate::LocalTempRecovery::from_native_entries_with_paths(recovery_path, recovery_entries);
                        return Err(cleanup_failed_temp_owner(primary, recovery, &sandbox));
                    }
                };
                let (recovery_path, recovery_entries) =
                    host_temp_recovery_entries(&sandbox, sandbox_entry, Some((&path, recovery_child)), false)?;
                match LocalTempFile::host(path.clone(), sandbox.clone(), file, symlink_policy) {
                    Ok(resource) => Ok(resource),
                    Err(error) => Err(cleanup_failed_temp_owner(
                        LocalFileError::from_io(LocalFileOperation::CreateTempFile, Some(path), None, error),
                        crate::LocalTempRecovery::from_native_entries_with_paths(recovery_path, recovery_entries),
                        &sandbox,
                    )),
                }
            }
            Err(error) => {
                let primary =
                    LocalFileError::from_io(LocalFileOperation::CreateTempFile, Some(sandbox.clone()), None, error);
                let recovery_path = sandbox.clone();
                let recovery_entries = vec![sandbox_entry];
                let recovery = crate::LocalTempRecovery::from_native_entries(recovery_path, recovery_entries);
                Err(cleanup_failed_temp_owner(primary, recovery, &sandbox))
            }
        }
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
        let parent = prepare_host_temp_parent(
            options.parent().map_or_else(std::env::temp_dir, Path::to_path_buf),
            options.creates_parent(),
            options.prefix(),
            options.suffix(),
            LocalFileOperation::CreateTempDirectory,
            symlink_policy,
        )?;
        if options.privacy() == LocalPrivacyRequirement::Required {
            return create_private_host_temp_directory(&parent, options, symlink_policy);
        }
        let (sandbox, sandbox_entry) =
            create_host_temp_sandbox(&parent, LocalFileOperation::CreateTempDirectory, options.max_attempts())?;
        let sandbox_handle = Arc::clone(
            sandbox_entry
                .file
                .as_ref()
                .expect("created sandbox retains its open directory handle"),
        );
        let created = create_host_temp_directory_entry(
            &sandbox,
            sandbox_handle,
            options.prefix(),
            options.suffix(),
            options.max_attempts(),
            LocalFileOperation::CreateTempDirectory,
        );
        match created {
            Ok((path, child)) => {
                let (recovery_path, recovery_entries) =
                    host_temp_recovery_entries(&sandbox, sandbox_entry, Some((&path, child)), true)?;
                match LocalTempDirectory::host(path.clone(), sandbox.clone(), symlink_policy, options.cleanup_limits())
                {
                    Ok(resource) => Ok(resource),
                    Err(error) => Err(cleanup_failed_temp_owner(
                        LocalFileError::from_io(LocalFileOperation::CreateTempDirectory, Some(path), None, error),
                        crate::LocalTempRecovery::from_native_entries_with_paths(recovery_path, recovery_entries),
                        &sandbox,
                    )),
                }
            }
            Err(primary) => {
                let recovery_path = sandbox.clone();
                let recovery_entries = vec![sandbox_entry];
                let recovery = crate::LocalTempRecovery::from_native_entries(recovery_path, recovery_entries);
                Err(cleanup_failed_temp_owner(primary, recovery, &sandbox))
            }
        }
    }
}

/// Creates a sandbox and returns its retained cleanup entry.
///
/// The returned entry owns the parent authority, opened sandbox handle, and
/// best-effort identity snapshot. Its joined path is diagnostic; recovery
/// deletes only while the retained name still identifies that object.
///
/// # Parameters
/// - `parent`: Validated directory in which to create the sandbox.
/// - `operation`: Operation to attach to creation and cleanup errors.
/// - `max_attempts`: Optional collision retry limit.
///
/// # Returns
/// The sandbox path and retained native cleanup entry.
///
/// # Errors
/// Returns an operation error when the parent cannot be opened or the sandbox
/// cannot be created.
fn create_host_temp_sandbox(
    parent: &Path,
    operation: LocalFileOperation,
    max_attempts: Option<usize>,
) -> LocalResult<(PathBuf, crate::temp::internal::NativePrivateEntry)> {
    let parent_handle = Arc::new(
        open_host_temp_parent_directory(parent)
            .map_err(|error| LocalFileError::from_io(operation, Some(parent.to_path_buf()), None, error))?,
    );
    let (sandbox, sandbox_handle) = match create_host_temp_directory_entry(
        parent,
        Arc::clone(&parent_handle),
        Some("sandbox-"),
        None,
        max_attempts,
        operation,
    ) {
        Ok(created) => created,
        Err(error) => return Err(cleanup_failed_temp_creation_owner(error)),
    };
    let identity = crate::observation::query::query_observation(
        &sandbox_handle,
        crate::observation::LocalObservationRequirement::Optional,
    )
    .ok()
    .and_then(|observation| observation.identity().cloned());
    let name = sandbox
        .file_name()
        .ok_or_else(|| LocalFileError::new(LocalFileErrorKind::InvalidPath, operation).with_path(sandbox.clone()))?;
    let entry = crate::temp::internal::NativePrivateEntry {
        parent: parent_handle,
        name: name.to_os_string(),
        file: Some(Arc::new(sandbox_handle)),
        identity,
        directory: true,
    };
    Ok((sandbox, entry))
}

/// Resolves and validates the parent and name affixes before temporary work.
///
/// Parent creation precedes directory validation, and affix validation follows
/// both so the observable error ordering matches the public creation methods.
///
/// # Parameters
/// - `parent`: Requested parent path, or the process temporary directory.
/// - `creates_parent`: Whether missing parent directories should be created.
/// - `prefix`, `suffix`: Optional entry-name affixes.
/// - `operation`: Operation attached to validation and creation errors.
/// - `symlink_policy`: Policy used to bind the parent path.
///
/// # Returns
/// The resolved and validated parent directory.
///
/// # Errors
/// Returns an operation error when resolution, creation, metadata validation,
/// or affix validation fails.
fn prepare_host_temp_parent(
    parent: PathBuf,
    creates_parent: bool,
    prefix: Option<&str>,
    suffix: Option<&str>,
    operation: LocalFileOperation,
    symlink_policy: LocalSymlinkPolicy,
) -> LocalResult<PathBuf> {
    let parent = resolve_host_path(&parent, symlink_policy, true)?;
    if creates_parent {
        create_host_temp_parent(&parent, operation)?;
    }
    validate_host_temp_parent(&parent, operation)?;
    validate_temp_affixes(prefix, suffix).map_err(|error| {
        LocalFileError::from_io(operation, Some(parent.clone()), None, error)
            .with_kind(LocalFileErrorKind::InvalidOptions)
    })?;
    Ok(parent)
}

/// Opens the temporary parent with Windows directory and attribute rights.
///
/// # Parameters
/// - `path`: Existing parent directory.
///
/// # Returns
/// A handle retaining access to the parent directory.
///
/// # Errors
/// Returns the operating-system error if the directory cannot be opened.
#[cfg(windows)]
fn open_host_temp_parent_directory(path: &Path) -> io::Result<File> {
    use std::os::windows::fs::OpenOptionsExt;

    use windows_sys::Win32::Storage::FileSystem::FILE_FLAG_BACKUP_SEMANTICS;
    use windows_sys::Win32::Storage::FileSystem::FILE_READ_ATTRIBUTES;

    fs::OpenOptions::new()
        .access_mode(FILE_READ_ATTRIBUTES)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS)
        .open(path)
}

/// Opens the temporary parent using the platform's standard file API.
///
/// # Parameters
/// - `path`: Existing parent directory.
///
/// # Returns
/// A handle retaining access to the parent directory.
///
/// # Errors
/// Returns the operating-system error if the directory cannot be opened.
#[cfg(not(windows))]
fn open_host_temp_parent_directory(path: &Path) -> io::Result<File> {
    File::open(path)
}

/// Creates a temporary directory entry and opens it for retained operations.
///
/// If opening fails after creation, the returned error carries recovery
/// ownership with the parent handle and name, but without claiming an object
/// identity that could not be observed.
///
/// # Parameters
/// - `parent`: Directory in which to create the entry.
/// - `parent_handle`: Retained authority for the parent directory.
/// - `prefix`, `suffix`: Optional entry-name affixes.
/// - `max_attempts`: Optional limit on collision retries.
/// - `operation`: Operation associated with returned errors.
///
/// # Returns
/// The created path and open directory handle, or an error retaining cleanup
/// ownership.
fn create_host_temp_directory_entry(
    parent: &Path,
    parent_handle: Arc<File>,
    prefix: Option<&str>,
    suffix: Option<&str>,
    max_attempts: Option<usize>,
    operation: LocalFileOperation,
) -> LocalResult<(PathBuf, File)> {
    create_host_temp_directory_entry_with_opener(
        parent,
        parent_handle,
        prefix,
        suffix,
        max_attempts,
        operation,
        open_temp_directory,
    )
}

/// Creates an entry and binds its opened directory handle.
///
/// The opener runs after creation. If it fails, ownership is transferred into
/// the error for a later cleanup attempt; the supplied path is retained only
/// as diagnostic context. `open_directory` is an internal seam for exercising
/// this failure boundary.
///
/// # Parameters
/// - `parent`: Directory in which to create the entry.
/// - `parent_handle`: Retained parent authority.
/// - `prefix`, `suffix`: Optional entry-name affixes.
/// - `max_attempts`: Optional collision retry limit.
/// - `operation`: Operation recorded in returned errors.
/// - `open_directory`: Opens the newly created entry.
///
/// # Returns
/// The created path and opened directory handle.
///
/// # Errors
/// Returns a structured operation error; a post-creation open failure carries
/// recovery ownership.
fn create_host_temp_directory_entry_with_opener(
    parent: &Path,
    parent_handle: Arc<File>,
    prefix: Option<&str>,
    suffix: Option<&str>,
    max_attempts: Option<usize>,
    operation: LocalFileOperation,
    open_directory: impl FnOnce(&Path) -> io::Result<File>,
) -> LocalResult<(PathBuf, File)> {
    let path =
        crate::local::create_temp_dir_in_dir_with_affixes(parent, prefix, suffix, max_attempts).map_err(|error| {
            let invalid_options = error.kind() == io::ErrorKind::InvalidInput;
            let error = LocalFileError::from_io(operation, Some(parent.to_path_buf()), None, error);
            if invalid_options {
                error.with_kind(LocalFileErrorKind::InvalidOptions)
            } else {
                error
            }
        })?;
    let handle = match open_directory(&path) {
        Ok(handle) => handle,
        Err(error) => {
            let name = path.file_name().ok_or_else(|| {
                LocalFileError::new(LocalFileErrorKind::InvalidPath, operation).with_path(path.clone())
            })?;
            let owner = crate::LocalTempRecovery::from_native(
                path.clone(),
                crate::temp::internal::NativePrivateEntry {
                    parent: Arc::clone(&parent_handle),
                    name: name.to_os_string(),
                    file: None,
                    identity: None,
                    directory: true,
                },
            );
            return Err(LocalFileError::from_io(operation, Some(path), None, error).with_temp_recovery(owner));
        }
    };
    Ok((path, handle))
}

/// Builds ordered recovery entries for an optional child and its sandbox.
///
/// The child is cleaned before the sandbox so the sandbox remains available
/// as its parent. Identity observation is best-effort here; cleanup refuses
/// to delete any entry whose identity cannot later be verified. Each path is
/// diagnostic and access remains anchored by the retained parent handle.
///
/// # Parameters
/// - `sandbox_path`: Diagnostic path for the sandbox.
/// - `sandbox`: Retained sandbox cleanup entry.
/// - `child`: Optional child path and open handle.
/// - `child_is_directory`: Whether the child is a directory.
///
/// # Returns
/// The primary diagnostic path and ordered cleanup entries.
///
/// # Errors
/// Returns an error if required parent authority or entry names are
/// unavailable.
fn host_temp_recovery_entries(
    sandbox_path: &Path,
    sandbox: crate::temp::internal::NativePrivateEntry,
    child: Option<(&Path, File)>,
    child_is_directory: bool,
) -> LocalResult<(PathBuf, Vec<(PathBuf, crate::temp::internal::NativePrivateEntry)>)> {
    let mut entries = Vec::with_capacity(2);
    let recovery_path = if let Some((child_path, child_handle)) = child {
        let child_parent = sandbox.file.as_ref().ok_or_else(|| {
            LocalFileError::new(LocalFileErrorKind::RequirementNotMet, LocalFileOperation::Cleanup)
                .with_path(child_path.to_path_buf())
        })?;
        let child_parent = Arc::clone(child_parent);
        let identity = crate::observation::query::query_observation(
            &child_handle,
            crate::observation::LocalObservationRequirement::Optional,
        )
        .ok()
        .and_then(|observation| observation.identity().cloned());
        let name = child_path.file_name().ok_or_else(|| {
            LocalFileError::new(LocalFileErrorKind::InvalidPath, LocalFileOperation::Cleanup)
                .with_path(child_path.to_path_buf())
        })?;
        entries.push((
            child_path.to_path_buf(),
            crate::temp::internal::NativePrivateEntry {
                parent: child_parent,
                name: name.to_os_string(),
                file: Some(Arc::new(child_handle)),
                identity,
                directory: child_is_directory,
            },
        ));
        child_path.to_path_buf()
    } else {
        sandbox_path.to_path_buf()
    };
    entries.push((sandbox_path.to_path_buf(), sandbox));
    Ok((recovery_path, entries))
}

/// Opens a newly created temporary directory for retained operations.
///
/// # Parameters
/// - `path`: Newly created directory.
///
/// # Returns
/// An open directory handle.
///
/// # Errors
/// Returns the native error if opening fails.
#[cfg(unix)]
fn open_temp_directory(path: &Path) -> io::Result<File> {
    #[cfg(feature = "test-support")]
    if crate::local::test_support_enabled("host-temp-sandbox-open") {
        return Err(io::Error::other("injected sandbox directory open failure"));
    }
    File::open(path)
}

/// Opens a newly created temporary directory with native cleanup rights.
///
/// # Parameters
/// - `path`: Newly created directory.
///
/// # Returns
/// An open handle with delete and attribute access.
///
/// # Errors
/// Returns the native error if opening fails.
#[cfg(windows)]
fn open_temp_directory(path: &Path) -> io::Result<File> {
    use std::os::windows::fs::OpenOptionsExt;

    use windows_sys::Win32::Storage::FileSystem::DELETE;
    use windows_sys::Win32::Storage::FileSystem::FILE_FLAG_BACKUP_SEMANTICS;
    use windows_sys::Win32::Storage::FileSystem::FILE_FLAG_OPEN_REPARSE_POINT;
    use windows_sys::Win32::Storage::FileSystem::FILE_READ_ATTRIBUTES;
    fs::OpenOptions::new()
        .access_mode(DELETE | FILE_READ_ATTRIBUTES)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)
}

/// Opens a newly created temporary directory using the standard file API.
///
/// # Parameters
/// - `path`: Newly created directory.
///
/// # Returns
/// An open directory handle.
///
/// # Errors
/// Returns the native error if opening fails.
#[cfg(not(any(unix, windows)))]
fn open_temp_directory(path: &Path) -> io::Result<File> {
    File::open(path)
}

/// Attempts cleanup after a temporary creation failure while preserving owners.
///
/// Existing recovery ownership from `primary` is combined with `owner` when
/// both are native-entry owners. Cleanup failure is attached to the primary
/// error and the remaining owner is returned so callers can retry; partial
/// cleanup may already have released earlier entries.
///
/// # Parameters
/// - `primary`: Creation error to preserve.
/// - `owner`: Cleanup ownership for the failed resource.
/// - `sandbox`: Diagnostic sandbox path.
///
/// # Returns
/// The primary error, retaining remaining ownership if cleanup fails.
fn cleanup_failed_temp_owner(
    primary: LocalFileError,
    mut owner: crate::LocalTempRecovery,
    sandbox: &Path,
) -> LocalFileError {
    let (primary, prior_owner) = primary.into_temp_recovery_parts();
    if let Some(prior_owner) = prior_owner {
        owner = match prior_owner.append_native_owner(owner) {
            Ok(owner) => owner,
            Err((prior_owner, sandbox_owner)) => {
                let cleanup_error =
                    LocalFileError::new(LocalFileErrorKind::RequirementNotMet, LocalFileOperation::Cleanup)
                        .with_temp_recovery(sandbox_owner);
                return primary
                    .with_cleanup_error(cleanup_error)
                    .with_temp_recovery(prior_owner);
            }
        };
    }
    #[cfg(feature = "test-support")]
    let cleanup = if crate::local::test_support_enabled("temp-identity-file-cleanup")
        || crate::local::test_support_enabled("host-temp-file-recovery-clone-cleanup")
    {
        Err(LocalFileError::from_io(
            LocalFileOperation::Cleanup,
            Some(sandbox.to_path_buf()),
            None,
            io::Error::other("injected sandbox cleanup failure"),
        ))
    } else {
        owner.cleanup()
    };
    #[cfg(not(feature = "test-support"))]
    let _ = sandbox;
    #[cfg(not(feature = "test-support"))]
    let cleanup = owner.cleanup();

    match cleanup {
        Ok(()) => primary,
        Err(cleanup_error) => primary.with_cleanup_error(cleanup_error).with_temp_recovery(owner),
    }
}

/// Attempts cleanup for an owner already attached to a creation error.
///
/// Successful cleanup leaves the original error intact. Failed cleanup is
/// recorded separately and the owner stays attached for a caller-controlled
/// retry rather than being dropped at this boundary.
///
/// # Parameters
/// - `primary`: Creation error that may carry a recovery owner.
///
/// # Returns
/// The original error, retaining recovery ownership if cleanup fails.
fn cleanup_failed_temp_creation_owner(primary: LocalFileError) -> LocalFileError {
    let (primary, recovery) = primary.into_temp_recovery_parts();
    let Some(mut recovery) = recovery else {
        return primary;
    };
    match recovery.cleanup() {
        Ok(()) => primary,
        Err(cleanup_error) => primary.with_cleanup_error(cleanup_error).with_temp_recovery(recovery),
    }
}

/// Clones a file handle so failed resource construction retains independent
/// cleanup ownership.
///
/// # Parameters
/// - `file`: Open handle to the created temporary file.
///
/// # Returns
/// A cloned handle to the same file object.
///
/// # Errors
/// Returns the operating-system error if cloning fails.
fn clone_host_temp_file(file: &File) -> io::Result<File> {
    #[cfg(feature = "test-support")]
    if crate::local::test_support_enabled("host-temp-file-recovery-clone-cleanup") {
        return Err(io::Error::other("injected temp recovery handle clone failure"));
    }
    file.try_clone()
}

/// Creates a required-private temporary file using handle-relative platform
/// APIs.
///
/// # Parameters
/// - `parent`: Validated parent directory.
/// - `options`: File naming and retry options.
/// - `symlink_policy`: Policy used to bind the resulting resource.
///
/// # Returns
/// A private temporary file with cleanup ownership.
///
/// # Errors
/// Returns an operation error when creation or required privacy cannot be
/// provided.
fn create_private_host_temp_file(
    parent: &Path,
    options: &LocalTempFileOptions,
    symlink_policy: LocalSymlinkPolicy,
) -> LocalResult<LocalTempFile> {
    #[cfg(not(any(unix, windows)))]
    {
        let _ = (parent, options, symlink_policy);
        return Err(LocalFileError::new(
            LocalFileErrorKind::RequirementNotMet,
            LocalFileOperation::CreateTempFile,
        ));
    }
    #[cfg(any(unix, windows))]
    {
        if options.max_attempts() == Some(0) {
            return Err(LocalFileError::from_io(
                LocalFileOperation::CreateTempFile,
                Some(parent.to_path_buf()),
                None,
                io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "temporary entry retry count must be positive",
                ),
            )
            .with_kind(LocalFileErrorKind::InvalidOptions));
        }
        let parent_handle = open_host_directory_handle(parent).map_err(|error| {
            LocalFileError::from_io(
                LocalFileOperation::CreateTempFile,
                Some(parent.to_path_buf()),
                None,
                error,
            )
        })?;
        let mut attempts = 0_usize;
        loop {
            if options.max_attempts().is_some_and(|limit| attempts >= limit) {
                break;
            }
            attempts = attempts.saturating_add(1);
            let recovery_parent = reserve_private_parent(&parent_handle).map_err(|error| {
                LocalFileError::from_io(
                    LocalFileOperation::CreateTempFile,
                    Some(parent.to_path_buf()),
                    None,
                    error,
                )
            })?;
            let sandbox_name = crate::local::try_random_file_name("sandbox-", None, None).map_err(|error| {
                LocalFileError::from_io(
                    LocalFileOperation::CreateTempFile,
                    Some(parent.to_path_buf()),
                    None,
                    error,
                )
            })?;
            let sandbox_os = std::ffi::OsStr::new(&sandbox_name);
            #[cfg(feature = "test-support")]
            let sandbox_result = if crate::local::take_repeated_test_support("temp-private-sandbox-collision") {
                Err(crate::temp::types::NativePrivateCreateError {
                    source: io::Error::from(io::ErrorKind::AlreadyExists),
                    retained: None,
                })
            } else {
                private_create_directory_at(&parent_handle, sandbox_os)
            };
            #[cfg(not(feature = "test-support"))]
            let sandbox_result = private_create_directory_at(&parent_handle, sandbox_os);
            let sandbox_handle = match sandbox_result {
                Ok(handle) => handle,
                Err(error) if error.retained.is_none() && error.source.kind() == io::ErrorKind::AlreadyExists => {
                    continue;
                }
                Err(error) => {
                    return Err(private_create_error(
                        LocalFileOperation::CreateTempFile,
                        parent.join(&sandbox_name),
                        error,
                        None,
                    ));
                }
            };
            let sandbox = parent.join(&sandbox_name);
            let sandbox_guard = match reserve_private_directory_guard(recovery_parent, &sandbox_name, &sandbox_handle) {
                Ok(guard) => guard,
                Err((error, recovery_parent)) => {
                    let owner = native_temp_recovery(
                        sandbox.clone(),
                        recovery_parent,
                        sandbox_name,
                        sandbox_handle,
                        true,
                        None,
                    );
                    return Err(LocalFileError::from_io(
                        LocalFileOperation::CreateTempFile,
                        Some(sandbox),
                        None,
                        error,
                    )
                    .with_temp_recovery(owner));
                }
            };
            let file_name =
                match crate::local::try_random_file_name("qubit-local-files-", options.prefix(), options.suffix()) {
                    Ok(name) => name,
                    Err(error) => {
                        return Err(sandbox_failure(
                            LocalFileOperation::CreateTempFile,
                            sandbox.clone(),
                            error,
                            Some(sandbox_guard),
                        ));
                    }
                };
            let file = match private_create_file_at(&sandbox_handle, OsStr::new(&file_name)) {
                Ok(file) => file,
                Err(error) if error.retained.is_none() && error.source.kind() == io::ErrorKind::AlreadyExists => {
                    cleanup_private_sandbox(&sandbox, sandbox_guard)?;
                    continue;
                }
                Err(error) => {
                    return Err(private_create_error(
                        LocalFileOperation::CreateTempFile,
                        sandbox.join(&file_name),
                        error,
                        Some(sandbox_guard),
                    ));
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
                    return Err(
                        LocalFileError::from_io(LocalFileOperation::CreateTempFile, Some(target), None, error)
                            .with_temp_recovery(recovery),
                    );
                }
            };
            return match LocalTempFile::host(target.clone(), sandbox, owner_file, symlink_policy) {
                Ok(resource) => Ok(resource),
                Err(error) => {
                    Err(
                        LocalFileError::from_io(LocalFileOperation::CreateTempFile, Some(target.clone()), None, error)
                            .with_temp_recovery(native_temp_recovery(
                                target,
                                sandbox_handle,
                                file_name,
                                file,
                                false,
                                Some(sandbox_guard),
                            )),
                    )
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

/// Creates a required-private temporary directory using handle-relative
/// platform APIs.
///
/// # Parameters
/// - `parent`: Validated parent directory.
/// - `options`: Directory naming and retry options.
/// - `symlink_policy`: Policy used to bind the resulting resource.
///
/// # Returns
/// A private temporary directory with cleanup ownership.
///
/// # Errors
/// Returns an operation error when creation or required privacy cannot be
/// provided.
fn create_private_host_temp_directory(
    parent: &Path,
    options: &LocalTempDirectoryOptions,
    symlink_policy: LocalSymlinkPolicy,
) -> LocalResult<LocalTempDirectory> {
    #[cfg(not(any(unix, windows)))]
    {
        let _ = (parent, options, symlink_policy);
        return Err(LocalFileError::new(
            LocalFileErrorKind::RequirementNotMet,
            LocalFileOperation::CreateTempDirectory,
        ));
    }
    #[cfg(any(unix, windows))]
    {
        if options.max_attempts() == Some(0) {
            return Err(LocalFileError::from_io(
                LocalFileOperation::CreateTempDirectory,
                Some(parent.to_path_buf()),
                None,
                io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "temporary entry retry count must be positive",
                ),
            )
            .with_kind(LocalFileErrorKind::InvalidOptions));
        }
        let parent_handle = open_host_directory_handle(parent).map_err(|error| {
            LocalFileError::from_io(
                LocalFileOperation::CreateTempDirectory,
                Some(parent.to_path_buf()),
                None,
                error,
            )
        })?;
        let mut attempts = 0_usize;
        loop {
            if options.max_attempts().is_some_and(|limit| attempts >= limit) {
                break;
            }
            attempts = attempts.saturating_add(1);
            let recovery_parent = reserve_private_parent(&parent_handle).map_err(|error| {
                LocalFileError::from_io(
                    LocalFileOperation::CreateTempDirectory,
                    Some(parent.to_path_buf()),
                    None,
                    error,
                )
            })?;
            let sandbox_name = crate::local::try_random_file_name("sandbox-", None, None).map_err(|error| {
                LocalFileError::from_io(
                    LocalFileOperation::CreateTempDirectory,
                    Some(parent.to_path_buf()),
                    None,
                    error,
                )
            })?;
            let sandbox = parent.join(&sandbox_name);
            #[cfg(feature = "test-support")]
            let sandbox_result = if crate::local::take_repeated_test_support("temp-private-sandbox-collision") {
                Err(crate::temp::types::NativePrivateCreateError {
                    source: io::Error::from(io::ErrorKind::AlreadyExists),
                    retained: None,
                })
            } else {
                private_create_directory_at(&parent_handle, OsStr::new(&sandbox_name))
            };
            #[cfg(not(feature = "test-support"))]
            let sandbox_result = private_create_directory_at(&parent_handle, OsStr::new(&sandbox_name));
            let sandbox_handle = match sandbox_result {
                Ok(handle) => handle,
                Err(error) if error.retained.is_none() && error.source.kind() == io::ErrorKind::AlreadyExists => {
                    continue;
                }
                Err(error) => {
                    return Err(private_create_error(
                        LocalFileOperation::CreateTempDirectory,
                        sandbox,
                        error,
                        None,
                    ));
                }
            };
            let sandbox_guard = match reserve_private_directory_guard(recovery_parent, &sandbox_name, &sandbox_handle) {
                Ok(guard) => guard,
                Err((error, recovery_parent)) => {
                    let owner = native_temp_recovery(
                        sandbox.clone(),
                        recovery_parent,
                        sandbox_name,
                        sandbox_handle,
                        true,
                        None,
                    );
                    return Err(LocalFileError::from_io(
                        LocalFileOperation::CreateTempDirectory,
                        Some(sandbox),
                        None,
                        error,
                    )
                    .with_temp_recovery(owner));
                }
            };
            let name =
                match crate::local::try_random_file_name("qubit-local-files-", options.prefix(), options.suffix()) {
                    Ok(name) => name,
                    Err(error) => {
                        return Err(sandbox_failure(
                            LocalFileOperation::CreateTempDirectory,
                            sandbox.clone(),
                            error,
                            Some(sandbox_guard),
                        ));
                    }
                };
            #[cfg(feature = "test-support")]
            let target_created = if crate::local::take_repeated_test_support("temp-private-directory-target-collision")
            {
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
                    return match LocalTempDirectory::host(
                        target.clone(),
                        sandbox,
                        symlink_policy,
                        options.cleanup_limits(),
                    ) {
                        Ok(resource) => Ok(resource),
                        Err(error) => Err(LocalFileError::from_io(
                            LocalFileOperation::CreateTempDirectory,
                            Some(target.clone()),
                            None,
                            error,
                        )
                        .with_temp_recovery(native_temp_recovery(
                            target,
                            sandbox_handle,
                            name,
                            target_handle,
                            true,
                            Some(sandbox_guard),
                        ))),
                    };
                }
                Err(error) if error.retained.is_none() && error.source.kind() == io::ErrorKind::AlreadyExists => {
                    cleanup_private_sandbox(&sandbox, sandbox_guard)?;
                }
                Err(error) => {
                    return Err(private_create_error(
                        LocalFileOperation::CreateTempDirectory,
                        sandbox.join(name),
                        error,
                        Some(sandbox_guard),
                    ));
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

/// Opens a directory handle for native handle-relative temporary operations.
///
/// # Parameters
/// - `path`: Existing directory to open.
///
/// # Returns
/// An open directory handle.
///
/// # Errors
/// Returns the native open error, or `Unsupported` on other platforms.
fn open_host_directory_handle(path: &Path) -> io::Result<File> {
    #[cfg(unix)]
    {
        File::open(path)
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::OpenOptionsExt;

        use windows_sys::Win32::Storage::FileSystem::FILE_FLAG_BACKUP_SEMANTICS;
        std::fs::OpenOptions::new()
            .read(true)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS)
            .open(path)
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = path;
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "native private temp creation is unsupported",
        ))
    }
}

/// Creates a private file relative to an already-open directory.
///
/// # Parameters
/// - `parent`: Open directory used as creation authority.
/// - `name`: Single entry name to create.
///
/// # Returns
/// An open handle to the new file.
///
/// # Errors
/// Returns the native creation error and any retained partial-entry ownership.
#[cfg(any(unix, windows))]
fn private_create_file_at(parent: &File, name: &OsStr) -> Result<File, crate::temp::types::NativePrivateCreateError> {
    #[cfg(feature = "test-support")]
    if crate::local::take_repeated_test_support("temp-private-file-collision") {
        return Err(crate::temp::types::NativePrivateCreateError {
            source: io::Error::from(io::ErrorKind::AlreadyExists),
            retained: None,
        });
    }
    #[cfg(unix)]
    {
        crate::temp::internal::privacy_unix::create_private_file_at(parent, name)
    }
    #[cfg(windows)]
    {
        crate::temp::internal::privacy_windows::create_private_file_at(parent, name)
    }
}

/// Creates a private directory relative to an already-open directory.
///
/// # Parameters
/// - `parent`: Open directory used as creation authority.
/// - `name`: Single entry name to create.
///
/// # Returns
/// An open handle to the new directory.
///
/// # Errors
/// Returns the native creation error and any retained partial-entry ownership.
#[cfg(any(unix, windows))]
fn private_create_directory_at(
    parent: &File,
    name: &OsStr,
) -> Result<File, crate::temp::types::NativePrivateCreateError> {
    #[cfg(feature = "test-support")]
    if crate::local::take_repeated_test_support("temp-private-directory-collision") {
        return Err(crate::temp::types::NativePrivateCreateError {
            source: io::Error::from(io::ErrorKind::AlreadyExists),
            retained: None,
        });
    }
    #[cfg(unix)]
    {
        crate::temp::internal::privacy_unix::create_private_directory_at(parent, name)
    }
    #[cfg(windows)]
    {
        crate::temp::internal::privacy_windows::create_private_directory_at(parent, name)
    }
}

/// Builds an error and transfers any retained native entries to recovery.
///
/// The creation error may own a partially created target; `extra` may own a
/// sandbox needed to reach that target. Both are kept in the returned recovery
/// value so the caller can retry cleanup after this function returns.
///
/// # Parameters
/// - `operation`: Operation associated with the creation error.
/// - `path`: Diagnostic attempted-resource path.
/// - `error`: Native failure and optional partial-entry owner.
/// - `extra`: Optional additional cleanup owner, such as a sandbox.
///
/// # Returns
/// A structured error carrying all available cleanup ownership.
#[cfg(any(unix, windows))]
fn private_create_error(
    operation: LocalFileOperation,
    path: PathBuf,
    error: crate::temp::types::NativePrivateCreateError,
    extra: Option<crate::temp::types::NativePrivateEntry>,
) -> LocalFileError {
    let mut result = LocalFileError::from_io(operation, Some(path.clone()), None, error.source);
    let sandbox_path = path.parent().unwrap_or_else(|| Path::new("")).to_path_buf();
    let mut entries = error
        .retained
        .into_iter()
        .map(|entry| (path.clone(), entry))
        .collect::<Vec<_>>();
    if let Some(extra) = extra {
        entries.push((sandbox_path, extra));
    }
    if !entries.is_empty() {
        result = result.with_temp_recovery(crate::LocalTempRecovery::from_native_entries_with_paths(path, entries));
    }
    result
}

/// Creates recovery ownership for a native temporary entry and optional
/// sandbox.
///
/// Transfers retained parent/name and object handles into recovery ownership.
///
/// The parent handle and name retain namespace access, while the file handle
/// retains the created object. Identity observation is best-effort; if it is
/// unavailable, later cleanup refuses path-only deletion. When supplied,
/// `sandbox` is appended after the primary entry so cleanup removes the child
/// before its containing sandbox.
///
/// # Parameters
/// - `path`: Diagnostic primary-entry path.
/// - `parent`: Open parent directory handle.
/// - `name`: Entry name relative to `parent`.
/// - `file`: Open handle to the created object.
/// - `directory`: Whether the entry is a directory.
/// - `sandbox`: Optional containing sandbox owner.
///
/// # Returns
/// Recovery ownership ordered to clean the primary entry before its sandbox.
#[cfg(any(unix, windows))]
fn native_temp_recovery(
    path: PathBuf,
    parent: File,
    name: String,
    file: File,
    directory: bool,
    sandbox: Option<crate::temp::types::NativePrivateEntry>,
) -> crate::LocalTempRecovery {
    let identity =
        crate::observation::query::query_observation(&file, crate::observation::LocalObservationRequirement::Optional)
            .ok()
            .and_then(|observation| observation.identity().cloned());
    let primary = crate::temp::types::NativePrivateEntry {
        parent: Arc::new(parent),
        name: name.into(),
        file: Some(Arc::new(file)),
        identity,
        directory,
    };
    let mut entries = vec![primary];
    if let Some(sandbox) = sandbox {
        entries.push(sandbox);
    }
    let sandbox_path = path.parent().unwrap_or_else(|| Path::new("")).to_path_buf();
    let mut entries = entries.into_iter();
    let primary = entries.next().expect("native temp recovery always has a primary entry");
    let owner_path = path.clone();
    let mut paths = vec![(path, primary)];
    paths.extend(entries.map(|entry| (sandbox_path.clone(), entry)));
    crate::LocalTempRecovery::from_native_entries_with_paths(owner_path, paths)
}

/// Clones the parent handle for later native cleanup or recovery.
///
/// # Parameters
/// - `parent`: Open parent directory handle to retain.
///
/// # Returns
/// A cloned handle to the same directory.
///
/// # Errors
/// Returns the native error if cloning fails.
#[cfg(any(unix, windows))]
fn reserve_private_parent(parent: &File) -> io::Result<File> {
    #[cfg(feature = "test-support")]
    if crate::local::take_test_support("temp-private-parent-clone") {
        return Err(io::Error::new(
            io::ErrorKind::ResourceBusy,
            "injected handle reservation failure",
        ));
    }
    parent.try_clone()
}

/// Reserves recovery ownership for a created sandbox directory.
///
/// On failure, returns the parent handle to the caller so it can transfer that
/// handle and the still-open directory into a recovery value. Identity capture
/// is best-effort; if it is absent, cleanup refuses deletion by path alone.
///
/// # Parameters
/// - `parent`: Open parent handle returned on reservation failure.
/// - `name`: Sandbox name relative to `parent`.
/// - `directory`: Open handle to the created sandbox.
///
/// # Returns
/// A retained cleanup entry, or the error and parent handle.
#[cfg(any(unix, windows))]
fn reserve_private_directory_guard(
    parent: File,
    name: &str,
    directory: &File,
) -> Result<crate::temp::types::NativePrivateEntry, (io::Error, File)> {
    #[cfg(feature = "test-support")]
    if crate::local::take_test_support("temp-private-sandbox-clone") {
        return Err((
            io::Error::new(io::ErrorKind::ResourceBusy, "injected handle reservation failure"),
            parent,
        ));
    }
    let directory_handle = match directory.try_clone() {
        Ok(handle) => handle,
        Err(error) => return Err((error, parent)),
    };
    let identity = crate::observation::query::query_observation(
        directory,
        crate::observation::LocalObservationRequirement::Optional,
    )
    .ok()
    .and_then(|observation| observation.identity().cloned());
    Ok(crate::temp::types::NativePrivateEntry {
        parent: Arc::new(parent),
        name: name.into(),
        file: Some(Arc::new(directory_handle)),
        identity,
        directory: true,
    })
}

/// Associates an operation failure with recovery ownership of its sandbox.
///
/// # Parameters
/// - `operation`: Operation associated with the failure.
/// - `path`: Diagnostic path.
/// - `source`: Underlying I/O failure.
/// - `entry`: Optional sandbox cleanup owner.
///
/// # Returns
/// A structured error retaining the owner when supplied.
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

/// Attempts sandbox cleanup and returns ownership when cleanup fails.
///
/// # Parameters
/// - `path`: Diagnostic sandbox path.
/// - `entry`: Retained cleanup authority.
///
/// # Returns
/// Success after cleanup, or an error carrying the owner for retry.
#[cfg(any(unix, windows))]
fn cleanup_private_sandbox(path: &Path, entry: crate::temp::types::NativePrivateEntry) -> LocalResult<()> {
    let mut owner = crate::LocalTempRecovery::from_native(path.to_path_buf(), entry);
    match owner.cleanup() {
        Ok(()) => Ok(()),
        Err(error) => Err(error.with_temp_recovery(owner)),
    }
}

/// Confirms that a host temporary-resource parent is an existing directory.
///
/// Returns `NotDirectory` for another entry kind and propagates metadata
/// failures with the supplied operation and parent path.
///
/// # Parameters
/// - `parent`: Path to validate.
/// - `operation`: Operation associated with validation errors.
///
/// # Returns
/// Success when the path names a directory.
///
/// # Errors
/// Returns a metadata error or `NotDirectory` when the path is not a directory.
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
///
/// # Parameters
/// - `parent`: Directory path to create, including missing ancestors.
/// - `operation`: Operation associated with creation errors.
///
/// # Returns
/// Success when creation succeeds or an existing entry is deferred to
/// validation.
///
/// # Errors
/// Returns the native error for failures other than an existing-entry
/// collision.
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

#[cfg(all(test, unix))]
#[test]
fn test_host_temp_creation_error_releases_known_identity_owner() {
    let fixture = tempfile::tempdir().expect("fixture");
    let path = fixture.path().join("sandbox");
    fs::create_dir(&path).expect("create sandbox");
    let parent = File::open(fixture.path()).expect("open parent");
    let handle = File::open(&path).expect("open sandbox");
    let identity = crate::observation::query::query_observation(
        &handle,
        crate::observation::LocalObservationRequirement::Required,
    )
    .expect("observe sandbox identity")
    .identity()
    .cloned();
    let owner = crate::LocalTempRecovery::from_native(
        path.clone(),
        crate::temp::internal::NativePrivateEntry {
            parent: Arc::new(parent),
            name: OsStr::new("sandbox").to_os_string(),
            file: Some(Arc::new(handle)),
            identity,
            directory: true,
        },
    );
    let primary = LocalFileError::from_io(
        LocalFileOperation::CreateTempFile,
        Some(path.clone()),
        None,
        io::Error::other("injected creation failure"),
    )
    .with_temp_recovery(owner);

    let error = cleanup_failed_temp_creation_owner(primary);

    assert!(
        error.cleanup_error().is_none(),
        "successful cleanup adds no cleanup error"
    );
    assert!(error.temp_recovery().is_none(), "successful cleanup releases the owner");
    assert!(!path.exists(), "identity-checked cleanup removes the sandbox");
}

#[cfg(all(test, unix))]
#[test]
fn test_host_temp_directory_open_failure_retains_unauthoritative_entry() {
    let fixture = tempfile::tempdir().expect("fixture");
    let parent_handle = Arc::new(File::open(fixture.path()).expect("open parent"));
    let error = create_host_temp_directory_entry_with_opener(
        fixture.path(),
        parent_handle,
        Some("child-"),
        None,
        Some(1),
        LocalFileOperation::CreateTempDirectory,
        |_| Err(io::Error::other("injected directory open failure")),
    )
    .expect_err("injected open failure");
    let path = error.path().expect("diagnostic path").to_path_buf();
    assert!(path.is_dir(), "creation succeeded before the open failure");
    let recovery = error.temp_recovery().expect("creation failure retains the entry");
    assert_eq!(recovery.path(), path);
    assert!(
        recovery.identity().is_none(),
        "path text alone cannot establish identity"
    );
    let (_primary, mut recovery) = error.into_temp_recovery_parts();
    let mut recovery = recovery.take().expect("retained owner");
    assert_eq!(
        recovery
            .cleanup()
            .expect_err("unknown identity must not authorize deletion")
            .kind(),
        LocalFileErrorKind::RequirementNotMet
    );
    assert!(path.is_dir(), "path-only recovery must not delete the entry");
}

#[cfg(all(test, unix))]
#[test]
fn test_host_temp_recovery_does_not_claim_replaced_sandbox() {
    let fixture = tempfile::tempdir().expect("fixture");
    let parent = fixture.path();
    let (sandbox, sandbox_entry) = create_host_temp_sandbox(parent, LocalFileOperation::CreateTempDirectory, Some(1))
        .expect("create sandbox and retain its native owner");
    let original = sandbox.with_extension("original");
    fs::rename(&sandbox, &original).expect("move original sandbox");
    fs::create_dir(&sandbox).expect("create replacement sandbox");

    let (path, entries) =
        host_temp_recovery_entries(&sandbox, sandbox_entry, None, false).expect("construct recovery owner");
    let mut recovery = crate::LocalTempRecovery::from_native_entries_with_paths(path, entries);
    assert_eq!(
        recovery.cleanup().expect_err("replacement must be rejected").kind(),
        LocalFileErrorKind::RequirementNotMet
    );
    drop(recovery);

    assert!(sandbox.is_dir(), "replacement sandbox must not be deleted");
}

#[cfg(all(test, unix))]
#[test]
fn test_host_temp_recovery_does_not_claim_replaced_child_directory() {
    let fixture = tempfile::tempdir().expect("fixture");
    let (sandbox, sandbox_entry) =
        create_host_temp_sandbox(fixture.path(), LocalFileOperation::CreateTempDirectory, Some(1))
            .expect("create sandbox and retain its native owner");
    let sandbox_handle = Arc::clone(sandbox_entry.file.as_ref().expect("retain sandbox directory handle"));
    let (child, child_handle) = create_host_temp_directory_entry(
        &sandbox,
        sandbox_handle,
        Some("child-"),
        None,
        Some(1),
        LocalFileOperation::CreateTempDirectory,
    )
    .expect("create child directory and retain its handle");
    let original = child.with_extension("original");
    fs::rename(&child, &original).expect("move original child");
    fs::create_dir(&child).expect("create replacement child");

    let (path, entries) = host_temp_recovery_entries(&sandbox, sandbox_entry, Some((&child, child_handle)), true)
        .expect("construct recovery owner");
    let mut recovery = crate::LocalTempRecovery::from_native_entries_with_paths(path, entries);
    let _cleanup = recovery.cleanup();

    assert!(child.is_dir(), "replacement child directory must not be deleted");
}

#[cfg(all(test, unix))]
#[test]
fn test_host_temp_recovery_keeps_original_parent_handle_after_sandbox_replacement() {
    let fixture = tempfile::tempdir().expect("fixture");
    let (sandbox, sandbox_entry) =
        create_host_temp_sandbox(fixture.path(), LocalFileOperation::CreateTempDirectory, Some(1))
            .expect("create sandbox and retain its native owner");
    let original = sandbox.with_extension("original");
    fs::rename(&sandbox, &original).expect("move original sandbox");
    fs::create_dir(&sandbox).expect("create replacement sandbox");

    let sandbox_handle = Arc::clone(sandbox_entry.file.as_ref().expect("retain original sandbox handle"));
    let (child, child_handle) = create_host_temp_directory_entry(
        &sandbox,
        sandbox_handle,
        Some("child-"),
        None,
        Some(1),
        LocalFileOperation::CreateTempDirectory,
    )
    .expect("create child in replacement namespace");
    let (path, entries) = host_temp_recovery_entries(&sandbox, sandbox_entry, Some((&child, child_handle)), true)
        .expect("construct recovery using the retained sandbox handle");
    let mut recovery = crate::LocalTempRecovery::from_native_entries_with_paths(path, entries);

    let error = recovery
        .cleanup()
        .expect_err("the retained parent handle cannot see the replacement child");

    assert_eq!(error.path(), Some(child.as_path()));
    assert!(
        child.is_dir(),
        "cleanup must not re-open the replacement parent by path"
    );
}

#[cfg(all(test, unix))]
#[test]
fn test_host_temp_private_sandbox_cleanup_removes_owned_directory() {
    let fixture = tempfile::tempdir().expect("fixture");
    let (sandbox, owner) = create_host_temp_sandbox(fixture.path(), LocalFileOperation::CreateTempDirectory, Some(1))
        .expect("create sandbox and retain its native owner");
    let error = sandbox_failure(
        LocalFileOperation::CreateTempDirectory,
        sandbox.clone(),
        io::Error::other("injected sandbox failure"),
        None,
    );
    assert_eq!(error.operation(), LocalFileOperation::CreateTempDirectory);
    assert_eq!(error.path(), Some(sandbox.as_path()));
    assert!(error.temp_recovery().is_none(), "missing ownership stays explicit");

    cleanup_private_sandbox(&sandbox, owner).expect("clean up the owned sandbox");

    assert!(!sandbox.exists(), "successful private cleanup removes the sandbox");

    let (sandbox, owner) = create_host_temp_sandbox(fixture.path(), LocalFileOperation::CreateTempDirectory, Some(1))
        .expect("create another sandbox");
    let child = sandbox.join("child");
    fs::write(&child, b"temporary data").expect("create child file");
    let mut recovery = native_temp_recovery(
        child.clone(),
        File::open(&sandbox).expect("open child parent"),
        "child".to_owned(),
        File::open(&child).expect("open child file"),
        false,
        Some(owner),
    );
    assert_eq!(recovery.path(), child);
    recovery.cleanup().expect("clean up the child and sandbox owners");
    assert!(!child.exists(), "recovery removes the owned child");
    assert!(!sandbox.exists(), "recovery removes the owned sandbox");
}
