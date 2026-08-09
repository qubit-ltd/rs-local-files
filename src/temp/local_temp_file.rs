// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Cleanup-owned temporary files with host or rooted authority.

use std::fs::File;
use std::io::Error;
use std::io::ErrorKind;
use std::io::IoSlice;
use std::io::Result;
use std::io::Seek;
use std::io::SeekFrom;
use std::io::Write;
use std::path::Path;
use std::path::PathBuf;
use std::sync::Arc;

use log::warn;

use super::internal::LocalTempResourceBackend;
use super::internal::LocalTempResourceState;
use super::internal::RootedTempResourceBackend;
use super::internal::TempEntryIdentity;
use super::internal::prepare_host_parent;
use super::internal::prepare_rooted_parent;
use crate::LocalFileError;
use crate::LocalFileOperation;
use crate::LocalPersistError;
use crate::LocalPersistFailureState;
use crate::LocalPersistMethod;
use crate::LocalPersistOptions;
use crate::LocalPersistOutcome;
use crate::LocalPersistStage;
use crate::LocalRelativePath;
use crate::LocalResult;
use crate::LocalSymlinkPolicy;

/// A temporary file whose cleanup remains bound to its creating authority.
///
/// Cleanup rejects ordinary path replacement by checking the identity captured
/// at creation. The check and deletion are not atomic, so callers must exclude
/// untrusted concurrent mutation of the containing directory; identity reuse
/// and a check/delete race cannot be ruled out by this path-based API.
/// Persistence resolves intermediate symbolic links using the policy captured
/// by the creating [`crate::LocalFileSystem`], while replacing a final link
/// entry itself.
///
/// The file is created inside a private generated sandbox. Cleanup removes the
/// file and then the empty sandbox; [`Self::keep`] transfers both to the
/// caller.
#[must_use = "dropping the temporary-file guard removes its file"]
#[derive(Debug)]
pub struct LocalTempFile {
    /// Stable authority-local path retained after close and cleanup.
    path: PathBuf,
    /// Authority and resource ownership state.
    backend: LocalTempResourceBackend,
    /// The open native file, until explicitly closed.
    file: Option<File>,
    /// Native identity captured when the temporary file was created.
    host_identity: Option<TempEntryIdentity>,
    /// Rooted identity captured through the opened root authority.
    rooted_identity: Option<crate::rooted::Metadata>,
    /// Namespace certainty governing cleanup and drop behavior.
    state: LocalTempResourceState,
    /// Symbolic-link policy retained for persistence targets.
    symlink_policy: LocalSymlinkPolicy,
}

impl LocalTempFile {
    /// Builds a host temporary file from its already-bound path and handle.
    #[inline]
    pub(crate) fn host(
        path: PathBuf,
        sandbox_path: PathBuf,
        file: File,
        symlink_policy: LocalSymlinkPolicy,
    ) -> Result<Self> {
        Ok(Self {
            path,
            backend: LocalTempResourceBackend::Host(
                super::internal::HostTempResourceBackend { sandbox_path },
            ),
            host_identity: Some(TempEntryIdentity::from_file(&file)?),
            rooted_identity: None,
            file: Some(file),
            state: LocalTempResourceState::Owned,
            symlink_policy,
        })
    }

    /// Builds a rooted temporary file from the retained root authority.
    #[inline]
    pub(crate) fn rooted(
        root: Arc<crate::rooted::Root>,
        path: PathBuf,
        sandbox_path: PathBuf,
        file: File,
        symlink_policy: LocalSymlinkPolicy,
    ) -> Result<Self> {
        Ok(Self {
            path: path.clone(),
            backend: LocalTempResourceBackend::Rooted(
                RootedTempResourceBackend {
                    root,
                    relative_path: path,
                    sandbox_path,
                },
            ),
            host_identity: None,
            rooted_identity: Some(crate::rooted::Metadata::from_open_file(
                &file,
            )?),
            file: Some(file),
            state: LocalTempResourceState::Owned,
            symlink_policy,
        })
    }

    /// Returns the authority-local generated path.
    #[must_use]
    #[inline(always)]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Closes the file I/O handle while retaining cleanup and persistence
    /// responsibility.
    #[inline(always)]
    pub fn close(&mut self) {
        drop(self.file.take());
    }

    /// Removes the entry through the authority retained at creation time.
    pub fn cleanup(&mut self) -> LocalResult<()> {
        self.close();
        self.ensure_cleanup_safe().map_err(|error| {
            LocalFileError::from_io(
                LocalFileOperation::Cleanup,
                Some(self.path.clone()),
                None,
                error,
            )
        })?;
        if self.state == LocalTempResourceState::Owned {
            self.remove_resource().map_err(|error| {
                LocalFileError::from_io(
                    LocalFileOperation::Cleanup,
                    Some(self.path.clone()),
                    None,
                    error,
                )
            })?;
            self.state = LocalTempResourceState::SandboxPending;
        }
        if self.state == LocalTempResourceState::SandboxPending {
            self.release_sandbox().map_err(|error| {
                LocalFileError::from_io(
                    LocalFileOperation::Cleanup,
                    Some(self.cleanup_path()),
                    None,
                    error,
                )
            })?;
            self.state = LocalTempResourceState::Released;
        }
        Ok(())
    }

    /// Disables automatic cleanup and returns the authority-local path.
    #[must_use = "keeping the temporary file disables automatic cleanup; retain the returned path"]
    #[inline]
    pub fn keep(mut self) -> PathBuf {
        self.close();
        self.state = LocalTempResourceState::Released;
        self.path.clone()
    }

    /// Persists the file within its creating authority without replacement.
    #[inline(always)]
    pub fn persist(
        self,
        target: impl AsRef<Path>,
    ) -> std::result::Result<LocalPersistOutcome, LocalPersistError<Self>> {
        self.persist_with(target, LocalPersistOptions::new())
    }

    /// Persists the file with explicit replacement policy within its creating
    /// authority.
    #[inline(always)]
    pub fn persist_with(
        self,
        target: impl AsRef<Path>,
        options: LocalPersistOptions,
    ) -> std::result::Result<LocalPersistOutcome, LocalPersistError<Self>> {
        self.persist_with_path(target.as_ref(), options)
    }

    /// Returns the mutable open file handle, or an error after [`Self::close`].
    #[inline(always)]
    pub fn as_file_mut(&mut self) -> Result<&mut File> {
        self.file.as_mut().ok_or_else(closed_file_error)
    }

    /// Persists the file to a resolved public-API target path.
    fn persist_with_path(
        mut self,
        target: &Path,
        options: LocalPersistOptions,
    ) -> std::result::Result<LocalPersistOutcome, LocalPersistError<Self>> {
        self.close();
        if let Err(error) = self.ensure_identity_matches() {
            return Err(LocalPersistError::new(
                error,
                self,
                target.to_path_buf(),
                None,
                LocalPersistStage::InstallDestination,
            ));
        }
        if self.state == LocalTempResourceState::Indeterminate {
            return Err(LocalPersistError::new(
                Error::other("temporary file namespace state is indeterminate"),
                self,
                target.to_path_buf(),
                None,
                LocalPersistStage::InstallDestination,
            ));
        }
        let requested_target = target.to_path_buf();
        if matches!(&self.backend, LocalTempResourceBackend::Host(_)) {
            let logical_target = match std::path::absolute(&requested_target) {
                Ok(target) => target,
                Err(error) => {
                    return Err(LocalPersistError::new(
                        error,
                        self,
                        requested_target,
                        None,
                        LocalPersistStage::ResolveTarget,
                    ));
                }
            };
            let target = match crate::local::resolve_host_path(
                &logical_target,
                self.symlink_policy,
                false,
            ) {
                Ok(target) => target,
                Err(error) => {
                    return Err(LocalPersistError::new(
                        error.into_io_error(),
                        self,
                        requested_target,
                        None,
                        LocalPersistStage::ResolveTarget,
                    ));
                }
            };
            if let Err(error) =
                prepare_host_parent(&target, options.creates_parent())
            {
                return Err(LocalPersistError::new(
                    error,
                    self,
                    requested_target,
                    Some(target),
                    LocalPersistStage::PrepareParent,
                ));
            }
            let result = if options.overwrites() {
                crate::local::replace_file(&self.path, &target)
            } else {
                crate::local::move_file_without_replacing(&self.path, &target)
            };
            if let Err(error) = result {
                self.record_native_persist_failure(&error);
                return Err(LocalPersistError::new(
                    error,
                    self,
                    requested_target,
                    Some(target),
                    LocalPersistStage::InstallDestination,
                ));
            }
            let cleanup_error = self.release_sandbox().err().map(|error| {
                LocalFileError::from_io(
                    LocalFileOperation::Cleanup,
                    Some(self.cleanup_path()),
                    None,
                    error,
                )
            });
            self.state = LocalTempResourceState::Released;
            return Ok(LocalPersistOutcome::new(
                logical_target,
                LocalPersistMethod::AtomicRename,
                true,
                false,
                cleanup_error,
            ));
        }
        let target = match LocalRelativePath::new(&requested_target) {
            Ok(path) => path.as_path().to_path_buf(),
            Err(error) => {
                return Err(LocalPersistError::new(
                    error,
                    self,
                    requested_target,
                    None,
                    LocalPersistStage::ResolveTarget,
                ));
            }
        };
        let LocalTempResourceBackend::Rooted(rooted) = &self.backend else {
            unreachable!()
        };
        let source = LocalRelativePath::new(&rooted.relative_path)
            .expect("rooted temporary path was validated at creation");
        let resolved =
            match crate::rooted_local_file_system::resolve_rooted_path(
                &rooted.root,
                &target,
                self.symlink_policy,
                false,
                LocalFileOperation::PersistTemp,
            ) {
                Ok(resolved) => resolved,
                Err(error) => {
                    return Err(LocalPersistError::new(
                        error.into_io_error(),
                        self,
                        requested_target,
                        None,
                        LocalPersistStage::ResolveTarget,
                    ));
                }
            };
        let destination = resolved;
        if let Err(error) = prepare_rooted_parent(
            &rooted.root,
            &destination,
            options.creates_parent(),
        ) {
            return Err(LocalPersistError::new(
                error,
                self,
                requested_target,
                Some(destination.as_path().to_path_buf()),
                LocalPersistStage::PrepareParent,
            ));
        }
        let result = if options.overwrites() {
            rooted.root.rename(&source, &destination)
        } else {
            rooted.root.rename_without_replacing(&source, &destination)
        };
        let target = destination.as_path().to_path_buf();
        if let Err(error) = result {
            self.record_native_persist_failure(&error);
            return Err(LocalPersistError::new(
                error,
                self,
                requested_target,
                Some(target),
                LocalPersistStage::InstallDestination,
            ));
        }
        let cleanup_error = self.release_sandbox().err().map(|error| {
            LocalFileError::from_io(
                LocalFileOperation::Cleanup,
                Some(self.cleanup_path()),
                None,
                error,
            )
        });
        self.state = LocalTempResourceState::Released;
        Ok(LocalPersistOutcome::new(
            target,
            LocalPersistMethod::AtomicRename,
            true,
            false,
            cleanup_error,
        ))
    }

    /// Removes the resource using the retained backend rather than a diagnostic
    /// path.
    #[inline]
    fn remove_resource(&mut self) -> Result<()> {
        self.ensure_identity_matches()?;
        match &self.backend {
            LocalTempResourceBackend::Host(_) => {
                std::fs::remove_file(&self.path)?;
                Ok(())
            }
            LocalTempResourceBackend::Rooted(rooted) => {
                let path = LocalRelativePath::new(&rooted.relative_path)
                    .expect("rooted temporary path was validated at creation");
                rooted.root.remove_file(&path)?;
                Ok(())
            }
        }
    }

    /// Removes the now-empty private sandbox.
    fn release_sandbox(&self) -> Result<()> {
        #[cfg(feature = "internal-test-support")]
        if crate::local::take_test_support("temp-file-sandbox-remove") {
            return Err(crate::local::test_fault_error());
        }
        match &self.backend {
            LocalTempResourceBackend::Host(host) => {
                std::fs::remove_dir(&host.sandbox_path)
            }
            LocalTempResourceBackend::Rooted(rooted) => {
                let sandbox = LocalRelativePath::new(&rooted.sandbox_path)
                    .expect("rooted temporary sandbox path was validated at creation");
                rooted.root.remove_empty_dir(&sandbox)
            }
        }
    }

    /// Returns the authority-local sandbox path used for cleanup diagnostics.
    fn cleanup_path(&self) -> PathBuf {
        match &self.backend {
            LocalTempResourceBackend::Host(host) => host.sandbox_path.clone(),
            LocalTempResourceBackend::Rooted(rooted) => {
                rooted.sandbox_path.clone()
            }
        }
    }

    /// Rejects namespace cleanup after an indeterminate native publication
    /// attempt.
    #[inline]
    fn ensure_cleanup_safe(&self) -> Result<()> {
        if self.state == LocalTempResourceState::Indeterminate {
            return Err(Error::other(
                "temporary file namespace state is indeterminate; cleanup is unsafe",
            ));
        }
        Ok(())
    }

    /// Rejects operations when the authority path no longer names this file.
    fn ensure_identity_matches(&mut self) -> Result<()> {
        let matches = match &self.backend {
            LocalTempResourceBackend::Host(_) => self
                .host_identity
                .as_ref()
                .expect("host temporary file must retain host identity")
                .matches_path(&self.path),
            LocalTempResourceBackend::Rooted(rooted) => rooted
                .root
                .symlink_metadata(
                    &LocalRelativePath::new(&rooted.relative_path).expect(
                        "rooted temporary path was validated at creation",
                    ),
                )
                .map(|metadata| {
                    metadata.is_same_file(self.rooted_identity.as_ref().expect(
                        "rooted temporary file must retain rooted identity",
                    ))
                }),
        };
        match matches {
            Ok(true) => Ok(()),
            Ok(false) => {
                self.state = LocalTempResourceState::Indeterminate;
                Err(Error::new(
                    ErrorKind::InvalidInput,
                    "temporary file path no longer names the created entry",
                ))
            }
            Err(error) => Err(error),
        }
    }

    /// Records whether a failed native install proves the source remains owned.
    #[inline]
    fn record_native_persist_failure(&mut self, error: &Error) {
        self.state = if LocalPersistFailureState::from_error(
            LocalPersistStage::InstallDestination,
            error.kind(),
        ) == LocalPersistFailureState::NotPublished
        {
            LocalTempResourceState::Owned
        } else {
            LocalTempResourceState::Indeterminate
        };
    }
}

impl Write for LocalTempFile {
    /// Writes bytes to the still-open temporary file.
    #[inline(always)]
    fn write(&mut self, buffer: &[u8]) -> Result<usize> {
        self.as_file_mut()?.write(buffer)
    }

    /// Writes vectored bytes to the still-open temporary file.
    #[inline(always)]
    fn write_vectored(&mut self, buffers: &[IoSlice<'_>]) -> Result<usize> {
        #[cfg(windows)]
        {
            let mut written = 0;
            for buffer in buffers {
                let count = self.write(buffer)?;
                written += count;
                if count < buffer.len() {
                    break;
                }
            }
            Ok(written)
        }

        #[cfg(not(windows))]
        self.as_file_mut()?.write_vectored(buffers)
    }

    /// Flushes the still-open temporary file.
    #[inline(always)]
    fn flush(&mut self) -> Result<()> {
        self.as_file_mut()?.flush()
    }
}

impl Seek for LocalTempFile {
    /// Seeks the still-open temporary file.
    #[inline(always)]
    fn seek(&mut self, position: SeekFrom) -> Result<u64> {
        self.as_file_mut()?.seek(position)
    }
}

impl Drop for LocalTempFile {
    /// Performs best-effort cleanup only while the resource remains owned.
    fn drop(&mut self) {
        self.close();
        match self.state {
            LocalTempResourceState::Owned => {
                if let Err(error) = self.remove_resource() {
                    // TODO: Route cleanup diagnostics through caller-controlled
                    // redaction, sampling, and metrics policy.
                    warn!(
                        "failed to remove temporary file {}: {}",
                        self.path.display(),
                        error
                    );
                    return;
                }
                self.state = LocalTempResourceState::SandboxPending;
                if let Err(error) = self.release_sandbox() {
                    // TODO: Route cleanup diagnostics through caller-controlled
                    // redaction, sampling, and metrics policy.
                    warn!(
                        "failed to remove temporary file sandbox for {}: {}",
                        self.path.display(),
                        error
                    );
                } else {
                    self.state = LocalTempResourceState::Released;
                }
            }
            LocalTempResourceState::SandboxPending => {
                if let Err(error) = self.release_sandbox() {
                    // TODO: Route cleanup diagnostics through caller-controlled
                    // redaction, sampling, and metrics policy.
                    warn!(
                        "failed to remove temporary file sandbox for {}: {}",
                        self.path.display(),
                        error
                    );
                } else {
                    self.state = LocalTempResourceState::Released;
                }
            }
            LocalTempResourceState::Indeterminate
            | LocalTempResourceState::Released => {}
        }
    }
}

/// Builds the error used after a temporary file handle was closed.
#[must_use]
#[inline]
fn closed_file_error() -> Error {
    Error::new(ErrorKind::BrokenPipe, "temporary file handle is closed")
}
