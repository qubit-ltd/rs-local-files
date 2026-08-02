// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Lazy descriptor-relative directory enumeration.

use std::fs::File;
use std::io::Result;
use std::path::Path;

use crate::local;

use super::{Entry, Metadata};

/// A single opened rooted directory whose children are yielded on demand.
#[derive(Debug)]
pub(crate) struct DirectoryReader {
    /// Platform-native directory enumerator.
    #[cfg(unix)]
    inner: local::RootedDirectoryReader,
    /// Native stream that advances without materializing the whole directory.
    #[cfg(windows)]
    inner: local::RootedDirectoryReader,
}

impl DirectoryReader {
    /// Opens a lazy reader for the already-opened root directory.
    ///
    /// Returns an I/O error when the root cannot be enumerated.
    pub(crate) fn open_root(root: &File, diagnostic_root: &Path) -> Result<Self> {
        #[cfg(unix)]
        {
            local::open_root_directory_reader(root, diagnostic_root).map(|inner| Self { inner })
        }
        #[cfg(windows)]
        {
            local::open_root_directory_reader(root, diagnostic_root).map(|inner| Self { inner })
        }
        #[cfg(not(any(unix, windows)))]
        {
            let _ = (root, diagnostic_root);
            Err(std::io::Error::new(
                std::io::ErrorKind::Unsupported,
                "descriptor-relative local roots are unsupported on this platform",
            ))
        }
    }

    /// Opens a lazy reader for a validated descendant directory.
    ///
    /// Returns an I/O error when secure traversal or enumeration cannot be
    /// performed.
    pub(crate) fn open_descendant(
        root: &File,
        diagnostic_root: &Path,
        path: &super::Path,
    ) -> Result<Self> {
        #[cfg(unix)]
        {
            local::open_rooted_directory_reader(root, diagnostic_root, path)
                .map(|inner| Self { inner })
        }
        #[cfg(windows)]
        {
            local::open_rooted_directory_reader(root, diagnostic_root, path)
                .map(|inner| Self { inner })
        }
        #[cfg(not(any(unix, windows)))]
        {
            let _ = (root, diagnostic_root, path);
            Err(std::io::Error::new(
                std::io::ErrorKind::Unsupported,
                "descriptor-relative local roots are unsupported on this platform",
            ))
        }
    }

    /// Produces the next immediate child from the open directory.
    ///
    /// Returns `Ok(None)` once the directory is exhausted, and returns an I/O
    /// error when native enumeration or metadata inspection fails.
    pub(crate) fn next_entry(&mut self) -> Result<Option<Entry>> {
        #[cfg(unix)]
        {
            self.inner.next_entry().map(|entry| {
                entry.map(|(name, status)| Entry::new(name, Metadata::from_stat(&status)))
            })
        }
        #[cfg(windows)]
        {
            self.inner.next_entry().and_then(|entry| {
                entry
                    .map(|(name, file)| {
                        Metadata::from_open_file(&file).map(|metadata| Entry::new(name, metadata))
                    })
                    .transpose()
            })
        }
        #[cfg(not(any(unix, windows)))]
        {
            Err(std::io::Error::new(
                std::io::ErrorKind::Unsupported,
                "descriptor-relative local roots are unsupported on this platform",
            ))
        }
    }
}
