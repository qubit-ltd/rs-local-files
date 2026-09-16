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
    fn path<'a>(&self, value: &'a Self::Path) -> &'a Path;
    /// Reads no-follow metadata for one entry.
    fn metadata(&self, path: &Self::Path) -> io::Result<Self::Metadata>;
    /// Reports whether metadata describes a real directory.
    fn is_directory(&self, metadata: &Self::Metadata) -> bool;
    /// Opens a lazy reader for one real directory.
    fn open_directory(&self, path: &Self::Path) -> io::Result<Self::Reader>;
    /// Returns the next immediate child from an opened directory.
    fn next_child(&self, parent: &Self::Path, reader: &mut Self::Reader) -> io::Result<Option<Self::Path>>;
    /// Removes one entry that is known not to be a real directory.
    fn remove_non_directory(&self, path: &Self::Path, metadata: &Self::Metadata) -> io::Result<()>;
    /// Removes one empty real directory.
    fn remove_empty_directory(&self, path: &Self::Path) -> io::Result<()>;
    /// Runs a deterministic fault hook immediately before native removal.
    fn before_remove(&self, path: &Self::Path) -> io::Result<()>;
}
