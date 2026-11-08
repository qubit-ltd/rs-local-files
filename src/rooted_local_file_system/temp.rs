// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

// Rooted temp operations.

use std::ffi::OsStr;
use std::fs::File;

#[path = "temp_creation_recovery.rs"]
mod creation_recovery;

use super::Arc;
use super::LocalFileErrorKind;
use super::LocalFileOperation;
use super::LocalResult;
use super::LocalSymlinkPolicy;
use super::LocalTempDirectory;
use super::LocalTempDirectoryOptions;
use super::LocalTempFile;
use super::LocalTempFileOptions;
use super::RootedLocalFileSystem;
use super::io;
use super::resolve_rooted_path_allow_root;
use super::rooted_io_error;
use super::rooted_path;
use super::rooted_temp_parent;
use super::temp_candidate;
use super::validate_rooted_temp_parent;
use super::validate_temp_affixes;
#[cfg(feature = "test-support")]
use crate::local::test_support_enabled;
use crate::local::try_random_file_name;
use crate::policy::LocalPrivacyRequirement;
use crate::temp::types::NativePrivateCreateError;
use crate::temp::types::NativePrivateEntry;

impl RootedLocalFileSystem {
    /// Creates a cleanup-owned temporary file below this opened root.
    ///
    /// The optional parent must be a validated rooted descendant. The returned
    /// resource retains this exact opened root authority, so later cleanup is
    /// unaffected by rename or replacement of the diagnostic root path.
    ///
    /// # Parameters
    ///
    /// - `options`: Parent, name affixes, privacy requirement, and retry limit
    ///   for the temporary file.
    /// - `symlink_policy`: Policy used when resolving the optional parent below
    ///   the opened root.
    ///
    /// # Returns
    ///
    /// The cleanup-owned temporary file and its rooted cleanup authority.
    ///
    /// # Panics
    ///
    /// Panics only if an internally generated sandbox path has no file name.
    ///
    /// # Errors
    /// Returns `LocalFileError` when options are invalid, entry creation
    /// collides through all attempts, or rooted traversal/opening fails.
    pub fn create_temp_file(
        &self,
        options: &LocalTempFileOptions,
        symlink_policy: LocalSymlinkPolicy,
    ) -> LocalResult<LocalTempFile> {
        let requested_parent = rooted_temp_parent(options.parent(), LocalFileOperation::CreateTempFile)?;
        let parent = if requested_parent.as_os_str().is_empty() {
            requested_parent
        } else {
            resolve_rooted_path_allow_root(
                &self.root,
                &requested_parent,
                symlink_policy,
                true,
                LocalFileOperation::CreateTempFile,
            )?
        };
        if options.creates_parent() && !parent.as_os_str().is_empty() {
            let parent_path = rooted_path(&parent, LocalFileOperation::CreateTempFile)?;
            if let Err(error) = self.root.create_dir_all(&parent_path) {
                return Err(rooted_io_error(LocalFileOperation::CreateTempFile, &parent, error));
            }
        }
        validate_rooted_temp_parent(&self.root, &parent, LocalFileOperation::CreateTempFile)?;
        if options.max_attempts() == Some(0) {
            return Err(rooted_io_error(
                LocalFileOperation::CreateTempFile,
                &parent,
                io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "temporary entry retry count must be greater than zero",
                ),
            )
            .with_kind(LocalFileErrorKind::InvalidOptions));
        }
        if let Err(error) = validate_temp_affixes(options.prefix(), options.suffix()) {
            return Err(rooted_io_error(LocalFileOperation::CreateTempFile, &parent, error)
                .with_kind(LocalFileErrorKind::InvalidOptions));
        }
        if options.privacy() == LocalPrivacyRequirement::Required {
            return self.create_private_rooted_temp_file(&parent, options, symlink_policy);
        }
        let parent_handle = open_rooted_temp_parent(&self.root, &parent, LocalFileOperation::CreateTempFile)?;
        let mut attempts = 0_usize;
        loop {
            attempts = attempts.saturating_add(1);
            let resource_name = match try_random_file_name("qubit-local-files-", options.prefix(), options.suffix()) {
                Ok(name) => name,
                Err(error) => {
                    return Err(rooted_io_error(LocalFileOperation::CreateTempFile, &parent, error));
                }
            };
            let sandbox = temp_candidate(&parent, Some("sandbox-"), None, LocalFileOperation::CreateTempFile)?;
            let sandbox_name = sandbox.file_name().expect("generated sandbox has a name");
            let sandbox_handle = match self.root.create_temp_child(&parent_handle, sandbox_name, true) {
                Ok(handle) => handle,
                Err(error) if error.retained.is_none() && error.source.kind() == io::ErrorKind::AlreadyExists => {
                    if attempt_budget_exhausted(attempts, options.max_attempts()) {
                        break;
                    }
                    continue;
                }
                Err(error) => {
                    let primary = rooted_io_error(LocalFileOperation::CreateTempFile, &sandbox, error.source);
                    return Err(match error.retained {
                        Some(entry) => creation_recovery::finish(primary, vec![(sandbox.clone(), entry)]),
                        None => primary,
                    });
                }
            };
            let sandbox_parent = match parent_handle.try_clone() {
                Ok(parent) => parent,
                Err(error) => {
                    let primary = rooted_io_error(LocalFileOperation::CreateTempFile, &sandbox, error);
                    let entry = creation_recovery::entry(parent_handle, sandbox_name, sandbox_handle, true);
                    return Err(creation_recovery::finish(primary, vec![(sandbox.clone(), entry)]));
                }
            };
            let sandbox_file = match sandbox_handle.try_clone() {
                Ok(file) => file,
                Err(error) => {
                    let primary = rooted_io_error(LocalFileOperation::CreateTempFile, &sandbox, error);
                    let entry = creation_recovery::entry(sandbox_parent, sandbox_name, sandbox_handle, true);
                    return Err(creation_recovery::finish(primary, vec![(sandbox.clone(), entry)]));
                }
            };
            let sandbox_entry = creation_recovery::entry(sandbox_parent, sandbox_name, sandbox_file, true);
            let candidate = sandbox.join(resource_name);
            #[cfg(feature = "test-support")]
            let opened = if test_support_enabled("rooted-temp-file-collision-cleanup") {
                Err(NativePrivateCreateError {
                    source: io::Error::from(io::ErrorKind::AlreadyExists),
                    retained: None,
                })
            } else if test_support_enabled("rooted-temp-file-open-cleanup") {
                Err(NativePrivateCreateError {
                    source: io::Error::from(io::ErrorKind::PermissionDenied),
                    retained: None,
                })
            } else if test_support_enabled("rooted-temp-file-collision") {
                Err(NativePrivateCreateError {
                    source: io::Error::from(io::ErrorKind::AlreadyExists),
                    retained: None,
                })
            } else if test_support_enabled("rooted-temp-file-open") {
                Err(NativePrivateCreateError {
                    source: io::Error::from(io::ErrorKind::PermissionDenied),
                    retained: None,
                })
            } else {
                self.root.create_temp_child(
                    &sandbox_handle,
                    candidate.file_name().expect("generated child has a name"),
                    false,
                )
            };
            #[cfg(not(feature = "test-support"))]
            let opened = self.root.create_temp_child(
                &sandbox_handle,
                candidate.file_name().expect("generated child has a name"),
                false,
            );
            match opened {
                Ok(file) => {
                    #[cfg(feature = "test-support")]
                    let recovery_file = if test_support_enabled("rooted-temp-file-recovery-clone-cleanup") {
                        Err(io::Error::other("injected rooted temp recovery file clone failure"))
                    } else {
                        file.try_clone()
                    };
                    #[cfg(not(feature = "test-support"))]
                    let recovery_file = file.try_clone();
                    let recovery_file = match recovery_file {
                        Ok(file) => file,
                        Err(error) => {
                            let primary = rooted_io_error(LocalFileOperation::CreateTempFile, &candidate, error);
                            let child = creation_recovery::entry(
                                sandbox_handle,
                                candidate.file_name().expect("generated child has a name"),
                                file,
                                false,
                            );
                            return Err(creation_recovery::finish(
                                primary,
                                vec![(candidate.clone(), child), (sandbox.clone(), sandbox_entry)],
                            ));
                        }
                    };
                    let child_parent = match sandbox_handle.try_clone() {
                        Ok(parent) => parent,
                        Err(error) => {
                            let primary = rooted_io_error(LocalFileOperation::CreateTempFile, &candidate, error);
                            let child = creation_recovery::entry(
                                sandbox_handle,
                                candidate.file_name().expect("generated child has a name"),
                                recovery_file,
                                false,
                            );
                            return Err(creation_recovery::finish(
                                primary,
                                vec![(candidate.clone(), child), (sandbox.clone(), sandbox_entry)],
                            ));
                        }
                    };
                    let child_entry = creation_recovery::entry(
                        child_parent,
                        candidate.file_name().expect("generated child has a name"),
                        recovery_file,
                        false,
                    );
                    #[cfg(feature = "test-support")]
                    let result = if test_support_enabled("rooted-temp-file-owner-cleanup") {
                        Err(io::Error::other(
                            "injected rooted temporary file owner construction failure",
                        ))
                    } else {
                        LocalTempFile::rooted(
                            Arc::clone(&self.root),
                            candidate.clone(),
                            sandbox.clone(),
                            file,
                            symlink_policy,
                        )
                    };
                    #[cfg(not(feature = "test-support"))]
                    let result = LocalTempFile::rooted(
                        Arc::clone(&self.root),
                        candidate.clone(),
                        sandbox.clone(),
                        file,
                        symlink_policy,
                    );
                    return match result {
                        Ok(resource) => Ok(resource),
                        Err(error) => {
                            let primary = rooted_io_error(LocalFileOperation::CreateTempFile, &candidate, error);
                            Err(creation_recovery::finish(
                                primary,
                                vec![(candidate.clone(), child_entry), (sandbox.clone(), sandbox_entry)],
                            ))
                        }
                    };
                }
                Err(error) if error.retained.is_none() && error.source.kind() == io::ErrorKind::AlreadyExists => {
                    let primary = rooted_io_error(LocalFileOperation::CreateTempFile, &candidate, error.source);
                    let error = creation_recovery::finish(primary, vec![(sandbox.clone(), sandbox_entry)]);
                    if error.temp_recovery().is_some() {
                        return Err(error);
                    }
                    if attempt_budget_exhausted(attempts, options.max_attempts()) {
                        break;
                    }
                    continue;
                }
                Err(error) => {
                    let primary = rooted_io_error(LocalFileOperation::CreateTempFile, &candidate, error.source);
                    let mut entries = Vec::new();
                    if let Some(child) = error.retained {
                        entries.push((candidate.clone(), child));
                    }
                    entries.push((sandbox.clone(), sandbox_entry));
                    return Err(creation_recovery::finish(primary, entries));
                }
            }
        }
        Err(rooted_io_error(
            LocalFileOperation::CreateTempFile,
            &parent,
            io::Error::new(io::ErrorKind::AlreadyExists, "temporary file name attempts exhausted"),
        ))
    }

    /// Creates a cleanup-owned temporary directory below this opened root.
    ///
    /// The optional parent must be a validated rooted descendant. The returned
    /// directory retains this exact opened root authority for recursive
    /// cleanup.
    ///
    /// # Parameters
    ///
    /// - `options`: Parent, name affixes, privacy requirement, retry limit, and
    ///   cleanup limits for the temporary directory.
    /// - `symlink_policy`: Policy used when resolving the optional parent below
    ///   the opened root.
    ///
    /// # Returns
    ///
    /// The cleanup-owned temporary directory and its rooted cleanup authority.
    ///
    /// # Panics
    ///
    /// Panics only if an internally generated sandbox path has no file name.
    ///
    /// # Errors
    /// Returns `LocalFileError` when options are invalid, entry creation
    /// collides through all attempts, or rooted traversal/creation fails.
    pub fn create_temp_directory(
        &self,
        options: &LocalTempDirectoryOptions,
        symlink_policy: LocalSymlinkPolicy,
    ) -> LocalResult<LocalTempDirectory> {
        let requested_parent = rooted_temp_parent(options.parent(), LocalFileOperation::CreateTempDirectory)?;
        let parent = if requested_parent.as_os_str().is_empty() {
            requested_parent
        } else {
            resolve_rooted_path_allow_root(
                &self.root,
                &requested_parent,
                symlink_policy,
                true,
                LocalFileOperation::CreateTempDirectory,
            )?
        };
        if options.creates_parent() && !parent.as_os_str().is_empty() {
            let parent_path = rooted_path(&parent, LocalFileOperation::CreateTempDirectory)?;
            if let Err(error) = self.root.create_dir_all(&parent_path) {
                return Err(rooted_io_error(LocalFileOperation::CreateTempDirectory, &parent, error));
            }
        }
        validate_rooted_temp_parent(&self.root, &parent, LocalFileOperation::CreateTempDirectory)?;
        if options.max_attempts() == Some(0) {
            return Err(rooted_io_error(
                LocalFileOperation::CreateTempDirectory,
                &parent,
                io::Error::new(
                    io::ErrorKind::InvalidInput,
                    "temporary entry retry count must be greater than zero",
                ),
            )
            .with_kind(LocalFileErrorKind::InvalidOptions));
        }
        if let Err(error) = validate_temp_affixes(options.prefix(), options.suffix()) {
            return Err(rooted_io_error(LocalFileOperation::CreateTempDirectory, &parent, error)
                .with_kind(LocalFileErrorKind::InvalidOptions));
        }
        if options.privacy() == LocalPrivacyRequirement::Required {
            return self.create_private_rooted_temp_directory(&parent, options, symlink_policy);
        }
        let parent_handle = open_rooted_temp_parent(&self.root, &parent, LocalFileOperation::CreateTempDirectory)?;
        let mut attempts = 0_usize;
        loop {
            attempts = attempts.saturating_add(1);
            let resource_name = match try_random_file_name("qubit-local-files-", options.prefix(), options.suffix()) {
                Ok(name) => name,
                Err(error) => {
                    return Err(rooted_io_error(LocalFileOperation::CreateTempDirectory, &parent, error));
                }
            };
            let sandbox = temp_candidate(&parent, Some("sandbox-"), None, LocalFileOperation::CreateTempDirectory)?;
            let sandbox_name = sandbox.file_name().expect("generated sandbox has a name");
            let sandbox_handle = match self.root.create_temp_child(&parent_handle, sandbox_name, true) {
                Ok(handle) => handle,
                Err(error) if error.retained.is_none() && error.source.kind() == io::ErrorKind::AlreadyExists => {
                    if attempt_budget_exhausted(attempts, options.max_attempts()) {
                        break;
                    }
                    continue;
                }
                Err(error) => {
                    let primary = rooted_io_error(LocalFileOperation::CreateTempDirectory, &sandbox, error.source);
                    return Err(match error.retained {
                        Some(entry) => creation_recovery::finish(primary, vec![(sandbox.clone(), entry)]),
                        None => primary,
                    });
                }
            };
            let sandbox_parent = match parent_handle.try_clone() {
                Ok(parent) => parent,
                Err(error) => {
                    let primary = rooted_io_error(LocalFileOperation::CreateTempDirectory, &sandbox, error);
                    let entry = creation_recovery::entry(parent_handle, sandbox_name, sandbox_handle, true);
                    return Err(creation_recovery::finish(primary, vec![(sandbox.clone(), entry)]));
                }
            };
            let sandbox_file = match sandbox_handle.try_clone() {
                Ok(file) => file,
                Err(error) => {
                    let primary = rooted_io_error(LocalFileOperation::CreateTempDirectory, &sandbox, error);
                    let entry = creation_recovery::entry(sandbox_parent, sandbox_name, sandbox_handle, true);
                    return Err(creation_recovery::finish(primary, vec![(sandbox.clone(), entry)]));
                }
            };
            let sandbox_entry = creation_recovery::entry(sandbox_parent, sandbox_name, sandbox_file, true);
            let candidate = sandbox.join(resource_name);
            #[cfg(feature = "test-support")]
            let created = if test_support_enabled("rooted-temp-directory-collision-cleanup") {
                Err(NativePrivateCreateError {
                    source: io::Error::from(io::ErrorKind::AlreadyExists),
                    retained: None,
                })
            } else if test_support_enabled("rooted-temp-directory-create-cleanup") {
                Err(NativePrivateCreateError {
                    source: io::Error::from(io::ErrorKind::PermissionDenied),
                    retained: None,
                })
            } else if test_support_enabled("rooted-temp-directory-collision") {
                Err(NativePrivateCreateError {
                    source: io::Error::from(io::ErrorKind::AlreadyExists),
                    retained: None,
                })
            } else if test_support_enabled("rooted-temp-directory-create") {
                Err(NativePrivateCreateError {
                    source: io::Error::from(io::ErrorKind::PermissionDenied),
                    retained: None,
                })
            } else {
                self.root.create_temp_child(
                    &sandbox_handle,
                    candidate.file_name().expect("generated child has a name"),
                    true,
                )
            };
            #[cfg(not(feature = "test-support"))]
            let created = self.root.create_temp_child(
                &sandbox_handle,
                candidate.file_name().expect("generated child has a name"),
                true,
            );
            match created {
                Ok(child_handle) => {
                    #[cfg(feature = "test-support")]
                    let child_file = if test_support_enabled("rooted-temp-directory-recovery-clone-cleanup") {
                        Err(io::Error::other(
                            "injected rooted temp recovery directory clone failure",
                        ))
                    } else {
                        child_handle.try_clone()
                    };
                    #[cfg(not(feature = "test-support"))]
                    let child_file = child_handle.try_clone();
                    let child_file = match child_file {
                        Ok(file) => file,
                        Err(error) => {
                            let primary = rooted_io_error(LocalFileOperation::CreateTempDirectory, &candidate, error);
                            let child = creation_recovery::entry(
                                sandbox_handle,
                                candidate.file_name().expect("generated child has a name"),
                                child_handle,
                                true,
                            );
                            return Err(creation_recovery::finish(
                                primary,
                                vec![(candidate.clone(), child), (sandbox.clone(), sandbox_entry)],
                            ));
                        }
                    };
                    let child_parent = match sandbox_handle.try_clone() {
                        Ok(parent) => parent,
                        Err(error) => {
                            let primary = rooted_io_error(LocalFileOperation::CreateTempDirectory, &candidate, error);
                            let child = creation_recovery::entry(
                                sandbox_handle,
                                candidate.file_name().expect("generated child has a name"),
                                child_handle,
                                true,
                            );
                            return Err(creation_recovery::finish(
                                primary,
                                vec![(candidate.clone(), child), (sandbox.clone(), sandbox_entry)],
                            ));
                        }
                    };
                    let child_entry = creation_recovery::entry(
                        child_parent,
                        candidate.file_name().expect("generated child has a name"),
                        child_file,
                        true,
                    );
                    #[cfg(feature = "test-support")]
                    let result = if test_support_enabled("rooted-temp-directory-owner-cleanup") {
                        Err(io::Error::other(
                            "injected rooted temporary directory owner construction failure",
                        ))
                    } else {
                        LocalTempDirectory::rooted_from_created_handle(
                            Arc::clone(&self.root),
                            candidate.clone(),
                            sandbox.clone(),
                            &child_handle,
                            symlink_policy,
                            options.cleanup_limits(),
                        )
                    };
                    #[cfg(not(feature = "test-support"))]
                    let result = LocalTempDirectory::rooted_from_created_handle(
                        Arc::clone(&self.root),
                        candidate.clone(),
                        sandbox.clone(),
                        &child_handle,
                        symlink_policy,
                        options.cleanup_limits(),
                    );
                    return match result {
                        Ok(resource) => Ok(resource),
                        Err(error) => {
                            let primary = rooted_io_error(LocalFileOperation::CreateTempDirectory, &candidate, error);
                            Err(creation_recovery::finish(
                                primary,
                                vec![(candidate.clone(), child_entry), (sandbox.clone(), sandbox_entry)],
                            ))
                        }
                    };
                }
                Err(error) if error.retained.is_none() && error.source.kind() == io::ErrorKind::AlreadyExists => {
                    let primary = rooted_io_error(LocalFileOperation::CreateTempDirectory, &candidate, error.source);
                    let error = creation_recovery::finish(primary, vec![(sandbox.clone(), sandbox_entry)]);
                    if error.temp_recovery().is_some() {
                        return Err(error);
                    }
                    if attempt_budget_exhausted(attempts, options.max_attempts()) {
                        break;
                    }
                    continue;
                }
                Err(error) => {
                    let primary = rooted_io_error(LocalFileOperation::CreateTempDirectory, &candidate, error.source);
                    let mut entries = Vec::new();
                    if let Some(child) = error.retained {
                        entries.push((candidate.clone(), child));
                    }
                    entries.push((sandbox.clone(), sandbox_entry));
                    return Err(creation_recovery::finish(primary, entries));
                }
            }
        }
        Err(rooted_io_error(
            LocalFileOperation::CreateTempDirectory,
            &parent,
            io::Error::new(
                io::ErrorKind::AlreadyExists,
                "temporary directory name attempts exhausted",
            ),
        ))
    }

    /// Creates a private temporary file inside a reserved sandbox directory.
    ///
    /// The opened parent handle anchors creation beneath this root, and the
    /// returned resource retains recovery handles if owner construction fails.
    ///
    /// # Parameters
    ///
    /// - `parent`: Validated relative directory beneath the opened root.
    /// - `options`: Name affixes and retry limit for the private file.
    /// - `symlink_policy`: Policy retained by the resulting temporary resource.
    ///
    /// # Returns
    ///
    /// The owned private file, or an error carrying any retained recovery
    /// state.
    fn create_private_rooted_temp_file(
        &self,
        parent: &std::path::Path,
        options: &LocalTempFileOptions,
        symlink_policy: LocalSymlinkPolicy,
    ) -> LocalResult<LocalTempFile> {
        let parent_handle = open_rooted_temp_parent(&self.root, parent, LocalFileOperation::CreateTempFile)?;
        let mut attempts = 0_usize;
        loop {
            if attempt_budget_exhausted(attempts, options.max_attempts()) {
                break;
            }
            attempts = attempts.saturating_add(1);
            let recovery_parent = reserve_rooted_private_parent(&parent_handle)
                .map_err(|error| rooted_io_error(LocalFileOperation::CreateTempFile, parent, error))?;
            let sandbox_name = try_random_file_name("sandbox-", None, None)
                .map_err(|error| rooted_io_error(LocalFileOperation::CreateTempFile, parent, error))?;
            let sandbox = parent.join(&sandbox_name);
            let _sandbox_relative = rooted_path(&sandbox, LocalFileOperation::CreateTempFile)?;
            #[cfg(feature = "test-support")]
            let sandbox_result = if crate::local::take_repeated_test_support("temp-private-sandbox-collision") {
                Err(NativePrivateCreateError {
                    source: io::Error::from(io::ErrorKind::AlreadyExists),
                    retained: None,
                })
            } else {
                rooted_private_directory(&parent_handle, OsStr::new(&sandbox_name))
            };
            #[cfg(not(feature = "test-support"))]
            let sandbox_result = rooted_private_directory(&parent_handle, OsStr::new(&sandbox_name));
            let sandbox_handle = match sandbox_result {
                Ok(handle) => handle,
                Err(error) if error.retained.is_none() && error.source.kind() == io::ErrorKind::AlreadyExists => {
                    continue;
                }
                Err(error) => {
                    return Err(private_rooted_error(
                        LocalFileOperation::CreateTempFile,
                        &sandbox,
                        error,
                        None,
                    ));
                }
            };
            let sandbox_guard = match reserve_rooted_directory_guard(recovery_parent, &sandbox_name, &sandbox_handle) {
                Ok(guard) => guard,
                Err((error, recovery_parent)) => {
                    let owner = rooted_temp_recovery(
                        sandbox.clone(),
                        recovery_parent,
                        sandbox_name,
                        sandbox_handle,
                        true,
                        None,
                    );
                    return Err(
                        rooted_io_error(LocalFileOperation::CreateTempFile, &sandbox, error).with_temp_recovery(owner)
                    );
                }
            };
            let name = match try_random_file_name("qubit-local-files-", options.prefix(), options.suffix()) {
                Ok(name) => name,
                Err(error) => {
                    return Err(rooted_sandbox_error(
                        LocalFileOperation::CreateTempFile,
                        &sandbox,
                        error,
                        Some(sandbox_guard),
                    ));
                }
            };
            let file = match rooted_private_file(&sandbox_handle, OsStr::new(&name)) {
                Ok(file) => file,
                Err(error) if error.retained.is_none() && error.source.kind() == io::ErrorKind::AlreadyExists => {
                    cleanup_rooted_sandbox(&sandbox, sandbox_guard)?;
                    continue;
                }
                Err(error) => {
                    return Err(private_rooted_error(
                        LocalFileOperation::CreateTempFile,
                        &sandbox.join(&name),
                        error,
                        Some(sandbox_guard),
                    ));
                }
            };
            let target = sandbox.join(&name);
            let owner_file = match file.try_clone() {
                Ok(file) => file,
                Err(error) => {
                    let recovery = rooted_temp_recovery(
                        target.clone(),
                        sandbox_handle,
                        name.clone(),
                        file,
                        false,
                        Some(sandbox_guard),
                    );
                    return Err(rooted_io_error(LocalFileOperation::CreateTempFile, &target, error)
                        .with_temp_recovery(recovery));
                }
            };
            return match LocalTempFile::rooted(
                Arc::clone(&self.root),
                target.clone(),
                sandbox.clone(),
                owner_file,
                symlink_policy,
            ) {
                Ok(resource) => Ok(resource),
                Err(error) => {
                    Err(
                        rooted_io_error(LocalFileOperation::CreateTempFile, &target, error).with_temp_recovery(
                            rooted_temp_recovery(target, sandbox_handle, name, file, false, Some(sandbox_guard)),
                        ),
                    )
                }
            };
        }
        Err(rooted_io_error(
            LocalFileOperation::CreateTempFile,
            parent,
            io::Error::new(io::ErrorKind::AlreadyExists, "private temp name attempts exhausted"),
        ))
    }

    /// Creates a private temporary directory inside a reserved sandbox.
    ///
    /// The returned resource retains the opened root authority and cleanup
    /// limits; failures preserve native handles needed for recovery.
    ///
    /// # Parameters
    ///
    /// - `parent`: Validated relative directory beneath the opened root.
    /// - `options`: Name affixes, retry limit, and cleanup limits.
    /// - `symlink_policy`: Policy retained by the resulting temporary resource.
    ///
    /// # Returns
    ///
    /// The owned private directory, or an error carrying any retained recovery
    /// state.
    fn create_private_rooted_temp_directory(
        &self,
        parent: &std::path::Path,
        options: &LocalTempDirectoryOptions,
        symlink_policy: LocalSymlinkPolicy,
    ) -> LocalResult<LocalTempDirectory> {
        let parent_handle = open_rooted_temp_parent(&self.root, parent, LocalFileOperation::CreateTempDirectory)?;
        let mut attempts = 0_usize;
        loop {
            if attempt_budget_exhausted(attempts, options.max_attempts()) {
                break;
            }
            attempts = attempts.saturating_add(1);
            let recovery_parent = reserve_rooted_private_parent(&parent_handle)
                .map_err(|error| rooted_io_error(LocalFileOperation::CreateTempDirectory, parent, error))?;
            let sandbox_name = try_random_file_name("sandbox-", None, None)
                .map_err(|error| rooted_io_error(LocalFileOperation::CreateTempDirectory, parent, error))?;
            let sandbox = parent.join(&sandbox_name);
            let _sandbox_relative = rooted_path(&sandbox, LocalFileOperation::CreateTempDirectory)?;
            #[cfg(feature = "test-support")]
            let sandbox_result = if crate::local::take_repeated_test_support("temp-private-sandbox-collision") {
                Err(NativePrivateCreateError {
                    source: io::Error::from(io::ErrorKind::AlreadyExists),
                    retained: None,
                })
            } else {
                rooted_private_directory(&parent_handle, OsStr::new(&sandbox_name))
            };
            #[cfg(not(feature = "test-support"))]
            let sandbox_result = rooted_private_directory(&parent_handle, OsStr::new(&sandbox_name));
            let sandbox_handle = match sandbox_result {
                Ok(handle) => handle,
                Err(error) if error.retained.is_none() && error.source.kind() == io::ErrorKind::AlreadyExists => {
                    continue;
                }
                Err(error) => {
                    return Err(private_rooted_error(
                        LocalFileOperation::CreateTempDirectory,
                        &sandbox,
                        error,
                        None,
                    ));
                }
            };
            let sandbox_guard = match reserve_rooted_directory_guard(recovery_parent, &sandbox_name, &sandbox_handle) {
                Ok(guard) => guard,
                Err((error, recovery_parent)) => {
                    let owner = rooted_temp_recovery(
                        sandbox.clone(),
                        recovery_parent,
                        sandbox_name,
                        sandbox_handle,
                        true,
                        None,
                    );
                    return Err(
                        rooted_io_error(LocalFileOperation::CreateTempDirectory, &sandbox, error)
                            .with_temp_recovery(owner),
                    );
                }
            };
            let name = match try_random_file_name("qubit-local-files-", options.prefix(), options.suffix()) {
                Ok(name) => name,
                Err(error) => {
                    return Err(rooted_sandbox_error(
                        LocalFileOperation::CreateTempDirectory,
                        &sandbox,
                        error,
                        Some(sandbox_guard),
                    ));
                }
            };
            #[cfg(feature = "test-support")]
            let target_created = if crate::local::take_repeated_test_support("temp-private-directory-target-collision")
            {
                Err(NativePrivateCreateError {
                    source: io::Error::from(io::ErrorKind::AlreadyExists),
                    retained: None,
                })
            } else {
                rooted_private_directory(&sandbox_handle, OsStr::new(&name))
            };
            #[cfg(not(feature = "test-support"))]
            let target_created = rooted_private_directory(&sandbox_handle, OsStr::new(&name));
            match target_created {
                Ok(target_handle) => {
                    let target = sandbox.join(&name);
                    return match LocalTempDirectory::rooted(
                        Arc::clone(&self.root),
                        target.clone(),
                        sandbox.clone(),
                        symlink_policy,
                        options.cleanup_limits(),
                    ) {
                        Ok(resource) => Ok(resource),
                        Err(error) => Err(rooted_io_error(LocalFileOperation::CreateTempDirectory, &target, error)
                            .with_temp_recovery(rooted_temp_recovery(
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
                    cleanup_rooted_sandbox(&sandbox, sandbox_guard)?;
                }
                Err(error) => {
                    return Err(private_rooted_error(
                        LocalFileOperation::CreateTempDirectory,
                        &sandbox.join(name),
                        error,
                        Some(sandbox_guard),
                    ));
                }
            }
        }
        Err(rooted_io_error(
            LocalFileOperation::CreateTempDirectory,
            parent,
            io::Error::new(io::ErrorKind::AlreadyExists, "private temp name attempts exhausted"),
        ))
    }
}

/// Opens the rooted parent handle used to create and recover a temporary entry.
///
/// An empty relative path selects the already-open root; other paths are
/// converted to a rooted path before opening. Open failures retain the caller's
/// operation and parent path in the resulting error.
///
/// # Parameters
///
/// - `root`: Opened rooted authority used to open the parent.
/// - `parent`: Validated relative parent path, or an empty path for the root.
/// - `operation`: Operation recorded if opening the parent fails.
///
/// # Returns
///
/// The opened parent handle, or a rooted error preserving the operation/path.
fn open_rooted_temp_parent(
    root: &crate::rooted::Root,
    parent: &std::path::Path,
    operation: LocalFileOperation,
) -> LocalResult<File> {
    let handle = if parent.as_os_str().is_empty() {
        root.open_probe_root()
    } else {
        root.open_probe_file(&rooted_path(parent, operation)?)
    };
    handle.map_err(|error| rooted_io_error(operation, parent, error))
}

/// Reports whether an explicitly configured collision budget is exhausted.
///
/// # Parameters
///
/// - `attempts`: Number of collision attempts already made.
/// - `max_attempts`: Optional configured upper bound.
///
/// # Returns
///
/// `true` when a configured limit has been reached; an absent limit is never
/// exhausted by this check.
#[must_use]
fn attempt_budget_exhausted(attempts: usize, max_attempts: Option<usize>) -> bool {
    max_attempts.is_some_and(|max_attempts| attempts >= max_attempts)
}

/// Creates a platform-private file relative to an opened parent handle.
///
/// On unsupported platforms, returns an `Unsupported` error. A failed native
/// creation may retain an entry handle for cleanup or recovery.
///
/// # Parameters
///
/// - `parent`: Open directory handle that anchors the new file.
/// - `name`: Single native filename to create beneath `parent`.
///
/// # Returns
///
/// The opened file handle, or the native error and any retained cleanup entry.
fn rooted_private_file(parent: &File, name: &OsStr) -> Result<File, NativePrivateCreateError> {
    #[cfg(feature = "test-support")]
    if crate::local::take_repeated_test_support("temp-private-file-collision") {
        return Err(NativePrivateCreateError {
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
    #[cfg(not(any(unix, windows)))]
    {
        let _ = (parent, name);
        Err(NativePrivateCreateError {
            source: io::Error::new(io::ErrorKind::Unsupported, "private temp files unsupported"),
            retained: None,
        })
    }
}

/// Creates a platform-private directory relative to an opened parent handle.
///
/// On unsupported platforms, returns an `Unsupported` error. A failed native
/// creation may retain an entry handle for cleanup or recovery.
///
/// # Parameters
///
/// - `parent`: Open directory handle that anchors the new directory.
/// - `name`: Single native directory name to create beneath `parent`.
///
/// # Returns
///
/// The opened directory handle, or the native error and any retained cleanup
/// entry.
fn rooted_private_directory(parent: &File, name: &OsStr) -> Result<File, NativePrivateCreateError> {
    #[cfg(feature = "test-support")]
    if crate::local::take_repeated_test_support("temp-private-directory-collision") {
        return Err(NativePrivateCreateError {
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
    #[cfg(not(any(unix, windows)))]
    {
        let _ = (parent, name);
        Err(NativePrivateCreateError {
            source: io::Error::new(io::ErrorKind::Unsupported, "private temp directories unsupported"),
            retained: None,
        })
    }
}

/// Converts a private-creation failure into a rooted error with recovery state.
///
/// Any retained target entry and optional sandbox guard are attached so the
/// caller can inspect or clean up resources left by a partial creation.
///
/// # Parameters
///
/// - `operation`: Operation reported by the resulting rooted error.
/// - `path`: Diagnostic path of the failed private entry.
/// - `error`: Native failure and optional retained target entry.
/// - `extra`: Optional recovery entry for the containing sandbox.
///
/// # Returns
///
/// A rooted error carrying recovery state for every retained entry.
fn private_rooted_error(
    operation: LocalFileOperation,
    path: &std::path::Path,
    error: NativePrivateCreateError,
    extra: Option<NativePrivateEntry>,
) -> crate::LocalFileError {
    let mut result = rooted_io_error(operation, path, error.source);
    let sandbox_path = path.parent().unwrap_or_else(|| std::path::Path::new(""));
    let mut entries = error
        .retained
        .into_iter()
        .map(|entry| (path.to_path_buf(), entry))
        .collect::<Vec<_>>();
    if let Some(extra) = extra {
        entries.push((sandbox_path.to_path_buf(), extra));
    }
    if !entries.is_empty() {
        result = result.with_temp_recovery(crate::LocalTempRecovery::from_native_entries_with_paths(
            path.to_path_buf(),
            entries,
        ));
    }
    result
}

/// Builds a rooted error for a sandbox operation and retains its cleanup entry.
///
/// When `entry` is absent, the returned error carries no temporary recovery
/// state because no native entry remains available to clean up.
///
/// # Parameters
///
/// - `operation`: Operation reported by the resulting error.
/// - `path`: Diagnostic sandbox path.
/// - `source`: Failure encountered while preparing or using the sandbox.
/// - `entry`: Optional sandbox entry retained for cleanup.
///
/// # Returns
///
/// A rooted error with cleanup recovery when an entry was retained.
fn rooted_sandbox_error(
    operation: LocalFileOperation,
    path: &std::path::Path,
    source: io::Error,
    entry: Option<NativePrivateEntry>,
) -> crate::LocalFileError {
    let error = rooted_io_error(operation, path, source);
    match entry {
        Some(entry) => error.with_temp_recovery(crate::LocalTempRecovery::from_native(path.to_path_buf(), entry)),
        None => error,
    }
}

/// Attempts cleanup of a sandbox and returns ownership of failed recovery.
///
/// On cleanup failure, the recovery object is attached to the error so cleanup
/// can be retried by the caller.
///
/// # Parameters
///
/// - `path`: Diagnostic path of the sandbox.
/// - `entry`: Native sandbox handle and parent ownership used for cleanup.
///
/// # Returns
///
/// `Ok(())` after cleanup, or an error that owns the recovery state.
fn cleanup_rooted_sandbox(path: &std::path::Path, entry: NativePrivateEntry) -> LocalResult<()> {
    let mut owner = crate::LocalTempRecovery::from_native(path.to_path_buf(), entry);
    match owner.cleanup() {
        Ok(()) => Ok(()),
        Err(error) => Err(error.with_temp_recovery(owner)),
    }
}

/// Builds recovery state for a rooted temporary entry and optional sandbox.
///
/// The entry's identity is recorded when available, allowing later cleanup to
/// avoid acting on a replacement at the same path.
///
/// # Parameters
///
/// - `path`: Relative diagnostic path of the created entry.
/// - `parent`: Open parent handle anchoring the entry.
/// - `name`: Native name of the entry beneath `parent`.
/// - `file`: Open handle for the entry.
/// - `directory`: Whether cleanup must treat the entry as a directory.
/// - `sandbox`: Optional recovery entry for its containing sandbox.
///
/// # Returns
///
/// Recovery state owning the entry handles and optional sandbox handle.
fn rooted_temp_recovery(
    path: std::path::PathBuf,
    parent: File,
    name: String,
    file: File,
    directory: bool,
    sandbox: Option<NativePrivateEntry>,
) -> crate::LocalTempRecovery {
    let identity =
        crate::observation::query::query_observation(&file, crate::observation::LocalObservationRequirement::Optional)
            .ok()
            .and_then(|observation| observation.identity().cloned());
    let primary = NativePrivateEntry {
        parent: std::sync::Arc::new(parent),
        name: name.into(),
        file: Some(std::sync::Arc::new(file)),
        identity,
        directory,
    };
    let sandbox_path = path.parent().unwrap_or_else(|| std::path::Path::new("")).to_path_buf();
    let mut entries = vec![(path.clone(), primary)];
    if let Some(sandbox) = sandbox {
        entries.push((sandbox_path, sandbox));
    }
    crate::LocalTempRecovery::from_native_entries_with_paths(path, entries)
}

/// Clones the opened parent handle for use as a recovery anchor.
///
/// The returned handle remains tied to the same directory even if its path is
/// renamed; clone failures are propagated to the caller.
///
/// # Parameters
///
/// - `parent`: Opened directory handle to clone as a recovery anchor.
///
/// # Returns
///
/// A cloned handle to the same directory, or the native clone error.
fn reserve_rooted_private_parent(parent: &File) -> io::Result<File> {
    #[cfg(feature = "test-support")]
    if crate::local::take_test_support("temp-private-parent-clone") {
        return Err(io::Error::new(
            io::ErrorKind::ResourceBusy,
            "injected handle reservation failure",
        ));
    }
    parent.try_clone()
}

/// Reserves a cleanup guard for a newly created sandbox directory.
///
/// The parent handle is returned with any error so recovery can still remove
/// the directory even if cloning its handle fails.
///
/// # Parameters
///
/// - `parent`: Owned handle to the directory containing the sandbox.
/// - `name`: Native name of the newly created sandbox.
/// - `directory`: Opened handle for the sandbox directory.
///
/// # Returns
///
/// A cleanup entry, or the clone error together with the still-owned parent.
fn reserve_rooted_directory_guard(
    parent: File,
    name: &str,
    directory: &File,
) -> Result<NativePrivateEntry, (io::Error, File)> {
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
    Ok(NativePrivateEntry {
        parent: std::sync::Arc::new(parent),
        name: name.into(),
        file: Some(std::sync::Arc::new(directory_handle)),
        identity,
        directory: true,
    })
}
