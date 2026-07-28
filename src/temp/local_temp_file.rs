//! Cleanup-owned temporary files with host or rooted authority.

use std::{
    fs::File,
    io::{Error, ErrorKind, IoSlice, Result, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
    sync::Arc,
};

use log::warn;

use crate::{LocalPersistError, LocalPersistOptions, LocalPersistStage, LocalRelativePath};

use super::internal::{
    LocalTempResourceBackend, LocalTempResourceState, RootedTempResourceBackend,
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
    /// Namespace certainty governing cleanup and drop behavior.
    state: LocalTempResourceState,
}

impl LocalTempFile {
    /// Builds a host temporary file from its already-bound path and handle.
    pub(crate) fn host(path: PathBuf, file: File) -> Self {
        Self {
            path,
            backend: LocalTempResourceBackend::Host(super::internal::HostTempResourceBackend),
            file: Some(file),
            state: LocalTempResourceState::Owned,
        }
    }

    /// Builds a rooted temporary file from the retained root authority.
    pub(crate) fn rooted(root: Arc<crate::rooted::Root>, path: PathBuf, file: File) -> Self {
        Self {
            path: path.clone(),
            backend: LocalTempResourceBackend::Rooted(RootedTempResourceBackend {
                root,
                relative_path: path,
            }),
            file: Some(file),
            state: LocalTempResourceState::Owned,
        }
    }

    /// Returns the authority-local generated path.
    #[must_use]
    #[inline(always)]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Closes the file I/O handle while retaining cleanup and persistence responsibility.
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
    pub fn keep(mut self) -> PathBuf {
        self.close();
        self.state = LocalTempResourceState::Released;
        self.path.clone()
    }

    /// Persists the file within its creating authority without replacement.
    pub fn persist(
        self,
        target: impl AsRef<Path>,
    ) -> std::result::Result<PathBuf, LocalPersistError<Self>> {
        self.persist_with(target, LocalPersistOptions::new())
    }

    /// Persists the file with explicit replacement policy within its creating authority.
    pub fn persist_with(
        mut self,
        target: impl AsRef<Path>,
        options: LocalPersistOptions,
    ) -> std::result::Result<PathBuf, LocalPersistError<Self>> {
        self.close();
        if self.state == LocalTempResourceState::Indeterminate {
            return Err(LocalPersistError::new(
                Error::other("temporary file namespace state is indeterminate"),
                self,
                target.as_ref().to_path_buf(),
                None,
                LocalPersistStage::InstallDestination,
            ));
        }
        let requested_target = target.as_ref().to_path_buf();
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
            return Ok(target);
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
        Ok(target)
    }

    /// Returns the mutable open file handle, or an error after [`Self::close`].
    pub fn as_file_mut(&mut self) -> Result<&mut File> {
        self.file.as_mut().ok_or_else(closed_file_error)
    }

    /// Removes the resource using the retained backend rather than a diagnostic path.
    fn remove(&self) -> Result<()> {
        match &self.backend {
            LocalTempResourceBackend::Host(_) => std::fs::remove_file(&self.path),
            LocalTempResourceBackend::Rooted(rooted) => {
                let path = LocalRelativePath::new(&rooted.relative_path)
                    .expect("rooted temporary path was validated at creation");
                rooted.root.remove_file(&path)
            }
        }
    }

    /// Rejects namespace cleanup after an indeterminate native publication attempt.
    fn ensure_cleanup_safe(&self) -> Result<()> {
        if self.state == LocalTempResourceState::Indeterminate {
            return Err(Error::other(
                "temporary file namespace state is indeterminate; cleanup is unsafe",
            ));
        }
        Ok(())
    }

    /// Records whether a failed native install proves the source remains owned.
    fn record_native_persist_failure(&mut self, error: &Error) {
        self.state = if error.kind() == ErrorKind::AlreadyExists {
            LocalTempResourceState::Owned
        } else {
            LocalTempResourceState::Indeterminate
        };
    }
}

impl Write for LocalTempFile {
    /// Writes bytes to the still-open temporary file.
    fn write(&mut self, buffer: &[u8]) -> Result<usize> {
        self.as_file_mut()?.write(buffer)
    }

    /// Writes vectored bytes to the still-open temporary file.
    fn write_vectored(&mut self, buffers: &[IoSlice<'_>]) -> Result<usize> {
        self.as_file_mut()?.write_vectored(buffers)
    }

    /// Flushes the still-open temporary file.
    fn flush(&mut self) -> Result<()> {
        self.as_file_mut()?.flush()
    }
}

impl Seek for LocalTempFile {
    /// Seeks the still-open temporary file.
    fn seek(&mut self, position: SeekFrom) -> Result<u64> {
        self.as_file_mut()?.seek(position)
    }
}

impl Drop for LocalTempFile {
    /// Performs best-effort cleanup only while the resource remains owned.
    fn drop(&mut self) {
        self.close();
        if matches!(
            self.state,
            LocalTempResourceState::Owned | LocalTempResourceState::CleanupRequired
        ) && let Err(error) = self.remove()
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
fn closed_file_error() -> Error {
    Error::new(ErrorKind::NotFound, "temporary file handle is closed")
}
