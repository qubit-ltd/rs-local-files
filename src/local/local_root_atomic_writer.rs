// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
// =============================================================================
//! Streaming descriptor-relative atomic file replacement.

#[cfg(unix)]
use std::ffi::CString;
#[cfg(unix)]
use std::fs::{
    self,
    File,
};
use std::io::{
    self,
    Write,
};
use std::path::{
    Path,
    PathBuf,
};

#[cfg(unix)]
use crate::LocalRelativePath;
use crate::{
    LocalAtomicWriteError,
    LocalAtomicWriteStage,
};

#[cfg(unix)]
use super::internal::{
    RootedStagedFile,
    create_rooted_staged_file,
    existing_rooted_file_permissions,
    open_rooted_parent,
};

/// A streaming atomic writer contained by an open [`crate::LocalRoot`].
///
/// Staging, replacement, synchronization, and cleanup use the destination
/// parent descriptor and entry names. No diagnostic path is reused as
/// authority, and no underlying file or directory handle is exposed.
///
/// ```compile_fail
/// #![deny(unused_must_use)]
/// use qubit_local_files::{LocalRelativePath, LocalRoot};
///
/// let root = LocalRoot::open(".").unwrap();
/// let path = LocalRelativePath::new("result.bin").unwrap();
/// let writer = root.begin_atomic_write(&path).unwrap();
/// writer;
/// ```
#[must_use = "rooted atomic writes have no effect unless committed"]
#[derive(Debug)]
pub struct LocalRootAtomicWriter {
    /// Requested relative destination retained for structured errors.
    path: PathBuf,
    #[cfg(unix)]
    /// Final destination entry name within the staging parent.
    final_name: CString,
    #[cfg(unix)]
    /// Existing ordinary-file permissions preserved at commit.
    existing_permissions: Option<fs::Permissions>,
    #[cfg(unix)]
    /// Descriptor-relative staging lifecycle.
    staged_file: RootedStagedFile,
}

impl LocalRootAtomicWriter {
    #[cfg(unix)]
    /// Creates a rooted atomic writer from an open root capability.
    ///
    /// # Parameters
    ///
    /// * `root` - Open root directory authority.
    /// * `diagnostic_root` - Path used only to contextualize traversal errors.
    /// * `path` - Validated relative destination.
    ///
    /// # Returns
    ///
    /// An armed rooted atomic writer.
    ///
    /// # Errors
    ///
    /// Returns a structured error for parent preparation, destination
    /// inspection, or staging-file creation failures.
    pub(crate) fn new(
        root: &File,
        diagnostic_root: &Path,
        path: &LocalRelativePath,
    ) -> Result<Self, LocalAtomicWriteError> {
        let requested_path = path.as_path().to_path_buf();
        let diagnostic_path = diagnostic_root.join(path.as_path());
        let (parent, final_name) = map_atomic_error(
            open_rooted_parent(root, &diagnostic_path, path, true),
            LocalAtomicWriteStage::PrepareParent,
            &requested_path,
            None,
            false,
        )?;
        let existing_permissions = map_atomic_error(
            existing_rooted_file_permissions(&parent, &final_name),
            LocalAtomicWriteStage::InspectDestination,
            &requested_path,
            None,
            false,
        )?;
        let relative_parent = path.as_path().parent().unwrap_or(Path::new(""));
        let staged_file = map_atomic_error(
            create_rooted_staged_file(parent, relative_parent),
            LocalAtomicWriteStage::CreateTemporaryFile,
            &requested_path,
            None,
            false,
        )?;
        Ok(Self {
            path: requested_path,
            final_name,
            existing_permissions,
            staged_file,
        })
    }

    /// Synchronizes and atomically replaces the rooted destination.
    ///
    /// # Errors
    ///
    /// Returns a structured error when permission preservation, staging-file
    /// synchronization, replacement, or parent-directory synchronization
    /// fails. A parent synchronization failure reports `committed = true`.
    #[cfg_attr(not(unix), allow(unused_mut))]
    pub fn commit(mut self) -> Result<(), LocalAtomicWriteError> {
        #[cfg(unix)]
        {
            let result = apply_existing_permissions(
                self.staged_file.file(),
                self.existing_permissions.as_ref(),
            );
            with_staging_cleanup(
                result,
                LocalAtomicWriteStage::PreservePermissions,
                &self.path,
                &mut self.staged_file,
            )?;
            let result = self.staged_file.file().sync_all();
            with_staging_cleanup(
                result,
                LocalAtomicWriteStage::SyncTemporaryFile,
                &self.path,
                &mut self.staged_file,
            )?;
            // Reinspect the final name immediately before replacement. The
            // subsequent same-parent `renameat` never follows a later link,
            // but rejecting a link already visible here preserves the public
            // no-symbolic-link policy as well as containment.
            let result = existing_rooted_file_permissions(
                self.staged_file.parent(),
                &self.final_name,
            )
            .map(drop);
            with_staging_cleanup(
                result,
                LocalAtomicWriteStage::ReplaceDestination,
                &self.path,
                &mut self.staged_file,
            )?;
            let result = self.staged_file.rename_to(&self.final_name);
            with_staging_cleanup(
                result,
                LocalAtomicWriteStage::ReplaceDestination,
                &self.path,
                &mut self.staged_file,
            )?;
            let temporary_path =
                self.staged_file.diagnostic_path().to_path_buf();
            self.staged_file.disarm();
            map_atomic_error(
                self.staged_file.parent().sync_all(),
                LocalAtomicWriteStage::SyncParentDirectory,
                &self.path,
                Some(temporary_path),
                true,
            )
        }
        #[cfg(not(unix))]
        {
            Err(unsupported_atomic_error(&self.path))
        }
    }

    /// Aborts replacement and removes the descriptor-relative staging entry.
    ///
    /// # Errors
    ///
    /// Returns a structured cleanup error when `unlinkat` fails.
    #[cfg_attr(not(unix), allow(unused_mut))]
    pub fn abort(mut self) -> Result<(), LocalAtomicWriteError> {
        #[cfg(unix)]
        {
            let temporary_path =
                self.staged_file.diagnostic_path().to_path_buf();
            match self.staged_file.cleanup() {
                Ok(()) => Ok(()),
                Err(source) => Err(LocalAtomicWriteError::new(
                    LocalAtomicWriteStage::CleanupTemporaryFile,
                    self.path.clone(),
                    Some(temporary_path),
                    false,
                    source,
                )),
            }
        }
        #[cfg(not(unix))]
        {
            Err(unsupported_atomic_error(&self.path))
        }
    }
}

impl Write for LocalRootAtomicWriter {
    /// Writes bytes into the private rooted staging file.
    #[inline(always)]
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        #[cfg(unix)]
        {
            self.staged_file.file_mut().write(buffer)
        }
        #[cfg(not(unix))]
        {
            let _ = buffer;
            Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "secure rooted atomic writes are unsupported on this target",
            ))
        }
    }

    /// Flushes userspace data into the private rooted staging file.
    #[inline(always)]
    fn flush(&mut self) -> io::Result<()> {
        #[cfg(unix)]
        {
            self.staged_file.file_mut().flush()
        }
        #[cfg(not(unix))]
        {
            Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "secure rooted atomic writes are unsupported on this target",
            ))
        }
    }
}

#[cfg(unix)]
/// Applies existing destination permissions to a staging file.
///
/// # Parameters
///
/// * `file` - Open staging file handle.
/// * `permissions` - Existing permissions to preserve, when any.
///
/// # Errors
///
/// Returns the operating-system error from permission application.
fn apply_existing_permissions(
    file: &File,
    permissions: Option<&fs::Permissions>,
) -> io::Result<()> {
    if let Some(permissions) = permissions {
        file.set_permissions(permissions.clone())?;
    }
    Ok(())
}

#[cfg(unix)]
/// Adds structured atomic context to a native I/O result.
///
/// # Parameters
///
/// * `result` - Native result to map.
/// * `stage` - Atomic stage associated with failure.
/// * `path` - Requested relative destination.
/// * `temporary_path` - Optional diagnostic staging path.
/// * `committed` - Whether replacement has already completed.
///
/// # Returns
///
/// The successful value or a structured atomic error.
fn map_atomic_error<T>(
    result: io::Result<T>,
    stage: LocalAtomicWriteStage,
    path: &Path,
    temporary_path: Option<PathBuf>,
    committed: bool,
) -> Result<T, LocalAtomicWriteError> {
    match result {
        Ok(value) => Ok(value),
        Err(source) => Err(LocalAtomicWriteError::new(
            stage,
            path.to_path_buf(),
            temporary_path,
            committed,
            source,
        )),
    }
}

#[cfg(unix)]
/// Maps a staging failure and attempts explicit cleanup.
///
/// # Parameters
///
/// * `result` - Staging operation result to map.
/// * `stage` - Atomic stage associated with failure.
/// * `path` - Requested relative destination.
/// * `staged_file` - Armed staging guard to clean on failure.
///
/// # Returns
///
/// The successful value or a structured error retaining any cleanup failure.
fn with_staging_cleanup<T>(
    result: io::Result<T>,
    stage: LocalAtomicWriteStage,
    path: &Path,
    staged_file: &mut RootedStagedFile,
) -> Result<T, LocalAtomicWriteError> {
    match result {
        Ok(value) => Ok(value),
        Err(source) => {
            let temporary_path = staged_file.diagnostic_path().to_path_buf();
            let cleanup_error = staged_file.cleanup().err();
            Err(LocalAtomicWriteError::new(
                stage,
                path.to_path_buf(),
                Some(temporary_path),
                false,
                source,
            )
            .with_cleanup_error(cleanup_error))
        }
    }
}

#[cfg(not(unix))]
/// Creates a structured unsupported rooted atomic-write error.
///
/// # Parameters
///
/// * `path` - Requested relative destination.
///
/// # Returns
///
/// An unsupported error that never falls back to ordinary path authority.
fn unsupported_atomic_error(path: &Path) -> LocalAtomicWriteError {
    LocalAtomicWriteError::new(
        LocalAtomicWriteStage::PrepareParent,
        path.to_path_buf(),
        None,
        false,
        io::Error::new(
            io::ErrorKind::Unsupported,
            "secure rooted atomic writes are unsupported on this target",
        ),
    )
}
