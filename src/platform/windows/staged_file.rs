// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Windows handle-relative staging file ownership and installation.

use std::ffi::OsString;
use std::fs::File;
use std::io;
use std::io::Write;
use std::os::windows::ffi::OsStringExt;
use std::path::Path;
use std::path::PathBuf;

use windows_sys::Wdk::Storage::FileSystem::FILE_CREATE;
use windows_sys::Wdk::Storage::FileSystem::FILE_NON_DIRECTORY_FILE;
use windows_sys::Win32::Foundation::GENERIC_READ;
use windows_sys::Win32::Foundation::GENERIC_WRITE;
use windows_sys::Win32::Storage::FileSystem::DELETE;
use windows_sys::Win32::Storage::FileSystem::FILE_READ_ATTRIBUTES;
use windows_sys::Win32::Storage::FileSystem::FILE_WRITE_ATTRIBUTES;
use windows_sys::Win32::Storage::FileSystem::SYNCHRONIZE;

use super::NamespaceHandle;
use super::namespace_handle::delete_handle;
use super::namespace_handle::io_error;
use super::namespace_handle::nt_open_at;
use super::namespace_handle::rename_handle;
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

        /// Returns the destination state known after failed installation.
        pub(crate) const fn state(&self) -> StagedInstallState {
            self.state
        }

        /// Returns retained staging ownership when cleanup or retry is safe.
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

/// Owns a private Windows staging entry through its open handle.
#[derive(Debug)]
#[must_use = "dropping the staging guard removes its uncommitted entry"]
pub(crate) struct StagedFile {
    /// Open data and namespace handle.
    file: Option<File>,
    /// Whether cleanup may still delete the staging entry.
    armed: bool,
    /// Relative target retained solely for diagnostic context.
    diagnostic_target: PathBuf,
}

impl StagedFile {
    /// Creates a uniquely named private file in `parent`.
    ///
    /// # Errors
    ///
    /// Returns an open-writer error when randomness fails or sixteen exclusive
    /// creation attempts collide or fail.
    pub(super) fn create(parent: File, diagnostic_target: &Path) -> LocalResult<Self> {
        let mut last_collision = None;
        for _ in 0..16 {
            let name = random_staging_name(diagnostic_target)?;
            match nt_open_at(
                &parent,
                &name,
                GENERIC_READ | GENERIC_WRITE | DELETE | FILE_READ_ATTRIBUTES | FILE_WRITE_ATTRIBUTES | SYNCHRONIZE,
                FILE_CREATE,
                FILE_NON_DIRECTORY_FILE,
            ) {
                Ok(file) => {
                    return Ok(Self {
                        file: Some(file),
                        armed: true,
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

    /// Returns the retained staging handle.
    ///
    /// # Panics
    ///
    /// Panics after the handle has been closed.
    #[must_use]
    pub(crate) fn file(&self) -> &File {
        self.file.as_ref().expect("staging handle has already been closed")
    }

    /// Returns the retained staging handle mutably.
    ///
    /// # Panics
    ///
    /// Panics after the handle has been closed.
    #[must_use]
    pub(crate) fn file_mut(&mut self) -> &mut File {
        self.file.as_mut().expect("staging handle has already been closed")
    }

    /// Flushes userspace buffers and synchronizes staging contents.
    ///
    /// # Errors
    ///
    /// Returns a commit error when flushing or handle synchronization fails.
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
    /// # Errors
    ///
    /// Returns native unchanged or indeterminate facts. The native rename is
    /// handle-relative and honors `overwrite` atomically.
    pub(crate) fn install(
        mut self,
        namespace: &NamespaceHandle,
        target: &RelativePath,
        overwrite: bool,
    ) -> Result<(), StagedInstallError> {
        let result = rename_handle(self.file(), &namespace.handle, target, overwrite);
        match result {
            Ok(()) => {
                self.armed = false;
                Ok(())
            }
            Err(native) => {
                let state = if native.raw_os_error() == Some(1177) {
                    StagedInstallState::Indeterminate
                } else {
                    StagedInstallState::Unchanged
                };
                let error = io_error(
                    LocalFileOperation::Commit,
                    &self.diagnostic_target,
                    Some(target.as_path()),
                    native,
                );
                let staged_file = if state == StagedInstallState::Indeterminate {
                    self.armed = false;
                    None
                } else {
                    Some(self)
                };
                Err(StagedInstallError::new(error, state, staged_file))
            }
        }
    }

    /// Deletes the uncommitted staging entry through its open handle.
    ///
    /// Cleanup remains armed on failure, allowing a later retry.
    ///
    /// # Errors
    ///
    /// Returns a cleanup error when permissions or native deletion fails.
    pub(crate) fn cleanup(&mut self) -> LocalResult<()> {
        if !self.armed {
            return Ok(());
        }
        let file = self.file();
        let mut permissions = file
            .metadata()
            .map_err(|error| io_error(LocalFileOperation::Cleanup, &self.diagnostic_target, None, error))?
            .permissions();
        if permissions.readonly() {
            #[allow(clippy::permissions_set_readonly_false)]
            permissions.set_readonly(false);
            file.set_permissions(permissions)
                .map_err(|error| io_error(LocalFileOperation::Cleanup, &self.diagnostic_target, None, error))?;
        }
        delete_handle(file)
            .map_err(|error| io_error(LocalFileOperation::Cleanup, &self.diagnostic_target, None, error))?;
        self.armed = false;
        let _ = self.file.take();
        Ok(())
    }
}

impl Write for StagedFile {
    /// Writes bytes to the retained staging handle.
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        self.file_mut().write(buffer)
    }

    /// Flushes userspace buffers for the retained staging handle.
    fn flush(&mut self) -> io::Result<()> {
        self.file_mut().flush()
    }
}

impl Drop for StagedFile {
    /// Best-effort removes an armed staging entry.
    fn drop(&mut self) {
        let _ = self.cleanup();
    }
}

/// Generates one cryptographically random native staging component.
///
/// # Errors
///
/// Returns an open-writer error when the operating-system random source fails.
fn random_staging_name(diagnostic_target: &Path) -> LocalResult<OsString> {
    let mut random = [0_u8; 16];
    getrandom::fill(&mut random).map_err(|error| {
        io_error(
            LocalFileOperation::OpenWriter,
            diagnostic_target,
            None,
            io::Error::other(error),
        )
    })?;
    const HEX: &[u16; 16] = &[
        b'0' as u16,
        b'1' as u16,
        b'2' as u16,
        b'3' as u16,
        b'4' as u16,
        b'5' as u16,
        b'6' as u16,
        b'7' as u16,
        b'8' as u16,
        b'9' as u16,
        b'a' as u16,
        b'b' as u16,
        b'c' as u16,
        b'd' as u16,
        b'e' as u16,
        b'f' as u16,
    ];
    let mut units: Vec<u16> = ".qubit-stage-".encode_utf16().collect();
    units.reserve(random.len() * 2);
    for byte in random {
        units.push(HEX[usize::from(byte >> 4)]);
        units.push(HEX[usize::from(byte & 0x0f)]);
    }
    Ok(OsString::from_wide(&units))
}
