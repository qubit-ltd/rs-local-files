// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

// Rooted temp operations.
// qubit-style: allow source-test-pair

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
use super::resolve_rooted_path;
use super::rooted_io_error;
use super::rooted_path;
use super::rooted_temp_parent;
use super::temp_candidate;
use super::validate_rooted_temp_parent;
use super::validate_temp_affixes;

impl RootedLocalFileSystem {
    /// Creates a cleanup-owned temporary file below this opened root.
    ///
    /// The optional parent must be a validated rooted descendant. The returned
    /// resource retains this exact opened root authority, so later cleanup is
    /// unaffected by rename or replacement of the diagnostic root path.
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
            resolve_rooted_path(
                &self.root,
                &requested_parent,
                symlink_policy,
                true,
                LocalFileOperation::CreateTempFile,
            )?
            .as_path()
            .to_path_buf()
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
        let mut attempts = 0_usize;
        loop {
            attempts = attempts.saturating_add(1);
            let resource_name =
                match crate::local::try_random_file_name("qubit-local-files-", options.prefix(), options.suffix()) {
                    Ok(name) => name,
                    Err(error) => {
                        return Err(rooted_io_error(LocalFileOperation::CreateTempFile, &parent, error));
                    }
                };
            let sandbox = temp_candidate(&parent, Some("sandbox-"), None, LocalFileOperation::CreateTempFile)?;
            let sandbox_relative = rooted_path(&sandbox, LocalFileOperation::CreateTempFile)?;
            match self.root.create_dir(&sandbox_relative) {
                Ok(()) => {}
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                    if attempt_budget_exhausted(attempts, options.max_attempts()) {
                        break;
                    }
                    continue;
                }
                Err(error) => {
                    return Err(rooted_io_error(
                        LocalFileOperation::CreateTempFile,
                        sandbox_relative.as_path(),
                        error,
                    ));
                }
            }
            let candidate = sandbox.join(resource_name);
            let relative = rooted_path(&candidate, LocalFileOperation::CreateTempFile)?;
            #[cfg(feature = "test-support")]
            let opened = if crate::local::test_support_enabled("rooted-temp-file-collision") {
                Err(io::Error::from(io::ErrorKind::AlreadyExists))
            } else if crate::local::test_support_enabled("rooted-temp-file-open") {
                Err(io::Error::from(io::ErrorKind::PermissionDenied))
            } else {
                self.root.open_writer(
                    &relative,
                    &crate::write::OpenOptions::new(crate::write::Mode::CreateNew),
                )
            };
            #[cfg(not(feature = "test-support"))]
            let opened = self.root.open_writer(
                &relative,
                &crate::write::OpenOptions::new(crate::write::Mode::CreateNew),
            );
            match opened {
                Ok(file) => {
                    let cleanup_sandbox = sandbox.clone();
                    let result =
                        LocalTempFile::rooted(Arc::clone(&self.root), candidate, sandbox, file, symlink_policy);
                    return match result {
                        Ok(resource) => Ok(resource),
                        Err(error) => {
                            let _ = self
                                .root
                                .remove_tree(&rooted_path(&cleanup_sandbox, LocalFileOperation::CreateTempFile)?);
                            Err(rooted_io_error(
                                LocalFileOperation::CreateTempFile,
                                relative.as_path(),
                                error,
                            ))
                        }
                    };
                }
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                    let _ = self.root.remove_tree(&sandbox_relative);
                    if attempt_budget_exhausted(attempts, options.max_attempts()) {
                        break;
                    }
                    continue;
                }
                Err(error) => {
                    let _ = self.root.remove_tree(&sandbox_relative);
                    return Err(rooted_io_error(
                        LocalFileOperation::CreateTempFile,
                        relative.as_path(),
                        error,
                    ));
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
            resolve_rooted_path(
                &self.root,
                &requested_parent,
                symlink_policy,
                true,
                LocalFileOperation::CreateTempDirectory,
            )?
            .as_path()
            .to_path_buf()
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
        let mut attempts = 0_usize;
        loop {
            attempts = attempts.saturating_add(1);
            let resource_name =
                match crate::local::try_random_file_name("qubit-local-files-", options.prefix(), options.suffix()) {
                    Ok(name) => name,
                    Err(error) => {
                        return Err(rooted_io_error(LocalFileOperation::CreateTempDirectory, &parent, error));
                    }
                };
            let sandbox = temp_candidate(&parent, Some("sandbox-"), None, LocalFileOperation::CreateTempDirectory)?;
            let sandbox_relative = rooted_path(&sandbox, LocalFileOperation::CreateTempDirectory)?;
            match self.root.create_dir(&sandbox_relative) {
                Ok(()) => {}
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                    if attempt_budget_exhausted(attempts, options.max_attempts()) {
                        break;
                    }
                    continue;
                }
                Err(error) => {
                    return Err(rooted_io_error(
                        LocalFileOperation::CreateTempDirectory,
                        sandbox_relative.as_path(),
                        error,
                    ));
                }
            }
            let candidate = sandbox.join(resource_name);
            let relative = rooted_path(&candidate, LocalFileOperation::CreateTempDirectory)?;
            #[cfg(feature = "test-support")]
            let created = if crate::local::test_support_enabled("rooted-temp-directory-collision") {
                Err(io::Error::from(io::ErrorKind::AlreadyExists))
            } else if crate::local::test_support_enabled("rooted-temp-directory-create") {
                Err(io::Error::from(io::ErrorKind::PermissionDenied))
            } else {
                self.root.create_dir(&relative)
            };
            #[cfg(not(feature = "test-support"))]
            let created = self.root.create_dir(&relative);
            match created {
                Ok(()) => {
                    let cleanup_sandbox = sandbox.clone();
                    let result = LocalTempDirectory::rooted(Arc::clone(&self.root), candidate, sandbox, symlink_policy);
                    return match result {
                        Ok(resource) => Ok(resource),
                        Err(error) => {
                            let _ = self
                                .root
                                .remove_tree(&rooted_path(&cleanup_sandbox, LocalFileOperation::CreateTempDirectory)?);
                            Err(rooted_io_error(
                                LocalFileOperation::CreateTempDirectory,
                                relative.as_path(),
                                error,
                            ))
                        }
                    };
                }
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                    let _ = self.root.remove_tree(&sandbox_relative);
                    if attempt_budget_exhausted(attempts, options.max_attempts()) {
                        break;
                    }
                    continue;
                }
                Err(error) => {
                    let _ = self.root.remove_tree(&sandbox_relative);
                    return Err(rooted_io_error(
                        LocalFileOperation::CreateTempDirectory,
                        relative.as_path(),
                        error,
                    ));
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
}

/// Reports whether an explicitly configured collision budget is exhausted.
#[must_use]
fn attempt_budget_exhausted(attempts: usize, max_attempts: Option<usize>) -> bool {
    max_attempts.is_some_and(|max_attempts| attempts >= max_attempts)
}
