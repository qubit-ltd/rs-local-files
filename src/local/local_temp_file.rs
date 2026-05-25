/*******************************************************************************
 *
 *    Copyright (c) 2026 Haixing Hu.
 *
 *    SPDX-License-Identifier: Apache-2.0
 *
 *    Licensed under the Apache License, Version 2.0.
 *
 ******************************************************************************/
use std::fs::{
    self,
    File,
};
use std::io::{
    Error,
    ErrorKind,
    Result,
};
use std::path::{
    Path,
    PathBuf,
};

use log::warn;

use crate::{
    FileWriteOptions,
    LocalFileWriter,
    LocalFiles,
    LocalPersistOptions,
};

use super::local_files::{
    create_temp_file_in_dir,
    move_file_without_replacing,
    open_writer_path,
    replace_file,
};

/// Temporary file that is removed automatically unless kept or persisted.
///
/// `LocalTempFile` owns both the temporary file path and the writer state. The
/// writer is closed before the path is removed, kept, or persisted. Use
/// [`LocalTempFile::keep`] to keep the file at its generated path, or
/// [`LocalTempFile::persist`] to move it to a final path.
///
/// Cleanup performed from `Drop` is best-effort. If removal fails, the failure
/// is reported through the `log` facade at warning level and the program is not
/// panicked.
#[derive(Debug)]
pub struct LocalTempFile {
    path: Option<PathBuf>,
    writer: LocalTempFileWriterState,
}

/// Writer state owned by a temporary file.
#[derive(Debug)]
enum LocalTempFileWriterState {
    /// A newly created file handle that has not yet been configured.
    Unconfigured(File),
    /// A configured writer returned through [`LocalTempFile::writer`].
    Configured {
        /// Configured writer.
        writer: LocalFileWriter,
        /// Options used to configure the writer.
        options: FileWriteOptions,
    },
    /// The temporary file writer has been closed.
    Closed,
}

impl LocalTempFile {
    /// Creates a temporary file in the process temporary directory.
    ///
    /// # Errors
    /// Returns an I/O error when the process temporary directory cannot be
    /// created or a unique temporary file cannot be created.
    #[inline]
    pub fn new() -> Result<Self> {
        Self::with_name(None, None)
    }

    /// Creates a temporary file in the process temporary directory.
    ///
    /// # Parameters
    /// - `prefix`: Optional file-name prefix.
    /// - `suffix`: Optional file-name suffix.
    ///
    /// # Errors
    /// Returns an I/O error when the process temporary directory cannot be
    /// created, `prefix` or `suffix` is not a safe file-name fragment, or a
    /// unique temporary file cannot be created.
    #[inline]
    pub fn with_name(prefix: Option<&str>, suffix: Option<&str>) -> Result<Self> {
        Self::in_dir(
            std::env::temp_dir(),
            prefix,
            suffix,
            LocalFiles::DEFAULT_TEMP_FILE_RETRIES,
        )
    }

    /// Creates a temporary file in the specified directory.
    ///
    /// # Parameters
    /// - `dir`: Parent directory in which the temporary file is created.
    /// - `prefix`: Optional file-name prefix.
    /// - `suffix`: Optional file-name suffix.
    /// - `max_tries`: Maximum number of random names to try.
    ///
    /// # Errors
    /// Returns an I/O error when `dir` cannot be created, `prefix` or `suffix`
    /// is not a safe file-name fragment, the retry limit is zero, all generated
    /// names collide, or file creation fails.
    pub fn in_dir<P>(dir: P, prefix: Option<&str>, suffix: Option<&str>, max_tries: usize) -> Result<Self>
    where
        P: AsRef<Path>,
    {
        let (path, file) = create_temp_file_in_dir(dir.as_ref(), prefix, suffix, max_tries)?;
        Ok(Self {
            path: Some(path),
            writer: LocalTempFileWriterState::Unconfigured(file),
        })
    }

    /// Returns the temporary file path.
    ///
    /// # Returns
    /// Borrowed path managed by this temporary file.
    #[inline]
    pub fn path(&self) -> &Path {
        self.path
            .as_deref()
            .expect("temporary file path has already been released")
    }

    /// Tests whether the temporary file path still exists.
    ///
    /// # Returns
    /// `true` when the path exists and `false` when it is missing.
    ///
    /// # Errors
    /// Returns an I/O error when the filesystem cannot determine whether the
    /// path exists. Unlike [`Path::exists`], this method does not silently map
    /// inspection errors to `false`.
    #[inline]
    pub fn exists(&self) -> Result<bool> {
        LocalFiles::exists(self.path())
    }

    /// Reads metadata for the temporary file path.
    ///
    /// # Returns
    /// Metadata for the temporary file path.
    ///
    /// # Errors
    /// Returns the I/O error reported by [`fs::metadata`].
    #[inline]
    pub fn metadata(&self) -> Result<fs::Metadata> {
        LocalFiles::metadata(self.path())
    }

    /// Returns the configured writer for this temporary file.
    ///
    /// The first call opens a writer using `options` and stores it inside the
    /// temporary file. Later calls must pass the same options and return the
    /// same writer. Passing different options after the writer has been
    /// configured is rejected so the write mode cannot silently change midway
    /// through a temporary file.
    ///
    /// Because the temporary file is created before this method is called,
    /// [`crate::FileWriteMode::CreateNew`] fails with
    /// [`ErrorKind::AlreadyExists`].
    ///
    /// # Parameters
    /// - `options`: Write options controlling mode and buffering.
    ///
    /// # Returns
    /// A mutable writer owned by this temporary file.
    ///
    /// # Errors
    /// Returns [`ErrorKind::NotFound`] after [`LocalTempFile::close`] has
    /// closed the writer, [`ErrorKind::InvalidInput`] when a later call passes
    /// different options, or any I/O error reported while opening the writer.
    pub fn writer(&mut self, options: FileWriteOptions) -> Result<&mut LocalFileWriter> {
        match &self.writer {
            LocalTempFileWriterState::Configured {
                options: existing_options,
                ..
            } if *existing_options == options => return self.configured_writer_mut(),
            LocalTempFileWriterState::Configured { .. } => {
                return Err(Error::new(
                    ErrorKind::InvalidInput,
                    "temporary file writer is already configured with different options",
                ));
            }
            LocalTempFileWriterState::Closed => return Err(writer_closed_error()),
            LocalTempFileWriterState::Unconfigured(_) => {}
        }

        let writer = open_writer_path(self.path(), options)?;
        let old_state = std::mem::replace(
            &mut self.writer,
            LocalTempFileWriterState::Configured { writer, options },
        );
        drop(old_state);
        self.configured_writer_mut()
    }

    /// Returns the configured writer after state validation.
    ///
    /// # Returns
    /// The configured writer.
    ///
    /// # Errors
    /// This helper currently cannot return an error when called after the state
    /// has been checked. The result type keeps the public method simple.
    fn configured_writer_mut(&mut self) -> Result<&mut LocalFileWriter> {
        match &mut self.writer {
            LocalTempFileWriterState::Configured { writer, .. } => Ok(writer),
            LocalTempFileWriterState::Unconfigured(_) | LocalTempFileWriterState::Closed => {
                unreachable!("temporary file writer is not configured")
            }
        }
    }

    /// Flushes and closes the temporary file writer while keeping cleanup active.
    ///
    /// # Errors
    /// Returns the I/O error reported while flushing a configured writer.
    pub fn close(&mut self) -> Result<()> {
        let state = std::mem::replace(&mut self.writer, LocalTempFileWriterState::Closed);
        match state {
            LocalTempFileWriterState::Unconfigured(file) => {
                drop(file);
                Ok(())
            }
            LocalTempFileWriterState::Configured { writer, .. } => writer.close(),
            LocalTempFileWriterState::Closed => Ok(()),
        }
    }

    /// Removes the temporary file immediately.
    ///
    /// This consumes the guard and disables the later best-effort cleanup in
    /// `Drop` after removal succeeds. If flushing or removal fails, the guard
    /// still owns the path until it is dropped.
    ///
    /// # Errors
    /// Returns an I/O error when flushing the writer or removing the file
    /// fails.
    pub fn cleanup(mut self) -> Result<()> {
        self.close()?;
        let path = self.path().to_path_buf();
        fs::remove_file(&path)?;
        let _ = self.path.take();
        Ok(())
    }

    /// Keeps the temporary file at its generated path.
    ///
    /// This consumes the guard, flushes and closes the writer, and disables
    /// automatic cleanup.
    ///
    /// # Returns
    /// The generated temporary file path.
    ///
    /// # Errors
    /// Returns the I/O error reported while flushing a configured writer.
    pub fn keep(mut self) -> Result<PathBuf> {
        self.close()?;
        Ok(self.path.take().expect("temporary file path has already been released"))
    }

    /// Moves the temporary file to a final path without overwriting.
    ///
    /// The writer is flushed and closed before moving. Parent directories for
    /// `target` are created before moving. Existing targets are rejected by the
    /// move operation instead of by a separate metadata precheck. Use
    /// [`LocalTempFile::persist_with`] and [`LocalPersistOptions`] when
    /// overwriting is intended. If the move fails, the temporary file remains
    /// owned by this guard and is cleaned up when the guard is dropped.
    ///
    /// # Parameters
    /// - `target`: Final file path.
    ///
    /// # Returns
    /// The final file path.
    ///
    /// # Errors
    /// Returns an I/O error when the parent directory cannot be created, the
    /// target already exists, or the temporary file cannot be moved to
    /// `target`.
    #[inline]
    pub fn persist<P>(self, target: P) -> Result<PathBuf>
    where
        P: AsRef<Path>,
    {
        self.persist_with(target, LocalPersistOptions::default())
    }

    /// Moves the temporary file to a final path using persistence options.
    ///
    /// The writer is flushed and closed before moving the path. Parent
    /// directories for `target` are created before moving. When
    /// `options.overwrite` is `false`, existing targets are rejected by the move
    /// operation. When
    /// `options.overwrite` is `true`, an existing target file may be replaced.
    ///
    /// # Parameters
    /// - `target`: Final file path.
    /// - `options`: Persistence behavior options.
    ///
    /// # Returns
    /// The final file path.
    ///
    /// # Errors
    /// Returns an I/O error when the parent directory cannot be created, the
    /// target already exists while overwriting is disabled, or the temporary
    /// file cannot be moved to `target`.
    pub fn persist_with<P>(mut self, target: P, options: LocalPersistOptions) -> Result<PathBuf>
    where
        P: AsRef<Path>,
    {
        self.close()?;
        let target = target.as_ref().to_path_buf();
        LocalFiles::ensure_parent(&target)?;
        let source = self
            .path
            .as_ref()
            .expect("temporary file path has already been released");
        if options.overwrite {
            replace_file(source, &target)?;
        } else {
            move_file_without_replacing(source, &target)?;
        }
        let _ = self.path.take();
        Ok(target)
    }
}

impl Drop for LocalTempFile {
    /// Closes and removes the temporary file unless ownership has been released.
    fn drop(&mut self) {
        drop(self.close());
        if let Some(path) = self.path.take()
            && let Err(error) = fs::remove_file(&path)
        {
            warn!("failed to remove temporary file {}: {}", path.display(), error);
        }
    }
}

/// Creates the error returned when a temporary file writer is closed.
///
/// # Returns
/// An [`ErrorKind::NotFound`] error describing the closed writer.
fn writer_closed_error() -> Error {
    Error::new(ErrorKind::NotFound, "temporary file writer is closed")
}
