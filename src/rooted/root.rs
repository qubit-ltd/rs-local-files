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
#[cfg(not(any(unix, windows)))]
use std::io::{Error, ErrorKind};
use std::path::{Path, PathBuf};

use crate::copy;
#[cfg(any(unix, windows))]
use crate::local;
#[cfg(not(any(unix, windows)))]
use crate::{LocalAtomicDestinationState, LocalAtomicWriteStage};
use crate::{atomic, read, write};

use super::path;
use super::{Entry, Metadata, Permissions, Writer};

/// An opened directory descriptor that authorizes contained operations.
#[must_use]
#[derive(Debug)]
pub struct Root {
    /// Absolute path retained only for diagnostics.
    path: PathBuf,
    /// Open descriptor used as the sole descendant authority.
    #[cfg(any(unix, windows))]
    directory: File,
}

impl Root {
    /// Opens and anchors a local filesystem root.
    ///
    /// # Errors
    /// Returns an I/O error when the directory cannot be securely opened.
    pub fn open(path: &Path) -> Result<Self> {
        let path = std::path::absolute(path)?;
        #[cfg(any(unix, windows))]
        {
            let directory = local::open_root_directory(&path)?;
            Ok(Self { path, directory })
        }
        #[cfg(not(any(unix, windows)))]
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

    /// Copies one descendant entry beneath this opened root.
    ///
    /// # Parameters
    ///
    /// * `source` - Existing rooted source entry.
    /// * `destination` - Rooted destination entry beneath the same root.
    /// * `options` - Explicit copy policies.
    ///
    /// # Returns
    ///
    /// Exact statistics accumulated by the completed copy.
    ///
    /// # Errors
    ///
    /// Returns a structured copy error when the source is unsupported,
    /// destination policies reject an entry, traversal fails, or a staged file
    /// cannot be installed.
    pub fn copy(
        &self,
        source: &path::Path,
        destination: &path::Path,
        options: copy::Options,
    ) -> std::result::Result<copy::Statistics, copy::Error> {
        super::copy::copy(self, source, destination, options)
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
        #[cfg(windows)]
        {
            Metadata::from_open_file(&self.directory)
        }
        #[cfg(not(any(unix, windows)))]
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
            local::read_rooted_symlink_metadata(&self.directory, &self.path, path)
                .map(|status| Metadata::from_stat(&status))
        }
        #[cfg(windows)]
        {
            local::read_rooted_symlink_metadata(&self.directory, &self.path, path)
                .and_then(|file| Metadata::from_open_file(&file))
        }
        #[cfg(not(any(unix, windows)))]
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
            local::read_root_directory(&self.directory, &self.path).map(|entries| {
                entries
                    .into_iter()
                    .map(|(name, status)| Entry::new(name, Metadata::from_stat(&status)))
                    .collect()
            })
        }
        #[cfg(windows)]
        {
            local::read_root_directory(&self.directory, &self.path)?
                .into_iter()
                .map(|(name, file)| {
                    Metadata::from_open_file(&file).map(|metadata| Entry::new(name, metadata))
                })
                .collect()
        }
        #[cfg(not(any(unix, windows)))]
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
            local::read_rooted_directory(&self.directory, &self.path, path).map(|entries| {
                entries
                    .into_iter()
                    .map(|(name, status)| Entry::new(name, Metadata::from_stat(&status)))
                    .collect()
            })
        }
        #[cfg(windows)]
        {
            local::read_rooted_directory(&self.directory, &self.path, path)?
                .into_iter()
                .map(|(name, file)| {
                    Metadata::from_open_file(&file).map(|metadata| Entry::new(name, metadata))
                })
                .collect()
        }
        #[cfg(not(any(unix, windows)))]
        {
            let _ = path;
            Err(Error::new(
                ErrorKind::Unsupported,
                "descriptor-relative local roots are unsupported on this platform",
            ))
        }
    }

    /// Creates one descendant directory.
    ///
    /// # Errors
    /// Returns an I/O error when secure traversal or creation fails, including
    /// when the parent is missing or the destination already exists.
    pub fn create_dir(&self, path: &path::Path) -> Result<()> {
        #[cfg(any(unix, windows))]
        {
            local::create_rooted_directory(&self.directory, &self.path, path, false, false)
        }
        #[cfg(not(any(unix, windows)))]
        {
            let _ = path;
            Err(Error::new(
                ErrorKind::Unsupported,
                "descriptor-relative local roots are unsupported on this platform",
            ))
        }
    }

    /// Creates a descendant directory and any missing parents.
    ///
    /// Existing directories are accepted, matching [`std::fs::create_dir_all`].
    ///
    /// # Errors
    /// Returns an I/O error when secure traversal or creation fails.
    pub fn create_dir_all(&self, path: &path::Path) -> Result<()> {
        #[cfg(any(unix, windows))]
        {
            local::create_rooted_directory(&self.directory, &self.path, path, true, true)
        }
        #[cfg(not(any(unix, windows)))]
        {
            let _ = path;
            Err(Error::new(
                ErrorKind::Unsupported,
                "descriptor-relative local roots are unsupported on this platform",
            ))
        }
    }

    /// Ensures one descendant directory exists without creating parents.
    ///
    /// # Errors
    /// Returns an I/O error when secure traversal fails or an existing entry
    /// is not a directory.
    pub fn ensure_dir(&self, path: &path::Path) -> Result<()> {
        #[cfg(any(unix, windows))]
        {
            local::create_rooted_directory(&self.directory, &self.path, path, false, true)
        }
        #[cfg(not(any(unix, windows)))]
        {
            let _ = path;
            Err(Error::new(
                ErrorKind::Unsupported,
                "descriptor-relative local roots are unsupported on this platform",
            ))
        }
    }

    /// Ensures a descendant directory and all of its parents exist.
    ///
    /// # Errors
    /// Returns an I/O error when secure traversal fails or an existing entry
    /// in the chain is not a directory.
    #[inline]
    pub fn ensure_dir_all(&self, path: &path::Path) -> Result<()> {
        self.create_dir_all(path)
    }

    /// Removes one descendant regular file or symbolic link.
    ///
    /// # Errors
    /// Returns an I/O error when secure traversal or removal fails.
    pub fn remove_file(&self, path: &path::Path) -> Result<()> {
        #[cfg(any(unix, windows))]
        {
            if self.symlink_metadata(path)?.kind() == super::EntryKind::Directory {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::IsADirectory,
                    "rooted remove_file does not remove directories",
                ));
            }
            local::remove_rooted_entry(&self.directory, &self.path, path, false)
        }
        #[cfg(not(any(unix, windows)))]
        {
            let _ = path;
            Err(Error::new(
                ErrorKind::Unsupported,
                "descriptor-relative local roots are unsupported on this platform",
            ))
        }
    }

    /// Removes one empty descendant directory.
    ///
    /// # Errors
    /// Returns an I/O error when secure traversal or removal fails.
    pub fn remove_empty_dir(&self, path: &path::Path) -> Result<()> {
        #[cfg(any(unix, windows))]
        {
            if self.symlink_metadata(path)?.kind() != super::EntryKind::Directory {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::NotADirectory,
                    "rooted remove_empty_dir requires a directory",
                ));
            }
            local::remove_rooted_entry(&self.directory, &self.path, path, false)
        }
        #[cfg(not(any(unix, windows)))]
        {
            let _ = path;
            Err(Error::new(
                ErrorKind::Unsupported,
                "descriptor-relative local roots are unsupported on this platform",
            ))
        }
    }

    /// Removes a descendant directory tree without following symbolic links.
    ///
    /// # Errors
    /// Returns an I/O error when secure traversal or removal fails.
    pub fn remove_tree(&self, path: &path::Path) -> Result<()> {
        #[cfg(any(unix, windows))]
        {
            if self.symlink_metadata(path)?.kind() != super::EntryKind::Directory {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::NotADirectory,
                    "rooted remove_tree requires a directory",
                ));
            }
            local::remove_rooted_entry(&self.directory, &self.path, path, true)
        }
        #[cfg(not(any(unix, windows)))]
        {
            let _ = path;
            Err(Error::new(
                ErrorKind::Unsupported,
                "descriptor-relative local roots are unsupported on this platform",
            ))
        }
    }

    /// Renames a descendant entry, replacing an existing destination.
    ///
    /// # Errors
    /// Returns an I/O error when secure traversal or the requested atomic
    /// rename fails.
    pub fn rename(&self, source: &path::Path, destination: &path::Path) -> Result<()> {
        #[cfg(any(unix, windows))]
        {
            local::rename_rooted_entry(&self.directory, &self.path, source, destination, true)
        }
        #[cfg(not(any(unix, windows)))]
        {
            let _ = (source, destination);
            Err(Error::new(
                ErrorKind::Unsupported,
                "descriptor-relative local roots are unsupported on this platform",
            ))
        }
    }

    /// Renames a descendant entry without replacing an existing destination.
    ///
    /// # Errors
    /// Returns an I/O error when secure traversal fails, the destination
    /// exists, or the requested atomic rename is unavailable.
    pub fn rename_without_replacing(
        &self,
        source: &path::Path,
        destination: &path::Path,
    ) -> Result<()> {
        #[cfg(any(unix, windows))]
        {
            local::rename_rooted_entry(&self.directory, &self.path, source, destination, false)
        }
        #[cfg(not(any(unix, windows)))]
        {
            let _ = (source, destination);
            Err(Error::new(
                ErrorKind::Unsupported,
                "descriptor-relative local roots are unsupported on this platform",
            ))
        }
    }

    /// Applies cross-platform permissions to a descendant entry.
    ///
    /// # Errors
    /// Returns an I/O error when traversal cannot remain beneath the opened
    /// root or the permission update fails.
    pub fn set_permissions(&self, path: &path::Path, permissions: Permissions) -> Result<()> {
        #[cfg(unix)]
        {
            let current_mode = self
                .symlink_metadata(path)?
                .permissions()
                .unix_mode()
                .expect("Unix rooted metadata always carries a mode");
            let mode = permissions.resolve_unix_mode(current_mode);
            local::set_rooted_permissions(&self.directory, &self.path, path, mode)
        }
        #[cfg(windows)]
        {
            let mode = if permissions.is_read_only() { 0 } else { 0o200 };
            local::set_rooted_permissions(&self.directory, &self.path, path, mode)
        }
        #[cfg(not(any(unix, windows)))]
        {
            let _ = (path, permissions);
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
    pub fn open_reader(&self, path: &path::Path, options: &read::OpenOptions) -> Result<File> {
        #[cfg(any(unix, windows))]
        {
            local::open_rooted_native_reader(&self.directory, &self.path, path, options)
        }
        #[cfg(not(any(unix, windows)))]
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
    pub fn open_writer(&self, path: &path::Path, options: &write::OpenOptions) -> Result<File> {
        #[cfg(any(unix, windows))]
        {
            local::open_rooted_native_writer(&self.directory, &self.path, path, options)
        }
        #[cfg(not(any(unix, windows)))]
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
        self.begin_atomic_write_with_options(path, atomic::Options::new().with_parent())
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
        #[cfg(any(unix, windows))]
        {
            Writer::new(&self.directory, &self.path, path, options)
        }
        #[cfg(not(any(unix, windows)))]
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

    /// Synchronizes the parent directory of a rooted descendant.
    ///
    /// # Errors
    ///
    /// Returns an I/O error when secure parent traversal or directory
    /// synchronization is unavailable or fails.
    pub(crate) fn sync_parent(&self, path: &path::Path) -> Result<()> {
        #[cfg(coverage)]
        if local::coverage_fault_enabled("rooted-copy-parent-sync")
            || local::coverage_fault_enabled("rooted-rename-parent-sync")
        {
            return Err(std::io::Error::from_raw_os_error(libc::EIO));
        }
        #[cfg(unix)]
        {
            local::sync_rooted_parent(&self.directory, &self.path, path)
        }
        #[cfg(not(unix))]
        {
            let _ = path;
            Err(Error::new(
                ErrorKind::Unsupported,
                "rooted directory durability is unsupported on this platform",
            ))
        }
    }
}
