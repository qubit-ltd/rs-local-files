// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Open directory capabilities.
// qubit-style: allow source-test-pair

use std::fs::File;
use std::io::Error;
use std::io::ErrorKind;
use std::io::Result;
use std::path::Path;
use std::path::PathBuf;

use super::DirectoryReader;
use super::Entry;
use super::EntryKind;
use super::Metadata;
use super::Permissions;
use super::Writer;
use super::path;
#[cfg(not(any(unix, windows)))]
use crate::LocalAtomicDestinationState;
use crate::LocalAtomicWriteError;
use crate::LocalAtomicWriteOptions;
#[cfg(not(any(unix, windows)))]
use crate::LocalAtomicWriteStage;
use crate::LocalCopyDirError;
use crate::LocalCopyDirOptions;
use crate::LocalCopyDirStats;
use crate::LocalDurabilityRequirement;
#[cfg(any(unix, windows))]
use crate::local;
use crate::read;
use crate::write;

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
    // qubit-style: allow coverage-cfg
    #[cfg_attr(not(coverage), inline(always))]
    #[cfg_attr(coverage, inline(never))]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Clones the opened directory authority for a best-effort observation.
    ///
    /// # Errors
    ///
    /// Returns an I/O error when the operating system cannot duplicate the
    /// already-opened authority handle.
    #[cfg(any(unix, windows))]
    #[cfg_attr(not(coverage), inline)]
    #[cfg_attr(coverage, inline(never))]
    pub fn try_clone_authority(&self) -> Result<File> {
        self.directory.try_clone()
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
        #[cfg(feature = "test-support")]
        if local::take_test_support_on_nth("rooted-copy-destination-metadata-native", 2) {
            return Err(crate::local::test_fault_error());
        }
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

    /// Reads the target stored in a final symbolic-link entry.
    ///
    /// # Parameters
    ///
    /// * `path` - Validated rooted path naming the symbolic link.
    ///
    /// # Returns
    ///
    /// The link target exactly as stored in the directory entry.
    ///
    /// # Errors
    ///
    /// Returns an I/O error when the final entry is not a readable symbolic
    /// link or descriptor-relative traversal fails.
    pub fn read_link(&self, path: &path::Path) -> Result<PathBuf> {
        #[cfg(any(unix, windows))]
        {
            local::read_rooted_link(&self.directory, &self.path, path)
        }
        #[cfg(not(any(unix, windows)))]
        {
            let _ = path;
            Err(Error::new(
                ErrorKind::Unsupported,
                "symbolic links are unsupported on this platform",
            ))
        }
    }

    /// Creates a symbolic link while retaining publication and rollback facts.
    pub(super) fn create_symlink_for_copy(
        &self,
        target: &Path,
        path: &path::Path,
        targets_directory: bool,
    ) -> std::result::Result<(), local::RootedSymlinkCreateError> {
        #[cfg(feature = "test-support")]
        if local::take_test_support("rooted-copy-symlink-create") {
            return Err(local::RootedSymlinkCreateError::new(
                local::RootedSymlinkCreateFailureState::Unchanged,
                local::test_fault_error(),
                None,
            ));
        }
        #[cfg(unix)]
        {
            let _ = targets_directory;
            local::create_rooted_symlink(&self.directory, &self.path, target, path).map_err(|primary| {
                local::RootedSymlinkCreateError::new(local::RootedSymlinkCreateFailureState::Unchanged, primary, None)
            })
        }
        #[cfg(windows)]
        {
            local::create_rooted_symlink(&self.directory, &self.path, target, path, targets_directory)
        }
        #[cfg(not(any(unix, windows)))]
        {
            let _ = (target, path, targets_directory);
            Err(local::RootedSymlinkCreateError::new(
                local::RootedSymlinkCreateFailureState::Unchanged,
                Error::new(
                    ErrorKind::Unsupported,
                    "symbolic links are unsupported on this platform",
                ),
                None,
            ))
        }
    }

    /// Reports whether a symbolic-link entry was created as a directory link.
    ///
    /// This inspects only the final link handle and never dereferences its
    /// target, so dangling and external links remain classifiable.
    ///
    /// # Errors
    ///
    /// Returns an I/O error when the final link cannot be opened or inspected.
    #[cfg(windows)]
    #[must_use = "inspect the symbolic-link target kind"]
    pub fn symlink_targets_directory(&self, path: &path::Path) -> Result<bool> {
        local::rooted_link_targets_directory(&self.directory, path)
    }

    /// Opens a lazy reader for immediate children of this root directory.
    ///
    /// # Errors
    ///
    /// Returns an I/O error when the root descriptor cannot be enumerated.
    pub fn open_root_dir_reader(&self) -> Result<DirectoryReader> {
        DirectoryReader::open_root(&self.directory, &self.path)
    }

    /// Lists immediate children of a descendant directory.
    ///
    /// # Errors
    /// Returns an I/O error when traversal cannot remain beneath the opened
    /// root or the directory cannot be enumerated.
    pub fn read_dir(&self, path: &path::Path) -> Result<Vec<Entry>> {
        #[cfg(feature = "test-support")]
        if local::test_support_enabled("rooted-copy-directory-read-native") {
            return Err(crate::local::test_fault_error());
        }
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
                .map(|(name, file)| Metadata::from_open_file(&file).map(|metadata| Entry::new(name, metadata)))
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

    /// Opens a lazy reader for immediate children of a descendant directory.
    ///
    /// # Errors
    ///
    /// Returns an I/O error when secure traversal or directory enumeration
    /// cannot remain beneath this opened root.
    pub fn open_dir_reader(&self, path: &path::Path) -> Result<DirectoryReader> {
        DirectoryReader::open_descendant(&self.directory, &self.path, path)
    }

    /// Opens a regular file or directory for filesystem capability probing.
    ///
    /// The returned handle remains relative to this opened root authority, so
    /// probing a descendant does not fall back to the diagnostic path.
    #[cfg(any(unix, windows))]
    pub fn open_probe_file(&self, path: &path::Path) -> Result<File> {
        if path.as_path().as_os_str().is_empty() {
            return self.try_clone_authority();
        }
        match self.symlink_metadata(path)?.kind() {
            EntryKind::Directory => self.open_dir_reader(path)?.try_clone_directory(),
            EntryKind::File => self.open_reader(path, &read::OpenOptions::default()),
            _ => Err(Error::new(
                ErrorKind::InvalidInput,
                "capability probing requires a regular file or directory",
            )),
        }
    }

    /// Duplicates the root authority for capability probing.
    #[cfg(any(unix, windows))]
    pub fn open_probe_root(&self) -> Result<File> {
        self.try_clone_authority()
    }

    /// Reports unsupported capability probing on platforms without rooted
    /// descriptor primitives.
    #[cfg(not(any(unix, windows)))]
    pub fn open_probe_file(&self, _path: &path::Path) -> Result<File> {
        Err(Error::new(
            ErrorKind::Unsupported,
            "rooted capability probing is unsupported on this platform",
        ))
    }

    /// Reports unsupported capability probing on platforms without rooted
    /// descriptor primitives.
    #[cfg(not(any(unix, windows)))]
    pub fn open_probe_root(&self) -> Result<File> {
        Err(Error::new(
            ErrorKind::Unsupported,
            "rooted capability probing is unsupported on this platform",
        ))
    }

    /// Creates one descendant directory.
    ///
    /// # Errors
    /// Returns an I/O error when secure traversal or creation fails, including
    /// when the parent is missing or the destination already exists.
    pub fn create_dir(&self, path: &path::Path) -> Result<()> {
        #[cfg(feature = "test-support")]
        if local::test_support_enabled("rooted-copy-directory-create-native") {
            return Err(crate::local::test_fault_error());
        }
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
            #[cfg(feature = "test-support")]
            if local::test_support_enabled("rooted-copy-remove-file-native") {
                return Err(crate::local::test_fault_error());
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
            #[cfg(feature = "test-support")]
            if local::test_support_enabled("rooted-copy-remove-tree-native") {
                return Err(crate::local::test_fault_error());
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
    pub fn rename_without_replacing(&self, source: &path::Path, destination: &path::Path) -> Result<()> {
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
            #[cfg(feature = "test-support")]
            if local::test_support_enabled("rooted-copy-set-permissions-native") {
                return Err(crate::local::test_fault_error());
            }
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
        #[cfg(feature = "test-support")]
        if local::test_support_enabled("rooted-copy-source-open-native") {
            return Err(crate::local::test_fault_error());
        }
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
        options: LocalAtomicWriteOptions,
    ) -> std::result::Result<Writer, LocalAtomicWriteError> {
        #[cfg(any(unix, windows))]
        {
            Writer::new(&self.directory, &self.path, path, options)
        }
        #[cfg(not(any(unix, windows)))]
        {
            let _ = options;
            Err(LocalAtomicWriteError::new(
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

    /// Copies one descendant entry with an explicit synchronization policy.
    ///
    /// # Parameters
    ///
    /// * `source` - Existing rooted source entry.
    /// * `destination` - Rooted destination entry beneath the same root.
    /// * `options` - Explicit copy policies.
    /// * `durability` - Synchronization policy applied to staged file commits.
    ///
    /// # Returns
    ///
    /// Exact statistics accumulated by the completed copy.
    ///
    /// # Errors
    ///
    /// Returns a structured copy error when the source is unsupported,
    /// destination policies reject an entry, traversal fails, staging cannot
    /// be installed, or required synchronization fails.
    #[cfg_attr(not(coverage), inline(always))]
    #[cfg_attr(coverage, inline(never))]
    pub fn copy_with_durability(
        &self,
        source: &path::Path,
        destination: &path::Path,
        options: LocalCopyDirOptions,
        durability: LocalDurabilityRequirement,
    ) -> std::result::Result<LocalCopyDirStats, LocalCopyDirError> {
        super::copy::copy(self, source, destination, options, durability)
    }

    /// Synchronizes the parent directory of a rooted descendant.
    ///
    /// # Errors
    ///
    /// Returns an I/O error when secure parent traversal or directory
    /// synchronization is unavailable or fails.
    pub fn sync_parent(&self, path: &path::Path) -> Result<()> {
        #[cfg(feature = "test-support")]
        if local::test_support_enabled("rooted-copy-parent-sync")
            || local::test_support_enabled("rooted-rename-parent-sync")
        {
            return Err(crate::local::test_fault_error());
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
