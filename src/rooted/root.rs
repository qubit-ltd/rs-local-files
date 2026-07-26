// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Open directory capabilities.

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

#[cfg(unix)]
use crate::local;
#[cfg(not(unix))]
use crate::{
    LocalAtomicDestinationState,
    LocalAtomicWriteStage,
};
use crate::{
    atomic,
    read,
    write,
};

use super::path;
use super::{
    Entry,
    Metadata,
    Writer,
};

// TODO: Implement a Windows directory-handle backend before advertising
// rooted support on non-Unix platforms.

/// An opened directory descriptor that authorizes contained operations.
#[must_use]
#[derive(Debug)]
pub struct Root {
    /// Absolute path retained only for diagnostics.
    path: PathBuf,
    /// Open descriptor used as the sole descendant authority.
    #[cfg(unix)]
    directory: File,
}

impl Root {
    /// Opens and anchors a local filesystem root.
    ///
    /// # Errors
    /// Returns an I/O error when the directory cannot be securely opened.
    pub fn open(path: &Path) -> Result<Self> {
        let path = std::path::absolute(path)?;
        #[cfg(unix)]
        {
            let directory = local::open_root_directory(&path)?;
            Ok(Self { path, directory })
        }
        #[cfg(not(unix))]
        {
            let _ = path;
            Err(Error::new(
                ErrorKind::Unsupported,
                "descriptor-relative local roots are unsupported on this platform",
            ))
        }
    }

    /// Returns the diagnostic path captured when the root was opened.
    #[must_use]
    #[inline(always)]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Reads metadata for the opened root directory through its descriptor.
    ///
    /// # Returns
    /// Metadata for the directory that was securely opened by [`Self::open`].
    ///
    /// # Errors
    /// Returns an I/O error when the operating system cannot inspect the open
    /// root descriptor.
    pub fn metadata(&self) -> Result<Metadata> {
        #[cfg(unix)]
        {
            self.directory
                .metadata()
                .map(|metadata| Metadata::from_native(&metadata))
        }
        #[cfg(not(unix))]
        {
            Err(Error::new(
                ErrorKind::Unsupported,
                "descriptor-relative local roots are unsupported on this platform",
            ))
        }
    }

    /// Reads final-entry metadata without following a symbolic link.
    ///
    /// # Parameters
    ///
    /// * `path` - Validated non-empty relative path beneath this root.
    ///
    /// # Returns
    /// Metadata for the final entry itself, including a symbolic link when the
    /// final entry is a link.
    ///
    /// # Errors
    /// Returns an I/O error when traversal cannot remain beneath the opened
    /// root or when the final entry cannot be inspected.
    pub fn symlink_metadata(&self, path: &path::Path) -> Result<Metadata> {
        #[cfg(unix)]
        {
            local::read_rooted_symlink_metadata(
                &self.directory,
                &self.path,
                path,
            )
            .map(|status| Metadata::from_stat(&status))
        }
        #[cfg(not(unix))]
        {
            let _ = path;
            Err(Error::new(
                ErrorKind::Unsupported,
                "descriptor-relative local roots are unsupported on this platform",
            ))
        }
    }

    /// Lists immediate children of the opened root directory.
    ///
    /// # Errors
    /// Returns an I/O error when the descriptor cannot be enumerated or an
    /// entry cannot be inspected without following links.
    pub fn read_root_dir(&self) -> Result<Vec<Entry>> {
        #[cfg(unix)]
        {
            local::read_root_directory(&self.directory, &self.path).map(
                |entries| {
                    entries
                        .into_iter()
                        .map(|(name, status)| {
                            Entry::new(name, Metadata::from_stat(&status))
                        })
                        .collect()
                },
            )
        }
        #[cfg(not(unix))]
        {
            Err(Error::new(
                ErrorKind::Unsupported,
                "descriptor-relative local roots are unsupported on this platform",
            ))
        }
    }

    /// Lists immediate children of a descendant directory.
    ///
    /// # Errors
    /// Returns an I/O error when traversal cannot remain beneath the opened
    /// root or the directory cannot be enumerated.
    pub fn read_dir(&self, path: &path::Path) -> Result<Vec<Entry>> {
        #[cfg(unix)]
        {
            local::read_rooted_directory(&self.directory, &self.path, path).map(
                |entries| {
                    entries
                        .into_iter()
                        .map(|(name, status)| {
                            Entry::new(name, Metadata::from_stat(&status))
                        })
                        .collect()
                },
            )
        }
        #[cfg(not(unix))]
        {
            let _ = path;
            Err(Error::new(
                ErrorKind::Unsupported,
                "descriptor-relative local roots are unsupported on this platform",
            ))
        }
    }

    /// Creates a descendant directory.
    ///
    /// # Errors
    /// Returns an I/O error when secure traversal or creation fails.
    pub fn create_dir(
        &self,
        path: &path::Path,
        recursive: bool,
        exists_ok: bool,
    ) -> Result<()> {
        #[cfg(unix)]
        {
            local::create_rooted_directory(
                &self.directory,
                &self.path,
                path,
                recursive,
                exists_ok,
            )
        }
        #[cfg(not(unix))]
        {
            let _ = (path, recursive, exists_ok);
            Err(Error::new(
                ErrorKind::Unsupported,
                "descriptor-relative local roots are unsupported on this platform",
            ))
        }
    }

    /// Removes a descendant entry without following symbolic links.
    ///
    /// # Errors
    /// Returns an I/O error when secure traversal or removal fails.
    pub fn remove(&self, path: &path::Path, recursive: bool) -> Result<()> {
        #[cfg(unix)]
        {
            local::remove_rooted_entry(
                &self.directory,
                &self.path,
                path,
                recursive,
            )
        }
        #[cfg(not(unix))]
        {
            let _ = (path, recursive);
            Err(Error::new(
                ErrorKind::Unsupported,
                "descriptor-relative local roots are unsupported on this platform",
            ))
        }
    }

    /// Renames a descendant entry within the same opened root.
    ///
    /// # Errors
    /// Returns an I/O error when secure traversal or the requested atomic
    /// rename fails.
    pub fn rename(
        &self,
        source: &path::Path,
        destination: &path::Path,
        overwrite: bool,
    ) -> Result<()> {
        #[cfg(unix)]
        {
            local::rename_rooted_entry(
                &self.directory,
                &self.path,
                source,
                destination,
                overwrite,
            )
        }
        #[cfg(not(unix))]
        {
            let _ = (source, destination, overwrite);
            Err(Error::new(
                ErrorKind::Unsupported,
                "descriptor-relative local roots are unsupported on this platform",
            ))
        }
    }

    /// Applies portable Unix permission bits to a descendant entry.
    ///
    /// # Errors
    /// Returns an I/O error when traversal cannot remain beneath the opened
    /// root or the permission update fails.
    pub fn set_permissions(&self, path: &path::Path, mode: u32) -> Result<()> {
        #[cfg(unix)]
        {
            local::set_rooted_permissions(
                &self.directory,
                &self.path,
                path,
                mode,
            )
        }
        #[cfg(not(unix))]
        {
            let _ = (path, mode);
            Err(Error::new(
                ErrorKind::Unsupported,
                "descriptor-relative local roots are unsupported on this platform",
            ))
        }
    }

    /// Opens a regular native file for reading beneath this root.
    ///
    /// # Errors
    /// Returns an I/O error when traversal escapes through a link or the file
    /// cannot be opened.
    pub fn open_reader(
        &self,
        path: &path::Path,
        options: &read::OpenOptions,
    ) -> Result<File> {
        #[cfg(unix)]
        {
            local::open_rooted_native_reader(
                &self.directory,
                &self.path,
                path,
                options,
            )
        }
        #[cfg(not(unix))]
        {
            let _ = (path, options);
            Err(Error::new(
                ErrorKind::Unsupported,
                "descriptor-relative local roots are unsupported on this platform",
            ))
        }
    }

    /// Opens a regular native file for writing beneath this root.
    ///
    /// # Errors
    /// Returns an I/O error when traversal escapes through a link or the file
    /// cannot be opened with the requested mode.
    pub fn open_writer(
        &self,
        path: &path::Path,
        options: &write::OpenOptions,
    ) -> Result<File> {
        #[cfg(unix)]
        {
            local::open_rooted_native_writer(
                &self.directory,
                &self.path,
                path,
                options,
            )
        }
        #[cfg(not(unix))]
        {
            let _ = (path, options);
            Err(Error::new(
                ErrorKind::Unsupported,
                "descriptor-relative local roots are unsupported on this platform",
            ))
        }
    }

    /// Begins a descriptor-relative atomic replacement and creates missing
    /// parent directories.
    ///
    /// # Parameters
    ///
    /// * `path` - Validated non-empty relative destination beneath this root.
    ///
    /// # Returns
    /// A staging writer that publishes only when committed.
    ///
    /// # Errors
    /// Returns a structured atomic-write error when parent preparation or
    /// staging-file creation fails.
    #[inline]
    pub fn begin_atomic_write(
        &self,
        path: &path::Path,
    ) -> std::result::Result<Writer, atomic::Error> {
        self.begin_atomic_write_with_options(
            path,
            atomic::Options::new().with_parent(),
        )
    }

    /// Begins a descriptor-relative atomic replacement with explicit options.
    ///
    /// # Parameters
    ///
    /// * `path` - Validated non-empty relative destination beneath this root.
    /// * `options` - Parent creation and destination-open retry policy.
    ///
    /// # Returns
    /// A staging writer that publishes only when committed.
    ///
    /// # Errors
    /// Returns a structured atomic-write error when parent preparation,
    /// destination inspection, or staging-file creation fails.
    pub fn begin_atomic_write_with_options(
        &self,
        path: &path::Path,
        options: atomic::Options,
    ) -> std::result::Result<Writer, atomic::Error> {
        #[cfg(unix)]
        {
            Writer::new(&self.directory, &self.path, path, options)
        }
        #[cfg(not(unix))]
        {
            let _ = options;
            Err(atomic::Error::new(
                LocalAtomicWriteStage::PrepareParent,
                path.as_path().to_path_buf(),
                None,
                LocalAtomicDestinationState::Unchanged,
                Error::new(
                    ErrorKind::Unsupported,
                    "descriptor-relative local roots are unsupported on this platform",
                ),
            ))
        }
    }
}
