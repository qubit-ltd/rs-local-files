// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Cleanup-owned temporary files with host or rooted authority.

use std::{
    fs::File,
    io::{Error, ErrorKind, IoSlice, Result, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    sync::Arc,
};

use log::warn;

use crate::{
    LocalPersistError, LocalPersistFailureState, LocalPersistMethod, LocalPersistOptions,
    LocalPersistOutcome, LocalPersistStage, LocalRelativePath,
};

use super::internal::{
    LocalTempResourceBackend, LocalTempResourceState, RootedTempResourceBackend, TempEntryIdentity,
};

/// A temporary file whose cleanup remains bound to its creating authority.
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
}

impl LocalTempFile {
    /// Builds a host temporary file from its already-bound path and handle.
    #[inline]
    pub(crate) fn host(path: PathBuf, file: File) -> Result<Self> {
        Ok(Self {
            path,
            backend: LocalTempResourceBackend::Host(super::internal::HostTempResourceBackend),
            host_identity: Some(TempEntryIdentity::from_file(&file)?),
            rooted_identity: None,
            file: Some(file),
            state: LocalTempResourceState::Owned,
        })
    }

    /// Builds a rooted temporary file from the retained root authority.
    #[inline]
    pub(crate) fn rooted(root: Arc<crate::rooted::Root>, path: PathBuf, file: File) -> Result<Self> {
        Ok(Self {
            path: path.clone(),
            backend: LocalTempResourceBackend::Rooted(RootedTempResourceBackend {
                root,
                relative_path: path,
            }),
            host_identity: None,
            rooted_identity: Some(crate::rooted::Metadata::from_open_file(&file)?),
            file: Some(file),
            state: LocalTempResourceState::Owned,
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
    pub fn cleanup(&mut self) -> Result<()> {
        self.close();
        self.ensure_cleanup_safe()?;
        self.remove()?;
        self.state = LocalTempResourceState::Released;
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
    ) -> std::result::Result<PathBuf, LocalPersistError<Self>> {
        self.persist_with(target, LocalPersistOptions::new())
    }

    /// Persists the file with explicit replacement policy within its creating
    /// authority.
    #[inline(always)]
    pub fn persist_with(
        self,
        target: impl AsRef<Path>,
        options: LocalPersistOptions,
    ) -> std::result::Result<PathBuf, LocalPersistError<Self>> {
        self.persist_with_outcome(target, options)
            .map(LocalPersistOutcome::into_path)
    }

    /// Persists the file and reports the achieved publication guarantees.
    #[inline(always)]
    pub fn persist_with_outcome(
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
            let target = match std::path::absolute(&requested_target) {
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
            if let Err(error) = crate::local::ensure_parent_path(&target) {
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
            self.state = LocalTempResourceState::Released;
            return Ok(LocalPersistOutcome::new(
                target,
                LocalPersistMethod::AtomicRename,
                true,
                false,
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
        let destination = LocalRelativePath::new(&target).expect("persist target was validated");
        let result = if options.overwrites() {
            rooted.root.rename(&source, &destination)
        } else {
            rooted.root.rename_without_replacing(&source, &destination)
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
        self.state = LocalTempResourceState::Released;
        Ok(LocalPersistOutcome::new(
            target,
            LocalPersistMethod::AtomicRename,
            true,
            false,
        ))
    }

    /// Removes the resource using the retained backend rather than a diagnostic
    /// path.
    #[inline]
    fn remove(&mut self) -> Result<()> {
        self.ensure_identity_matches()?;
        match &self.backend {
            LocalTempResourceBackend::Host(_) => std::fs::remove_file(&self.path),
            LocalTempResourceBackend::Rooted(rooted) => {
                let path = LocalRelativePath::new(&rooted.relative_path)
                    .expect("rooted temporary path was validated at creation");
                rooted.root.remove_file(&path)
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
                    &LocalRelativePath::new(&rooted.relative_path)
                        .expect("rooted temporary path was validated at creation"),
                )
                .map(|metadata| {
                    metadata.is_same_file(
                        self.rooted_identity
                            .as_ref()
                            .expect("rooted temporary file must retain rooted identity"),
                    )
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
        if matches!(self.state, LocalTempResourceState::Owned)
            && let Err(error) = self.remove()
        {
            warn!(
                "failed to remove temporary file {}: {}",
                self.path.display(),
                error
            );
        }
    }
}

/// Builds the error used after a temporary file handle was closed.
#[must_use]
#[inline]
fn closed_file_error() -> Error {
    Error::new(ErrorKind::NotFound, "temporary file handle is closed")
}
