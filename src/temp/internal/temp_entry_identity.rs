// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Stable identity checks for cleanup-owned host temporary entries.

use std::{fs, io, path::Path};

/// Native identity captured when a host temporary entry is created.
#[derive(Debug)]
pub(crate) struct TempEntryIdentity {
    /// Unix device identifier.
    #[cfg(unix)]
    device: u64,
    /// Unix inode identifier.
    #[cfg(unix)]
    inode: u64,
    /// Windows volume serial number.
    #[cfg(windows)]
    volume: u64,
    /// Windows file index within the volume.
    #[cfg(windows)]
    file: u64,
}

impl TempEntryIdentity {
    /// Captures identity from a newly created regular file handle.
    pub(crate) fn from_file(file: &fs::File) -> io::Result<Self> {
        Self::from_metadata(&file.metadata()?)
    }

    /// Captures identity from a newly created directory path.
    pub(crate) fn from_path(path: &Path) -> io::Result<Self> {
        Self::from_metadata(&fs::symlink_metadata(path)?)
    }

    /// Returns whether `path` still names the captured entry.
    pub(crate) fn matches_path(&self, path: &Path) -> io::Result<bool> {
        Ok(*self == Self::from_path(path)?)
    }

    /// Captures the stable native identity reported by metadata.
    fn from_metadata(metadata: &fs::Metadata) -> io::Result<Self> {
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;

            return Ok(Self {
                device: metadata.dev(),
                inode: metadata.ino(),
            });
        }
        #[cfg(windows)]
        {
            use std::os::windows::fs::MetadataExt;

            return Ok(Self {
                volume: metadata.volume_serial_number().unwrap_or_default(),
                file: metadata.file_index(),
            });
        }
        #[cfg(not(any(unix, windows)))]
        {
            let _ = metadata;
            Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "temporary entry identity is unsupported on this platform",
            ))
        }
    }
}

impl PartialEq for TempEntryIdentity {
    fn eq(&self, other: &Self) -> bool {
        #[cfg(unix)]
        {
            self.device == other.device && self.inode == other.inode
        }
        #[cfg(windows)]
        {
            self.volume == other.volume && self.file == other.file
        }
        #[cfg(not(any(unix, windows)))]
        {
            let _ = other;
            false
        }
    }
}

