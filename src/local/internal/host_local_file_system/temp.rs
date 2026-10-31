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
use super::LocalWriteMode;
use super::LocalWriteOptions;
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
        let (sandbox, sandbox_entry) =
            create_host_temp_sandbox(&parent, LocalFileOperation::CreateTempFile, options.max_attempts())?;
        let created =
            crate::local::create_temp_file_in_dir(&sandbox, options.prefix(), options.suffix(), options.max_attempts());
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

fn create_host_temp_sandbox(
    parent: &Path,
    operation: LocalFileOperation,
    max_attempts: Option<usize>,
) -> LocalResult<(PathBuf, crate::temp::internal::NativePrivateEntry)> {
    let parent_handle = Arc::new(
        File::open(parent)
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

#[cfg(unix)]
fn open_temp_directory(path: &Path) -> io::Result<File> {
    #[cfg(feature = "test-support")]
    if crate::local::test_support_enabled("host-temp-sandbox-open") {
        return Err(io::Error::other("injected sandbox directory open failure"));
    }
    File::open(path)
}

#[cfg(windows)]
fn open_temp_directory(path: &Path) -> io::Result<File> {
    use std::os::windows::fs::OpenOptionsExt;

    use windows_sys::Win32::Storage::FileSystem::FILE_FLAG_BACKUP_SEMANTICS;
    use windows_sys::Win32::Storage::FileSystem::FILE_FLAG_OPEN_REPARSE_POINT;
    fs::OpenOptions::new()
        .read(true)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
        .open(path)
}

#[cfg(not(any(unix, windows)))]
fn open_temp_directory(path: &Path) -> io::Result<File> {
    File::open(path)
}

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

fn clone_host_temp_file(file: &File) -> io::Result<File> {
    #[cfg(feature = "test-support")]
    if crate::local::test_support_enabled("host-temp-file-recovery-clone-cleanup") {
        return Err(io::Error::other("injected temp recovery handle clone failure"));
    }
    file.try_clone()
}

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

#[cfg(any(unix, windows))]
/// Builds an error and transfers any retained native entries to recovery.
///
/// The creation error may own a partially created target; `extra` may own a
/// sandbox needed to reach that target. Both are kept in the returned recovery
/// value so the caller can retry cleanup after this function returns.
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

#[cfg(any(unix, windows))]
/// Creates recovery ownership for a native temporary entry and optional
/// sandbox.
///
/// The parent handle and name retain namespace access, while the file handle
/// retains the created object. Identity observation is best-effort and may be
/// absent; the returned owner carries both entries when a sandbox is supplied.
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

#[cfg(any(unix, windows))]
/// Clones the parent handle for later native cleanup or recovery.
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

#[cfg(any(unix, windows))]
/// Reserves recovery ownership for a created sandbox directory.
///
/// On failure, returns the parent handle to the caller so it can transfer that
/// handle and the still-open directory into a recovery value.
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

#[cfg(any(unix, windows))]
/// Associates an operation failure with recovery ownership of its sandbox.
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
/// Attempts sandbox cleanup and returns ownership when cleanup fails.
fn cleanup_private_sandbox(path: &Path, entry: crate::temp::types::NativePrivateEntry) -> LocalResult<()> {
    let mut owner = crate::LocalTempRecovery::from_native(path.to_path_buf(), entry);
    match owner.cleanup() {
        Ok(()) => Ok(()),
        Err(error) => Err(error.with_temp_recovery(owner)),
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

#[cfg(all(test, unix))]
#[test]
#[cfg(feature = "test-support")]
fn test_host_temp_file_clone_failure_transfers_owned_recovery_for_retry() {
    let fixture = tempfile::tempdir().expect("fixture");
    let guard = crate::local::install_test_fault("host-temp-file-recovery-clone-cleanup")
        .expect("install file clone and cleanup failure");
    let error = HostLocalFileSystem::create_temp_file_with_policy(
        &LocalTempFileOptions::new().with_parent(fixture.path()),
        LocalSymlinkPolicy::FollowWithinScope,
    )
    .expect_err("injected handle preparation failure");
    drop(guard);

    let (primary, recovery) = error.into_temp_recovery_parts();
    assert_eq!(primary.operation(), LocalFileOperation::CreateTempFile);
    let mut recovery = recovery.expect("failed preparation retains native cleanup ownership");
    assert!(recovery.identity().is_some(), "retained identity remains authoritative");
    let path = recovery.path().to_path_buf();
    assert!(path.exists(), "failed cleanup preserves the created file");
    recovery.cleanup().expect("retry cleans the file and sandbox");
    assert!(!path.exists(), "identity-checked retry removes the created file");
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
#[cfg(feature = "test-support")]
fn test_host_temp_sandbox_open_failure_reports_cleanup_error_and_retains_owner() {
    let fixture = tempfile::tempdir().expect("fixture");
    for (operation, error) in [
        {
            let guard = crate::local::install_test_fault("host-temp-sandbox-open").expect("install open failure");
            let error = HostLocalFileSystem::create_temp_file_with_policy(
                &LocalTempFileOptions::new().with_parent(fixture.path()),
                LocalSymlinkPolicy::FollowWithinScope,
            )
            .expect_err("sandbox open should fail");
            drop(guard);
            (LocalFileOperation::CreateTempFile, error)
        },
        {
            let guard = crate::local::install_test_fault("host-temp-sandbox-open").expect("install open failure");
            let error = HostLocalFileSystem::create_temp_directory_with_policy(
                &LocalTempDirectoryOptions::new().with_parent(fixture.path()),
                LocalSymlinkPolicy::FollowWithinScope,
            )
            .expect_err("sandbox open should fail");
            drop(guard);
            (LocalFileOperation::CreateTempDirectory, error)
        },
    ] {
        assert_eq!(error.operation(), operation);
        assert_eq!(
            error.cleanup_error().expect("cleanup failure is reported").kind(),
            LocalFileErrorKind::RequirementNotMet
        );
        let recovery = error.temp_recovery().expect("sandbox recovery owner is retained");
        let path = recovery.path().to_path_buf();
        assert!(path.is_dir(), "unknown identity must preserve the residual sandbox");
        assert!(recovery.identity().is_none(), "no path-derived identity is trusted");
        let (_primary, mut recovery) = error.into_temp_recovery_parts();
        let mut recovery = recovery.take().expect("owner transfers from public error");
        assert_eq!(
            recovery
                .cleanup()
                .expect_err("unknown identity is not cleanup authority")
                .kind(),
            LocalFileErrorKind::RequirementNotMet
        );
        assert!(path.is_dir(), "cleanup must not remove by diagnostic path");
    }
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
