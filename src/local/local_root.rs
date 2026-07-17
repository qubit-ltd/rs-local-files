// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Open directory capabilities for contained local filesystem operations.

#[cfg(unix)]
use std::fs::File;
use std::io::Result;
#[cfg(not(unix))]
use std::io::{
    Error,
    ErrorKind,
};
use std::path::{
    Path,
    PathBuf,
};

#[cfg(not(unix))]
use crate::LocalAtomicWriteStage;
use crate::{
    FileReadOptions,
    FileWriteOptions,
    LocalAtomicWriteError,
    LocalFileReader,
    LocalFileWriter,
    LocalRelativePath,
    LocalRootAtomicWriter,
};

use super::internal::absolute_path;
#[cfg(unix)]
use super::internal::{
    open_root_directory,
    open_rooted_reader,
    open_rooted_writer,
};

/// An open local directory capability used as the authority for descendants.
///
/// The diagnostic path never authorizes descendant access. Supported rooted
/// operations traverse exclusively from the open directory handle and deny
/// symbolic links at every component. Platforms without a proven secure
/// backend return [`std::io::ErrorKind::Unsupported`] from [`Self::open`].
#[must_use]
#[derive(Debug)]
pub struct LocalRoot {
    /// Absolute path retained only for display and error diagnostics.
    path: PathBuf,
    #[cfg(unix)]
    /// Open directory descriptor that is the sole filesystem authority.
    directory: File,
}

impl LocalRoot {
    /// Opens and anchors a local filesystem root.
    ///
    /// # Parameters
    ///
    /// * `root` - Directory path to bind as the root capability.
    ///
    /// # Returns
    ///
    /// A root whose descendant operations remain anchored after path renames.
    ///
    /// # Errors
    ///
    /// Returns an I/O error when the root cannot be made absolute or opened as
    /// a real directory without following a link. Returns
    /// [`std::io::ErrorKind::Unsupported`] when no secure backend exists on the
    /// target.
    pub fn open<P>(root: P) -> Result<Self>
    where
        P: AsRef<Path>,
    {
        let path = absolute_path(root.as_ref())?;
        #[cfg(unix)]
        {
            let directory = open_root_directory(&path)?;
            Ok(Self { path, directory })
        }
        #[cfg(not(unix))]
        {
            let _ = path;
            Err(unsupported_root_error())
        }
    }

    /// Returns the absolute diagnostic path captured when the root was opened.
    ///
    /// The path is not used as filesystem authority and may no longer name the
    /// opened directory after a rename.
    ///
    /// # Returns
    ///
    /// The root's diagnostic path.
    #[inline(always)]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Opens an ordinary file reader beneath this root.
    ///
    /// # Parameters
    ///
    /// * `path` - Validated relative descendant path.
    /// * `options` - Buffering options for the returned reader.
    ///
    /// # Returns
    ///
    /// A reader for the anchored ordinary file.
    ///
    /// # Errors
    ///
    /// Returns an I/O error when a component is missing, is a symbolic link,
    /// is the wrong resource type, or cannot be opened. Unsupported targets
    /// return [`std::io::ErrorKind::Unsupported`].
    pub fn open_reader(
        &self,
        path: &LocalRelativePath,
        options: FileReadOptions,
    ) -> Result<LocalFileReader> {
        #[cfg(unix)]
        {
            open_rooted_reader(&self.directory, &self.path, path, options)
        }
        #[cfg(not(unix))]
        {
            let _ = (path, options);
            Err(unsupported_root_error())
        }
    }

    /// Opens an ordinary file writer beneath this root.
    ///
    /// Missing parent directories are created through anchored directory
    /// descriptors only when requested by `options`.
    ///
    /// # Parameters
    ///
    /// * `path` - Validated relative descendant path.
    /// * `options` - Parent-creation, mode, and buffering options.
    ///
    /// # Returns
    ///
    /// A writer for the anchored ordinary file.
    ///
    /// # Errors
    ///
    /// Returns an I/O error when a component is missing, is a symbolic link,
    /// is the wrong resource type, cannot be created, or cannot be opened with
    /// the requested mode. Unsupported targets return
    /// [`std::io::ErrorKind::Unsupported`].
    pub fn open_writer(
        &self,
        path: &LocalRelativePath,
        options: FileWriteOptions,
    ) -> Result<LocalFileWriter> {
        #[cfg(unix)]
        {
            open_rooted_writer(&self.directory, &self.path, path, options)
        }
        #[cfg(not(unix))]
        {
            let _ = (path, options);
            Err(unsupported_root_error())
        }
    }

    /// Begins a descriptor-relative atomic replacement beneath this root.
    ///
    /// # Parameters
    ///
    /// * `path` - Validated relative destination path.
    ///
    /// # Returns
    ///
    /// An armed streaming writer whose commit remains within the destination
    /// parent descriptor.
    ///
    /// # Errors
    ///
    /// Returns a structured atomic-write error for parent preparation,
    /// destination inspection, staging creation, or unsupported secure
    /// backend failures.
    pub fn begin_atomic_write(
        &self,
        path: &LocalRelativePath,
    ) -> std::result::Result<LocalRootAtomicWriter, LocalAtomicWriteError> {
        #[cfg(unix)]
        {
            LocalRootAtomicWriter::new(&self.directory, &self.path, path)
        }
        #[cfg(not(unix))]
        {
            Err(LocalAtomicWriteError::new(
                LocalAtomicWriteStage::PrepareParent,
                path.as_path().to_path_buf(),
                None,
                false,
                unsupported_root_error(),
            ))
        }
    }
}

#[cfg(not(unix))]
/// Creates the conservative error used when no rooted backend is available.
///
/// # Returns
///
/// An unsupported-operation error that never falls back to path authority.
fn unsupported_root_error() -> Error {
    Error::new(
        ErrorKind::Unsupported,
        "secure rooted filesystem operations are unsupported on this target",
    )
}
