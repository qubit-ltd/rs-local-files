// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Open directory capabilities.

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
use super::RootedResolutionCursor;
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
use crate::LocalFileError;
use crate::LocalFileOperation;
#[cfg(any(unix, windows))]
use crate::local;
use crate::read;
use crate::write;

/// An opened directory descriptor that authorizes contained operations.
///
/// # Examples
///
/// ```ignore
/// use qubit_local_files::rooted::Root;
///
/// let directory = tempfile::tempdir().expect("temporary directory should be created");
/// let root = Root::open(directory.path()).expect("directory should be opened as a root");
/// assert_eq!(root.path(), directory.path());
/// ```
#[must_use]
pub struct Root {
    /// Absolute path retained only for diagnostics.
    path: PathBuf,
    /// Open descriptor used as the sole descendant authority.
    #[cfg(any(unix, windows))]
    directory: File,
    /// Resolution contract inherited by every operation using this authority.
    contract: crate::resolution::ResolutionContract,
}
impl std::fmt::Debug for Root {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("Root")
            .field("path", &self.path)
            .finish_non_exhaustive()
    }
}
impl Root {
    /// Builds a root from an already-open directory authority.
    #[allow(dead_code)] // Retained as the unrestricted-authority constructor for internal callers.
    pub(crate) fn from_directory_authority(directory: File, diagnostic_path: PathBuf) -> Self {
        Self {
            path: diagnostic_path,
            directory,
            contract: crate::resolution::ResolutionContract::unrestricted(),
        }
    }

    pub(crate) fn from_directory_authority_with_contract(
        directory: File,
        diagnostic_path: PathBuf,
        contract: crate::resolution::ResolutionContract,
    ) -> Self {
        Self {
            path: diagnostic_path,
            directory,
            contract,
        }
    }

    pub(crate) fn resolution_contract(&self) -> &crate::resolution::ResolutionContract {
        &self.contract
    }

    pub(crate) fn with_resolution_contract(mut self, contract: crate::resolution::ResolutionContract) -> Self {
        self.contract = contract;
        self
    }

    /// Opens an existing descendant without following its final reparse point,
    /// with the rights needed to update its Windows security descriptor.
    #[cfg(windows)]
    pub(crate) fn open_existing_file_for_security(&self, path: &Path) -> Result<File> {
        let relative = crate::local::LocalRelativePath::new(path)?;
        crate::temp::internal::privacy_windows::open_existing_relative(&self.directory, relative.as_path(), false)
    }

    /// Opens and anchors a local filesystem root.
    ///
    /// # Errors
    /// Returns an I/O error when the directory cannot be securely opened.
    #[allow(dead_code)] // Public standalone root-opening API; current crate paths use rooted options.
    pub fn open(path: &Path) -> Result<Self> {
        #[cfg(test)]
        crate::tests::native_io_fault_tests::check(crate::tests::native_io_fault_tests::NativeIoPoint::RootOpen)?;
        let path = std::path::absolute(path)?;
        #[cfg(any(unix, windows))]
        {
            let directory = local::open_root_directory(&path)?;
            Ok(Self {
                path,
                directory,
                contract: crate::resolution::ResolutionContract::unrestricted(),
            })
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
    #[inline]
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
    #[inline]
    pub fn try_clone_authority(&self) -> Result<File> {
        #[cfg(test)]
        crate::tests::native_io_fault_tests::check(crate::tests::native_io_fault_tests::NativeIoPoint::CloneAuthority)?;
        self.directory.try_clone()
    }

    /// Starts a component-at-a-time observation from this rooted authority.
    ///
    /// The cursor is an internal optimization used by rooted path resolution.
    /// It retains only one directory handle at a time and never changes the
    /// authority represented by this root.
    /// Returns a native handle-duplication error, or `Unsupported` when rooted
    /// descriptor operations are unavailable.
    pub(crate) fn resolution_cursor(&self) -> Result<RootedResolutionCursor> {
        #[cfg(test)]
        crate::tests::native_io_fault_tests::check(crate::tests::native_io_fault_tests::NativeIoPoint::OpenCursor)?;
        #[cfg(any(unix, windows))]
        {
            RootedResolutionCursor::new(self.directory.try_clone()?)
        }
        #[cfg(not(any(unix, windows)))]
        {
            Err(Error::new(
                ErrorKind::Unsupported,
                "descriptor-relative rooted resolution is unsupported on this platform",
            ))
        }
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
        #[cfg(test)]
        crate::tests::native_io_fault_tests::check(crate::tests::native_io_fault_tests::NativeIoPoint::RootMetadata)?;
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
        #[cfg(test)]
        crate::tests::native_io_fault_tests::check(crate::tests::native_io_fault_tests::NativeIoPoint::EntryMetadata)?;
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
        #[cfg(unix)]
        {
            local::read_rooted_link(&self.directory, &self.path, path, None).and_then(|(target, _)| {
                target.ok_or_else(|| Error::new(ErrorKind::InvalidData, "unbounded link read was truncated"))
            })
        }
        #[cfg(windows)]
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

    pub(crate) fn read_link_limited(
        &self,
        path: &path::Path,
        maximum_bytes: Option<usize>,
        validate_raw_target: impl FnOnce(&std::path::Path) -> crate::LocalResult<()>,
    ) -> crate::LocalResult<PathBuf> {
        #[cfg(unix)]
        {
            let (target, observed) = local::read_rooted_link(&self.directory, &self.path, path, maximum_bytes)
                .map_err(|source| {
                    crate::LocalFileError::from_io(
                        crate::LocalFileOperation::Metadata,
                        Some(path.as_path().to_path_buf()),
                        None,
                        source,
                    )
                })?;
            let Some(target) = target else {
                let limit = maximum_bytes.unwrap_or(usize::MAX);
                let facts = crate::error::LocalResourceLimitError::new(
                    crate::error::LocalResourceKind::SymlinkTargetBytes,
                    limit,
                    0,
                    1,
                )
                .with_observed(observed);
                return Err(crate::LocalFileError::from_resource_limit(
                    crate::LocalFileOperation::Metadata,
                    Some(path.as_path().to_path_buf()),
                    facts,
                ));
            };
            validate_raw_target(&target)?;
            Ok(target)
        }
        #[cfg(windows)]
        {
            match local::read_rooted_link_limited(&self.directory, &self.path, path, maximum_bytes).map_err(
                |source| {
                    crate::LocalFileError::from_io(
                        crate::LocalFileOperation::Metadata,
                        Some(path.as_path().to_path_buf()),
                        None,
                        source,
                    )
                },
            )? {
                local::RootedLinkTargetRead::Target(target) => {
                    validate_raw_target(&target.raw_path)?;
                    local::normalize_rooted_link_target(&target.raw_path, target.flags).map_err(|source| {
                        crate::LocalFileError::from_io(
                            crate::LocalFileOperation::Metadata,
                            Some(path.as_path().to_path_buf()),
                            None,
                            source,
                        )
                    })
                }
                local::RootedLinkTargetRead::Exceeded(observed) => {
                    let limit = maximum_bytes.unwrap_or(usize::MAX);
                    let facts = crate::error::LocalResourceLimitError::new(
                        crate::error::LocalResourceKind::SymlinkTargetBytes,
                        limit,
                        0,
                        1,
                    )
                    .with_observed(observed);
                    Err(crate::LocalFileError::from_resource_limit(
                        crate::LocalFileOperation::Metadata,
                        Some(path.as_path().to_path_buf()),
                        facts,
                    ))
                }
            }
        }
        #[cfg(not(any(unix, windows)))]
        {
            let _ = (path, maximum_bytes, validate_raw_target);
            Err(crate::LocalFileError::new(
                crate::LocalFileErrorKind::Unsupported,
                crate::LocalFileOperation::Metadata,
            ))
        }
    }

    /// Creates a symbolic link while retaining publication and rollback facts.
    ///
    /// Stores `target` without dereferencing it at the validated rooted `path`.
    /// `targets_directory` selects the Windows link kind and is ignored on
    /// Unix. Native failures retain the primary error, destination certainty,
    /// and any failure to remove a partially created Windows placeholder.
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
    #[allow(dead_code)]
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
    /// Returns `InvalidInput` for other entry kinds, or a native inspection,
    /// traversal, open, or handle-duplication error.
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
    /// Propagates the native handle-duplication error.
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
    /// Successfully created ancestors remain if a later operation fails.
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
            #[cfg(all(any(unix, windows), feature = "test-support"))]
            crate::test_support::temp_cleanup_replace_observed_entry(&self.directory, &self.path, path, false)?;
            #[cfg(feature = "test-support")]
            if local::test_support_enabled("rooted-copy-remove-file-native") {
                return Err(crate::local::test_fault_error());
            }
            local::remove_rooted_entry_typed(&self.directory, &self.path, path, false)
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

    /// Removes an owned regular file through its captured rooted identity.
    /// Windows uses the retained parent authority and ignores the read-only
    /// attribute only for this cleanup-owned entry.
    #[cfg(windows)]
    pub(crate) fn remove_owned_read_only_file(&self, path: &path::Path, expected: &Metadata) -> Result<()> {
        local::remove_rooted_owned_read_only_file(&self.directory, path, expected)
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
            #[cfg(all(any(unix, windows), feature = "test-support"))]
            crate::test_support::temp_cleanup_replace_observed_entry(&self.directory, &self.path, path, true)?;
            local::remove_rooted_entry_typed(&self.directory, &self.path, path, true)
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

    /// Removes an entry already classified as a non-directory by the scheduler.
    /// Unix performs no duplicate metadata lookup: unlinkat rejects a current
    /// directory and never follows a final link. Other platforms retain their
    /// existing checked removal path. Secure parent traversal and native
    /// removal errors propagate; this does not grant authority over a changed
    /// intermediate symbolic link or switch between leaf and directory removal.
    pub(crate) fn remove_observed_non_directory(&self, path: &path::Path) -> Result<()> {
        #[cfg(unix)]
        {
            #[cfg(feature = "test-support")]
            {
                if local::test_support_enabled("rooted-copy-remove-file-native") {
                    return Err(crate::local::test_fault_error());
                }
                crate::test_support::temp_cleanup_replace_observed_entry(&self.directory, &self.path, path, false)?;
            }
            local::unlink_rooted_entry(&self.directory, &self.path, path, false)
        }
        #[cfg(not(unix))]
        {
            self.remove_file(path)
        }
    }

    /// Removes a directory whose children were processed by the scheduler.
    /// Unix fixes AT_REMOVEDIR rather than reclassifying the current entry:
    /// replacement leaves and symbolic links are rejected, while concurrent
    /// children produce DirectoryNotEmpty without a rescan. Other platforms
    /// retain their checked removal path. All native traversal/removal errors
    /// propagate without rolling back earlier removals.
    pub(crate) fn remove_observed_empty_directory(&self, path: &path::Path) -> Result<()> {
        #[cfg(unix)]
        {
            #[cfg(feature = "test-support")]
            crate::test_support::temp_cleanup_replace_observed_entry(&self.directory, &self.path, path, true)?;
            local::unlink_rooted_entry(&self.directory, &self.path, path, true)
        }
        #[cfg(not(unix))]
        {
            self.remove_empty_dir(path)
        }
    }

    /// Removes a descendant directory tree without following symbolic links.
    /// Removal is incremental; earlier deletions remain after a later failure.
    ///
    /// # Errors
    /// Returns an I/O error when secure traversal or removal fails.
    pub fn remove_tree(&self, path: &path::Path) -> Result<()> {
        self.remove_tree_typed(path).map_err(LocalFileError::into_io_error)
    }

    /// Removes a descendant tree while retaining the typed partial-effect
    /// state.
    pub(crate) fn remove_tree_typed(&self, path: &path::Path) -> crate::LocalResult<()> {
        self.remove_tree_typed_with_options(
            path,
            crate::LocalDeleteOptions::new().with_recursive(),
            std::time::Instant::now(),
        )
    }

    /// Removes a descendant tree using caller-supplied delete limits and the
    /// original monotonic operation start.
    ///
    /// # Errors
    /// Returns a typed local-file error when the target is not a directory,
    /// secure traversal fails, or a configured delete limit is exceeded. The
    /// error retains any deletion already completed.
    pub(crate) fn remove_tree_typed_with_options(
        &self,
        path: &path::Path,
        options: crate::LocalDeleteOptions,
        started_at: std::time::Instant,
    ) -> crate::LocalResult<()> {
        #[cfg(any(unix, windows))]
        {
            let metadata = self.symlink_metadata(path).map_err(|source| {
                LocalFileError::from_io(
                    LocalFileOperation::DeleteDirectory,
                    Some(path.as_path().to_path_buf()),
                    None,
                    source,
                )
            })?;
            if metadata.kind() != super::EntryKind::Directory {
                return Err(LocalFileError::from_io(
                    LocalFileOperation::DeleteDirectory,
                    Some(path.as_path().to_path_buf()),
                    None,
                    std::io::Error::new(
                        std::io::ErrorKind::NotADirectory,
                        "rooted remove_tree requires a directory",
                    ),
                ));
            }
            #[cfg(feature = "test-support")]
            if local::test_support_enabled("rooted-copy-remove-tree-native") {
                return Err(LocalFileError::from_io(
                    LocalFileOperation::DeleteDirectory,
                    Some(path.as_path().to_path_buf()),
                    None,
                    crate::local::test_fault_error(),
                ));
            }
            local::remove_directory_tree(
                &crate::temp::internal::TempDirectoryDeleteBackend { root: self },
                path,
                options,
                started_at,
            )
        }
        #[cfg(not(any(unix, windows)))]
        {
            let _ = (path, options, started_at);
            Err(LocalFileError::from_io(
                LocalFileOperation::DeleteDirectory,
                None,
                None,
                Error::new(
                    ErrorKind::Unsupported,
                    "descriptor-relative local roots are unsupported on this platform",
                ),
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

    /// Prepares same-directory staging for a rooted copy that will replace an
    /// incompatible destination only after the copied contents are complete.
    /// The final entry is deliberately not inspected here; the copy operation
    /// must revalidate its captured identity before removing it.
    ///
    /// # Errors
    /// Returns a structured atomic-write error if parent or staging preparation
    /// fails, or an unsupported-platform error when rooted operations are not
    /// available.
    pub(crate) fn begin_copy_replacement_staging(
        &self,
        path: &path::Path,
        options: LocalAtomicWriteOptions,
    ) -> std::result::Result<Writer, LocalAtomicWriteError> {
        #[cfg(any(unix, windows))]
        {
            Writer::new_for_copy_replacement(&self.directory, &self.path, path, options)
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
    #[inline]
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

#[cfg(test)]
mod authority_and_native_failure_tests {
    use std::fs;
    use std::path::Path;

    use super::Root;
    #[cfg(not(windows))]
    use crate::LocalFileOperation;
    use crate::local::LocalRelativePath;

    #[test]
    fn test_root_from_existing_authority_uses_descriptor_and_reports_diagnostic_path() {
        let fixture = tempfile::tempdir().expect("root fixture should exist");
        fs::write(fixture.path().join("payload"), b"descriptor authority").expect("file should exist");
        let opened = Root::open(fixture.path()).expect("root should open");
        let diagnostic = fixture.path().join("diagnostic-only");
        let root = Root::from_directory_authority(
            opened.try_clone_authority().expect("authority should clone"),
            diagnostic.clone(),
        );
        let path = LocalRelativePath::new(Path::new("payload")).expect("path should be relative");

        let metadata = root
            .symlink_metadata(&path)
            .expect("lookup must use the retained descriptor, even when diagnostic path does not exist");

        assert_eq!(metadata.size(), b"descriptor authority".len() as u64);
        assert_eq!(root.path(), diagnostic);
        assert_eq!(
            root.resolution_contract().limits(),
            crate::options::LocalResolutionLimits::new()
        );
        let debug = format!("{root:?}");
        assert!(debug.contains("Root"));
        assert!(debug.contains("diagnostic-only"));
    }

    #[cfg(unix)]
    #[test]
    fn test_root_native_errors_retain_read_link_and_recursive_delete_context() {
        let fixture = tempfile::tempdir().expect("root fixture should exist");
        fs::write(fixture.path().join("ordinary"), b"unchanged").expect("ordinary file should exist");
        let root = Root::open(fixture.path()).expect("root should open");
        let file = LocalRelativePath::new(Path::new("ordinary")).expect("path should be relative");

        let mut validations = 0;
        let mut validate = |_: &Path| {
            validations += 1;
            Ok(())
        };
        let error = root
            .read_link_limited(&file, None, &mut validate)
            .expect_err("an ordinary file cannot be read as a symlink");
        assert_eq!(error.operation(), LocalFileOperation::Metadata);
        assert_eq!(error.path(), Some(file.as_path()));
        assert!(error.io_error().is_some());
        std::os::unix::fs::symlink("ordinary", fixture.path().join("link")).expect("real link should exist");
        let link = LocalRelativePath::new(Path::new("link")).expect("link path should be relative");
        assert_eq!(
            root.read_link_limited(&link, None, &mut validate)
                .expect("link target should be validated"),
            Path::new("ordinary")
        );
        assert_eq!(validations, 1, "only the successful read may validate a raw target");

        let missing = LocalRelativePath::new(Path::new("missing")).expect("path should be relative");
        let error = root
            .remove_tree_typed(&missing)
            .expect_err("recursive removal must report a missing target");
        assert_eq!(error.operation(), LocalFileOperation::DeleteDirectory);
        assert_eq!(error.path(), Some(missing.as_path()));
        assert_eq!(error.io_error_kind(), std::io::ErrorKind::NotFound);
        assert_eq!(
            fs::read(fixture.path().join("ordinary")).expect("unrelated file should remain"),
            b"unchanged"
        );
    }

    #[cfg(unix)]
    #[test]
    fn test_root_copy_symlink_collision_reports_unchanged_destination() {
        let fixture = tempfile::tempdir().expect("root fixture should exist");
        let destination = fixture.path().join("occupied");
        fs::write(&destination, b"original").expect("destination should exist");
        let root = Root::open(fixture.path()).expect("root should open");
        let path = LocalRelativePath::new(Path::new("occupied")).expect("path should be relative");

        let error = root
            .create_symlink_for_copy(Path::new("target"), &path, false)
            .expect_err("symlink publication must not replace an occupied destination");

        let (state, primary, cleanup) = error.into_parts();
        assert_eq!(state, crate::local::RootedSymlinkCreateFailureState::Unchanged);
        assert_eq!(primary.kind(), std::io::ErrorKind::AlreadyExists);
        assert!(cleanup.is_none());
        assert_eq!(
            fs::read(destination).expect("original destination should remain"),
            b"original"
        );
    }
}
