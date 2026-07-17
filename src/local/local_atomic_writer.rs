// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
// =============================================================================
//! Streaming durable atomic file replacement.

use std::fs::{
    self,
    File,
};
use std::io::{
    self,
    ErrorKind,
    Write,
};
use std::path::{
    Path,
    PathBuf,
};

use crate::{
    LocalAtomicWriteError,
    LocalAtomicWriteStage,
};

use super::internal::{
    DEFAULT_TEMP_FILE_RETRIES,
    StagedFile,
    absolute_path,
    add_path_context,
    create_temp_file_in_dir,
    ensure_parent_path_with_sync_dirs,
    parent_dir_for,
    replace_file,
    sync_parent_dir,
};

/// Default suffix used by atomic-write temporary files.
const ATOMIC_WRITE_TEMP_SUFFIX: &str = ".tmp";

/// Prefix used by atomic-write temporary files.
const ATOMIC_WRITE_TEMP_PREFIX: &str = ".atomic-write-";

/// A streaming same-directory atomic file writer.
///
/// Bytes written through [`Write`] remain in a private staging file until
/// [`Self::commit`] succeeds. Calling [`Self::abort`] or dropping the writer
/// leaves the destination unchanged and removes the staging file on a
/// best-effort basis. This initial API intentionally does not implement
/// [`std::io::Seek`]. A relative destination is bound to the process current
/// directory when the writer is created, so later current-directory changes
/// do not redirect commit or cleanup operations.
///
/// The guard must be committed or explicitly aborted:
///
/// ```compile_fail
/// #![deny(unused_must_use)]
/// use qubit_local_files::LocalFiles;
///
/// let writer = LocalFiles::begin_atomic_write("result.bin")?;
/// writer;
/// # Ok::<(), qubit_local_files::LocalAtomicWriteError>(())
/// ```
#[must_use = "atomic writes have no effect unless the writer is committed"]
#[derive(Debug)]
pub struct LocalAtomicWriter {
    /// Requested destination path.
    path: PathBuf,
    /// Absolute destination path used by filesystem operations.
    operation_path: PathBuf,
    /// Newly created parent directories that require synchronization.
    parent_dirs_to_sync: Vec<PathBuf>,
    /// Existing regular-file permissions preserved at commit time.
    existing_permissions: Option<fs::Permissions>,
    /// Owned same-directory staging file.
    staged_file: StagedFile,
}

impl LocalAtomicWriter {
    /// Creates a streaming atomic writer for `path`.
    ///
    /// # Parameters
    /// - `path`: Destination path to replace on commit.
    ///
    /// # Errors
    /// Returns a structured error when parent preparation, destination
    /// inspection, or staging-file creation fails.
    pub(crate) fn new(path: &Path) -> Result<Self, LocalAtomicWriteError> {
        let operation_path = with_atomic_context(
            absolute_path(path),
            LocalAtomicWriteStage::PrepareParent,
            path,
            None,
            false,
        )?;
        let parent_dirs_to_sync = with_atomic_context(
            ensure_parent_path_with_sync_dirs(&operation_path),
            LocalAtomicWriteStage::PrepareParent,
            path,
            None,
            false,
        )?;
        let existing_permissions = with_atomic_context(
            existing_file_permissions(&operation_path),
            LocalAtomicWriteStage::InspectDestination,
            path,
            None,
            false,
        )?;
        let parent = parent_dir_for(&operation_path);
        let (temp_path, file) = with_atomic_context(
            create_temp_file_in_dir(
                parent,
                Some(ATOMIC_WRITE_TEMP_PREFIX),
                Some(ATOMIC_WRITE_TEMP_SUFFIX),
                DEFAULT_TEMP_FILE_RETRIES,
            ),
            LocalAtomicWriteStage::CreateTemporaryFile,
            path,
            None,
            false,
        )?;
        Ok(Self {
            path: path.to_path_buf(),
            operation_path,
            parent_dirs_to_sync,
            existing_permissions,
            staged_file: StagedFile::new(temp_path, file),
        })
    }

    /// Synchronizes and atomically replaces the destination.
    ///
    /// # Errors
    /// Returns a structured error when permission preservation, staging-file
    /// synchronization, destination replacement, or parent synchronization
    /// fails. A parent synchronization error may report `committed = true`.
    pub fn commit(mut self) -> Result<(), LocalAtomicWriteError> {
        let result = apply_existing_permissions(
            self.staged_file.file(),
            self.existing_permissions.as_ref(),
            self.staged_file.path(),
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

        self.staged_file.close();
        let result =
            replace_file(self.staged_file.path(), &self.operation_path);
        with_staging_cleanup(
            result,
            LocalAtomicWriteStage::ReplaceDestination,
            &self.path,
            &mut self.staged_file,
        )?;
        let temporary_path = self.staged_file.path().to_path_buf();
        self.staged_file.disarm();
        with_atomic_context(
            sync_atomic_parent_chain(
                &self.operation_path,
                &self.parent_dirs_to_sync,
            ),
            LocalAtomicWriteStage::SyncParentDirectory,
            &self.path,
            Some(temporary_path),
            true,
        )
    }

    /// Aborts the staged replacement and removes its temporary file.
    ///
    /// The destination remains unchanged. If explicit cleanup fails, the
    /// internal staging guard retries best-effort cleanup as this method exits.
    ///
    /// # Errors
    /// Returns a structured cleanup error when the temporary file cannot be
    /// removed.
    pub fn abort(mut self) -> Result<(), LocalAtomicWriteError> {
        let temporary_path = self.staged_file.path().to_path_buf();
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

    /// Writes all bytes and commits the destination.
    pub(crate) fn write_bytes(
        self,
        bytes: &[u8],
    ) -> Result<(), LocalAtomicWriteError> {
        self.write_with(|writer| writer.write_all(bytes))
    }

    /// Invokes caller-provided staging logic and commits the destination.
    ///
    /// The callback receives the guarded writer itself rather than a [`File`].
    /// Exposing a file would let the callback retain a cloned handle and mutate
    /// the committed inode after rename, invalidating the atomic snapshot.
    ///
    /// # Parameters
    /// - `write`: Callback that writes the complete staged contents.
    ///
    /// # Errors
    /// Returns a structured error when the callback or commit sequence fails.
    /// Callback errors are reported at
    /// [`LocalAtomicWriteStage::WriteTemporaryFile`].
    ///
    /// # Panics
    /// Propagates callback panics after the staging guard attempts best-effort
    /// cleanup while unwinding.
    pub(crate) fn write_with<F>(
        mut self,
        write: F,
    ) -> Result<(), LocalAtomicWriteError>
    where
        F: FnOnce(&mut Self) -> io::Result<()>,
    {
        let result = write(&mut self);
        with_staging_cleanup(
            result,
            LocalAtomicWriteStage::WriteTemporaryFile,
            &self.path,
            &mut self.staged_file,
        )?;
        self.commit()
    }
}

impl Write for LocalAtomicWriter {
    #[inline(always)]
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        self.staged_file.file_mut().write(buffer)
    }

    #[inline(always)]
    fn flush(&mut self) -> io::Result<()> {
        self.staged_file.file_mut().flush()
    }
}

/// Adds atomic-write context to a native I/O result.
fn with_atomic_context<T>(
    result: io::Result<T>,
    stage: LocalAtomicWriteStage,
    path: &Path,
    temporary_path: Option<PathBuf>,
    committed: bool,
) -> Result<T, LocalAtomicWriteError> {
    result.map_err(|source| {
        LocalAtomicWriteError::new(
            stage,
            path.to_path_buf(),
            temporary_path,
            committed,
            source,
        )
    })
}

/// Creates an atomic-write error and explicitly cleans up its staging file.
fn atomic_error_with_staging(
    stage: LocalAtomicWriteStage,
    path: &Path,
    source: io::Error,
    staged_file: &mut StagedFile,
) -> LocalAtomicWriteError {
    let temporary_path = staged_file.path().to_path_buf();
    let cleanup_error = staged_file.cleanup().err();
    LocalAtomicWriteError::new(
        stage,
        path.to_path_buf(),
        Some(temporary_path),
        false,
        source,
    )
    .with_cleanup_error(cleanup_error)
}

/// Adds atomic-write context and cleanup to a staging operation result.
fn with_staging_cleanup<T>(
    result: io::Result<T>,
    stage: LocalAtomicWriteStage,
    path: &Path,
    staged_file: &mut StagedFile,
) -> Result<T, LocalAtomicWriteError> {
    result.map_err(|source| {
        atomic_error_with_staging(stage, path, source, staged_file)
    })
}

/// Synchronizes the destination and every newly created parent entry.
fn sync_atomic_parent_chain(
    path: &Path,
    parent_dirs_to_sync: &[PathBuf],
) -> io::Result<()> {
    sync_parent_dir(path)?;
    for directory in parent_dirs_to_sync.iter().rev() {
        sync_parent_dir(directory)?;
    }
    Ok(())
}

/// Returns existing regular-file permissions for atomic replacement.
fn existing_file_permissions(
    path: &Path,
) -> io::Result<Option<fs::Permissions>> {
    match fs::symlink_metadata(path) {
        Ok(metadata)
            if metadata.is_file() && !metadata.file_type().is_symlink() =>
        {
            Ok(Some(metadata.permissions()))
        }
        Ok(_) => Ok(None),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
        Err(error) => {
            Err(add_path_context(error, "read destination metadata", path))
        }
    }
}

/// Applies preserved destination permissions to the staging file.
fn apply_existing_permissions(
    file: &File,
    permissions: Option<&fs::Permissions>,
    temporary_path: &Path,
) -> io::Result<()> {
    if let Some(permissions) = permissions
        && let Err(error) = file.set_permissions(permissions.clone())
    {
        return Err(add_path_context(
            error,
            "set temporary file permissions",
            temporary_path,
        ));
    }
    Ok(())
}
