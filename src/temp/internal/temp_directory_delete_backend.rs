// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Borrowed native backends for the shared temporary-tree deletion scheduler.
// qubit-style: allow source-test-pair
// Covered by temporary cleanup and rooted tree integration tests.

use std::fs;
use std::io;
use std::path::Path;
use std::path::PathBuf;

use super::HostTempResourceBackend;
use crate::LocalRelativePath;
use crate::local::DeleteBackend;
use crate::rooted::Root;

/// Borrows the original opened Root; diagnostic paths never confer authority.
pub(crate) struct TempDirectoryDeleteBackend<'root> {
    /// Retained root used for every native traversal and unlink.
    pub(crate) root: &'root Root,
}

impl DeleteBackend for TempDirectoryDeleteBackend<'_> {
    /// Validated authority-relative coordinates retained in the queue.
    type Path = LocalRelativePath;
    /// No-follow metadata read through the original root.
    type Metadata = crate::rooted::Metadata;
    /// Lazy directory reader bound to the root authority.
    type Reader = crate::rooted::DirectoryReader;

    /// Borrows the authority-relative path for queue accounting and
    /// diagnostics.
    fn path<'a>(&self, path: &'a Self::Path) -> &'a Path {
        path.as_path()
    }

    /// Reads no-follow metadata; traversal and disappearance errors propagate.
    fn metadata(&self, path: &Self::Path) -> io::Result<Self::Metadata> {
        #[cfg(feature = "test-support")]
        crate::test_support::temp_cleanup_metadata()?;
        self.root.symlink_metadata(path)
    }

    /// Distinguishes real directories from links and other leaves.
    fn is_directory(&self, metadata: &Self::Metadata) -> bool {
        metadata.kind() == crate::rooted::EntryKind::Directory
    }

    /// Opens an unsorted lazy reader through the retained Root authority.
    /// Native traversal and enumeration-open failures propagate.
    fn open_directory(&self, path: &Self::Path) -> io::Result<Self::Reader> {
        self.root.open_dir_reader(path)
    }

    /// Returns the next validated child; native enumeration errors propagate.
    fn next_child(&self, parent: &Self::Path, reader: &mut Self::Reader) -> io::Result<Option<Self::Path>> {
        reader
            .next_entry()
            .and_then(|entry| entry.map_or(Ok(None), |entry| parent.join_component(entry.name()).map(Some)))
    }

    /// Removes the leaf itself without following its target; native errors
    /// propagate.
    fn remove_non_directory(&self, path: &Self::Path, _metadata: &Self::Metadata) -> io::Result<()> {
        self.root.remove_observed_non_directory(path)
    }

    /// Removes one empty directory; concurrent additions fail without
    /// rescanning.
    fn remove_empty_directory(&self, path: &Self::Path) -> io::Result<()> {
        #[cfg(feature = "test-support")]
        if crate::test_support::temp_cleanup_add_child() {
            self.root
                .create_dir(&path.join_component(std::ffi::OsStr::new("late-child"))?)?;
        }
        self.root.remove_observed_empty_directory(path)
    }

    /// Runs the thread-isolated native-removal failure boundary in test builds.
    fn before_remove(&self, _path: &Self::Path) -> io::Result<()> {
        #[cfg(feature = "test-support")]
        crate::test_support::temp_cleanup_before_remove()?;
        Ok(())
    }
}

impl DeleteBackend for HostTempResourceBackend {
    /// Native coordinates bound at temporary resource creation.
    type Path = PathBuf;
    /// No-follow metadata, including native directory-link information.
    type Metadata = fs::Metadata;
    /// Lazy native directory enumeration.
    type Reader = fs::ReadDir;

    /// Borrows the creation-bound native path for accounting and diagnostics.
    fn path<'a>(&self, path: &'a Self::Path) -> &'a Path {
        path
    }

    /// Inspects a final entry without following links; native errors propagate.
    fn metadata(&self, path: &Self::Path) -> io::Result<Self::Metadata> {
        #[cfg(feature = "test-support")]
        crate::test_support::temp_cleanup_metadata()?;
        fs::symlink_metadata(path)
    }

    /// Reports whether the observed entry is a real directory.
    fn is_directory(&self, metadata: &Self::Metadata) -> bool {
        metadata.file_type().is_dir()
    }

    /// Opens an unsorted lazy native reader, propagating native open errors.
    fn open_directory(&self, path: &Self::Path) -> io::Result<Self::Reader> {
        fs::read_dir(path)
    }

    /// Produces one child path or propagates an enumeration error.
    fn next_child(&self, _parent: &Self::Path, reader: &mut Self::Reader) -> io::Result<Option<Self::Path>> {
        reader.next().transpose().map(|entry| entry.map(|entry| entry.path()))
    }

    /// Removes the leaf itself, including Windows directory links, without
    /// traversing its target. Native unlink errors propagate.
    fn remove_non_directory(&self, path: &Self::Path, metadata: &Self::Metadata) -> io::Result<()> {
        crate::local::remove_host_non_directory(path, metadata)
    }

    /// Removes one empty directory; concurrent additions fail without
    /// rescanning.
    fn remove_empty_directory(&self, path: &Self::Path) -> io::Result<()> {
        #[cfg(feature = "test-support")]
        if crate::test_support::temp_cleanup_add_child() {
            fs::create_dir(path.join("late-child"))?;
        }
        fs::remove_dir(path)
    }

    /// Runs the thread-isolated native-removal failure boundary in test builds.
    fn before_remove(&self, _path: &Self::Path) -> io::Result<()> {
        #[cfg(feature = "test-support")]
        crate::test_support::temp_cleanup_before_remove()?;
        Ok(())
    }
}
