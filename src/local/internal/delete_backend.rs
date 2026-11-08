// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Backend capabilities used by the shared recursive deletion scheduler.

use std::io;
use std::path::Path;

/// Native operations needed by [`super::delete_tree::remove_directory_tree`].
pub(crate) trait DeleteBackend {
    /// Path representation retained by the backend work queue.
    type Path: Clone;
    /// No-follow metadata returned by the backend.
    type Metadata;
    /// Lazy directory reader returned by the backend.
    type Reader;

    /// Returns the path used for diagnostics and budget accounting.
    ///
    /// # Parameters
    ///
    /// * `value` is the backend-owned coordinate to borrow as a native path.
    ///
    /// # Returns
    ///
    /// The native path view used for diagnostics and resource accounting.
    fn path<'a>(&self, value: &'a Self::Path) -> &'a Path;
    /// Reads no-follow metadata for one entry.
    ///
    /// # Parameters
    ///
    /// * `path` identifies the entry whose metadata is inspected.
    ///
    /// # Errors
    ///
    /// Returns an I/O error if the entry cannot be inspected.
    fn metadata(&self, path: &Self::Path) -> io::Result<Self::Metadata>;
    /// Reports whether metadata describes a real directory.
    ///
    /// # Parameters
    ///
    /// * `metadata` is the no-follow metadata returned for an entry.
    ///
    /// # Returns
    ///
    /// `true` only when the entry itself is a real directory.
    #[must_use]
    fn is_directory(&self, metadata: &Self::Metadata) -> bool;
    /// Opens a lazy reader for one real directory.
    ///
    /// # Parameters
    ///
    /// * `path` identifies the directory to enumerate.
    ///
    /// # Errors
    ///
    /// Returns an I/O error if the directory cannot be opened.
    fn open_directory(&self, path: &Self::Path) -> io::Result<Self::Reader>;
    /// Returns the next immediate child from an opened directory.
    ///
    /// # Parameters
    ///
    /// * `parent` is the directory whose reader is being consumed.
    /// * `reader` is the mutable enumeration state for that directory.
    ///
    /// # Returns
    ///
    /// `Some` contains the next immediate child; `None` means enumeration is
    /// complete.
    ///
    /// # Errors
    ///
    /// Returns an I/O error if reading the next directory entry fails.
    fn next_child(&self, parent: &Self::Path, reader: &mut Self::Reader) -> io::Result<Option<Self::Path>>;
    /// Removes one entry that is known not to be a real directory.
    ///
    /// # Parameters
    ///
    /// * `path` identifies the leaf entry to remove.
    /// * `metadata` is the no-follow metadata used to classify that entry.
    ///
    /// # Errors
    ///
    /// Returns an I/O error if the entry cannot be removed.
    fn remove_non_directory(&self, path: &Self::Path, metadata: &Self::Metadata) -> io::Result<()>;
    /// Removes one empty real directory.
    ///
    /// # Parameters
    ///
    /// * `path` identifies the empty directory to remove.
    ///
    /// # Errors
    ///
    /// Returns an I/O error if the directory cannot be removed.
    fn remove_empty_directory(&self, path: &Self::Path) -> io::Result<()>;
    /// Runs a deterministic fault hook immediately before native removal.
    ///
    /// # Parameters
    ///
    /// * `path` identifies the entry about to be removed.
    ///
    /// # Errors
    ///
    /// Returns an I/O error when the deterministic test fault hook rejects
    /// the operation.
    fn before_remove(&self, path: &Self::Path) -> io::Result<()>;
}
