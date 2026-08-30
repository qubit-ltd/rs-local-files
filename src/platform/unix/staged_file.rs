// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Descriptor-relative Unix staging file ownership and installation.
#![allow(dead_code)]

use std::ffi::CStr;
use std::ffi::CString;
use std::fs::File;
use std::io;
use std::io::Write;
use std::os::fd::AsRawFd;
use std::path::Path;
use std::path::PathBuf;

use log::warn;

use super::NamespaceHandle;
use super::namespace_handle::io_error;
use super::namespace_handle::open_file_at;
use super::namespace_handle::open_parent;
use crate::LocalFileOperation;
use crate::LocalResult;
use crate::RelativePath;

mod install_error {
    use super::StagedFile;
    use crate::LocalFileError;

    /// Native namespace state known after a staging installation failure.
    #[derive(Clone, Copy, Debug, Eq, PartialEq)]
    #[must_use]
    pub(crate) enum StagedInstallState {
        /// The destination was not modified.
        Unchanged,
        /// The destination contains the staged file.
        Published,
        /// The destination outcome cannot be determined safely.
        Indeterminate,
    }

    /// A staging installation error with the most precise native outcome.
    #[derive(Debug)]
    pub(crate) struct StagedInstallError {
        /// Structured native failure.
        error: Box<LocalFileError>,
        /// Destination state known after the failure.
        state: StagedInstallState,
        /// Retained staging ownership when cleanup remains safe.
        staged_file: Option<StagedFile>,
    }

    impl StagedInstallError {
        /// Creates an installation failure and retains safe cleanup ownership.
        pub(super) fn new(error: LocalFileError, state: StagedInstallState, staged_file: Option<StagedFile>) -> Self {
            Self {
                error: Box::new(error),
                state,
                staged_file,
            }
        }

        /// Returns the structured native failure.
        #[must_use]
        pub(crate) fn error(&self) -> &LocalFileError {
            self.error.as_ref()
        }

        /// Returns the destination state known after the failed installation.
        pub(crate) const fn state(&self) -> StagedInstallState {
            self.state
        }

        /// Returns retained staging ownership when cleanup or retry is safe.
        ///
        /// `None` means the native outcome is indeterminate and callers must
        /// not mutate the staging entry based on this error alone.
        #[must_use]
        pub(crate) const fn staged_file(&self) -> Option<&StagedFile> {
            self.staged_file.as_ref()
        }

        /// Consumes this failure into all retained native facts.
        pub(crate) fn into_parts(self) -> (LocalFileError, StagedInstallState, Option<StagedFile>) {
            (*self.error, self.state, self.staged_file)
        }
    }
}

pub(crate) use install_error::StagedInstallError;
pub(crate) use install_error::StagedInstallState;

/// Owns a private Unix staging entry until installation or cleanup succeeds.
#[derive(Debug)]
#[must_use = "dropping the staging guard removes its uncommitted entry"]
pub(crate) struct StagedFile {
    /// Parent descriptor authorizing staging cleanup and installation.
    parent: File,
    /// Staging name while descriptor-relative cleanup remains armed.
    name: Option<CString>,
    /// Open data descriptor until synchronization or installation closes it.
    file: Option<File>,
    /// Relative target path retained solely for diagnostic context.
    diagnostic_target: PathBuf,
}

impl StagedFile {
    /// Creates a uniquely named private file in `parent`.
    ///
    /// # Errors
    ///
    /// Returns an open-writer error when randomness is unavailable or sixteen
    /// exclusive creation attempts all collide or fail.
    pub(super) fn create(parent: File, diagnostic_target: &Path) -> LocalResult<Self> {
        let mut last_collision = None;
        for _ in 0..16 {
            let name = random_staging_name(diagnostic_target)?;
            match open_file_at(
                &parent,
                &name,
                libc::O_RDWR | libc::O_CREAT | libc::O_EXCL | libc::O_NOFOLLOW | libc::O_CLOEXEC,
                0o600,
            ) {
                Ok(file) => {
                    return Ok(Self {
                        parent,
                        name: Some(name),
                        file: Some(file),
                        diagnostic_target: diagnostic_target.to_path_buf(),
                    });
                }
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                    last_collision = Some(error);
                }
                Err(error) => {
                    return Err(io_error(LocalFileOperation::OpenWriter, diagnostic_target, None, error));
                }
            }
        }
        Err(io_error(
            LocalFileOperation::OpenWriter,
            diagnostic_target,
            None,
            last_collision.unwrap_or_else(|| {
                io::Error::new(io::ErrorKind::AlreadyExists, "staging name attempts were exhausted")
            }),
        ))
    }

    /// Returns the retained staging data descriptor.
    ///
    /// # Panics
    ///
    /// Panics after the data descriptor has been closed.
    #[must_use]
    pub(crate) fn file(&self) -> &File {
        self.file
            .as_ref()
            .expect("staging data descriptor has already been closed")
    }

    /// Returns the retained staging data descriptor mutably.
    ///
    /// # Panics
    ///
    /// Panics after the data descriptor has been closed.
    #[must_use]
    pub(crate) fn file_mut(&mut self) -> &mut File {
        self.file
            .as_mut()
            .expect("staging data descriptor has already been closed")
    }

    /// Flushes userspace buffers and synchronizes staging contents.
    ///
    /// # Errors
    ///
    /// Returns a commit error when flushing or `fsync` fails. The staging
    /// entry remains owned for retry or cleanup.
    pub(crate) fn sync_contents(&mut self) -> LocalResult<()> {
        self.file_mut()
            .flush()
            .map_err(|error| io_error(LocalFileOperation::Commit, &self.diagnostic_target, None, error))?;
        self.file()
            .sync_all()
            .map_err(|error| io_error(LocalFileOperation::Commit, &self.diagnostic_target, None, error))
    }

    /// Installs this staging entry at `target` within `namespace`.
    ///
    /// # Parameters
    ///
    /// - `namespace`: Authority used to open the target parent.
    /// - `target`: Validated final destination.
    /// - `overwrite`: Whether an existing destination may be atomically
    ///   replaced.
    ///
    /// # Errors
    ///
    /// Returns [`StagedInstallError`] with unchanged, published, or
    /// indeterminate native facts. Safe staging ownership is retained in the
    /// error; indeterminate outcomes deliberately disarm namespace mutation.
    pub(crate) fn install(
        mut self,
        namespace: &NamespaceHandle,
        target: &RelativePath,
        overwrite: bool,
    ) -> Result<(), StagedInstallError> {
        drop(self.file.take());
        let (target_parent, target_name) = match open_parent(namespace, target, LocalFileOperation::Commit) {
            Ok(parent) => parent,
            Err(error) => {
                return Err(StagedInstallError::new(
                    error,
                    StagedInstallState::Unchanged,
                    Some(self),
                ));
            }
        };
        let name = self
            .name
            .as_ref()
            .expect("armed staging entry should retain its native name");
        let result = if overwrite {
            // SAFETY: both parent descriptors and names remain live for this
            // non-retaining descriptor-relative rename.
            let result = unsafe {
                libc::renameat(
                    self.parent.as_raw_fd(),
                    name.as_ptr(),
                    target_parent.as_raw_fd(),
                    target_name.as_ptr(),
                )
            };
            if result == 0 {
                Ok(())
            } else {
                Err((io::Error::last_os_error(), StagedInstallState::Unchanged))
            }
        } else {
            install_no_replace(&self.parent, name, &target_parent, &target_name)
        };
        match result {
            Ok(()) => {
                let _ = self.name.take();
                Ok(())
            }
            Err((native, state)) => {
                let error = io_error(
                    LocalFileOperation::Commit,
                    &self.diagnostic_target,
                    Some(target.as_path()),
                    native,
                );
                let staged_file = if state == StagedInstallState::Indeterminate {
                    let _ = self.name.take();
                    None
                } else {
                    Some(self)
                };
                Err(StagedInstallError::new(error, state, staged_file))
            }
        }
    }

    /// Closes and removes the uncommitted staging entry.
    ///
    /// Cleanup remains armed when removal fails, allowing a later retry.
    ///
    /// # Errors
    ///
    /// Returns a cleanup error when descriptor-relative `unlinkat` fails.
    pub(crate) fn cleanup(&mut self) -> LocalResult<()> {
        drop(self.file.take());
        let Some(name) = self.name.as_ref() else {
            return Ok(());
        };
        // SAFETY: the parent descriptor and staging name remain live for this
        // non-retaining descriptor-relative unlink.
        let result = unsafe { libc::unlinkat(self.parent.as_raw_fd(), name.as_ptr(), 0) };
        if result == 0 {
            let _ = self.name.take();
            Ok(())
        } else {
            Err(io_error(
                LocalFileOperation::Cleanup,
                &self.diagnostic_target,
                None,
                io::Error::last_os_error(),
            ))
        }
    }
}

impl Write for StagedFile {
    /// Writes bytes to the retained staging descriptor.
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        self.file_mut().write(buffer)
    }

    /// Flushes userspace buffers for the retained staging descriptor.
    fn flush(&mut self) -> io::Result<()> {
        self.file_mut().flush()
    }
}

impl Drop for StagedFile {
    /// Best-effort removes an armed staging entry.
    fn drop(&mut self) {
        if let Err(error) = self.cleanup()
            && self.name.is_some()
        {
            warn!("failed to remove uncommitted descriptor-relative staging file: {error}");
        }
    }
}

/// Generates one cryptographically random staging component.
///
/// # Errors
///
/// Returns an open-writer error when the operating system random source fails.
fn random_staging_name(diagnostic_target: &Path) -> LocalResult<CString> {
    let mut random = [0_u8; 16];
    getrandom::fill(&mut random).map_err(|error| {
        io_error(
            LocalFileOperation::OpenWriter,
            diagnostic_target,
            None,
            io::Error::other(error),
        )
    })?;
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut name = Vec::with_capacity(14 + random.len() * 2);
    name.extend_from_slice(b".qubit-stage-");
    for byte in random {
        name.push(HEX[usize::from(byte >> 4)]);
        name.push(HEX[usize::from(byte & 0x0f)]);
    }
    Ok(CString::new(name).expect("generated staging name contains no NUL"))
}

/// Installs a staging file without replacing an existing target on Linux.
#[cfg(any(target_os = "linux", target_os = "android"))]
fn install_no_replace(
    staging_parent: &File,
    staging: &CStr,
    target_parent: &File,
    target: &CStr,
) -> Result<(), (io::Error, StagedInstallState)> {
    // SAFETY: both directory descriptors and names remain live for this
    // non-retaining atomic no-replace rename.
    let result = unsafe {
        libc::renameat2(
            staging_parent.as_raw_fd(),
            staging.as_ptr(),
            target_parent.as_raw_fd(),
            target.as_ptr(),
            libc::RENAME_NOREPLACE as _,
        )
    };
    if result == 0 {
        return Ok(());
    }
    let error = io::Error::last_os_error();
    if !matches!(
        error.raw_os_error(),
        Some(code)
            if code == libc::ENOSYS
                || code == libc::EINVAL
                || code == libc::EOPNOTSUPP
    ) {
        return Err((error, StagedInstallState::Unchanged));
    }
    link_then_unlink(staging_parent, staging, target_parent, target)
}

/// Installs a staging file without replacement on Apple systems.
#[cfg(any(target_os = "macos", target_os = "ios"))]
fn install_no_replace(
    staging_parent: &File,
    staging: &CStr,
    target_parent: &File,
    target: &CStr,
) -> Result<(), (io::Error, StagedInstallState)> {
    // SAFETY: both directory descriptors and names remain live for this
    // non-retaining exclusive rename.
    let result = unsafe {
        libc::renameatx_np(
            staging_parent.as_raw_fd(),
            staging.as_ptr(),
            target_parent.as_raw_fd(),
            target.as_ptr(),
            libc::RENAME_EXCL,
        )
    };
    if result == 0 {
        Ok(())
    } else {
        Err((io::Error::last_os_error(), StagedInstallState::Unchanged))
    }
}

/// Installs a staging file without replacement through hard-link publication.
#[cfg(not(any(target_os = "linux", target_os = "android", target_os = "macos", target_os = "ios",)))]
fn install_no_replace(
    staging_parent: &File,
    staging: &CStr,
    target_parent: &File,
    target: &CStr,
) -> Result<(), (io::Error, StagedInstallState)> {
    link_then_unlink(staging_parent, staging, target_parent, target)
}

/// Publishes a hard link and then removes the private staging name.
fn link_then_unlink(
    staging_parent: &File,
    staging: &CStr,
    target_parent: &File,
    target: &CStr,
) -> Result<(), (io::Error, StagedInstallState)> {
    // SAFETY: both directory descriptors and names remain live for this
    // non-retaining hard-link operation.
    let result = unsafe {
        libc::linkat(
            staging_parent.as_raw_fd(),
            staging.as_ptr(),
            target_parent.as_raw_fd(),
            target.as_ptr(),
            0,
        )
    };
    if result == -1 {
        return Err((io::Error::last_os_error(), StagedInstallState::Unchanged));
    }
    // SAFETY: the staging parent descriptor and name remain live for this
    // non-retaining unlink operation.
    if unsafe { libc::unlinkat(staging_parent.as_raw_fd(), staging.as_ptr(), 0) } == -1 {
        return Err((io::Error::last_os_error(), StagedInstallState::Published));
    }
    Ok(())
}
