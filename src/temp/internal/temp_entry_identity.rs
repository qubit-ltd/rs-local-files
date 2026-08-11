// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Stable identity checks for cleanup-owned host temporary entries.
// qubit-style: allow source-test-pair

use std::fs;
use std::io;
use std::path::Path;

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
        #[cfg(unix)]
        {
            Self::from_metadata(&file.metadata()?)
        }
        #[cfg(windows)]
        {
            Self::from_windows_file(file)
        }
        #[cfg(not(any(unix, windows)))]
        {
            let _ = file;
            Err(io::Error::new(
                io::ErrorKind::Unsupported,
                "temporary entry identity is unsupported on this platform",
            ))
        }
    }

    /// Captures identity from a newly created directory path.
    pub(crate) fn from_path(path: &Path) -> io::Result<Self> {
        #[cfg(windows)]
        {
            use std::os::windows::fs::OpenOptionsExt;

            use windows_sys::Win32::Storage::FileSystem::FILE_FLAG_BACKUP_SEMANTICS;
            use windows_sys::Win32::Storage::FileSystem::FILE_FLAG_OPEN_REPARSE_POINT;

            let file = fs::OpenOptions::new()
                .read(true)
                .custom_flags(FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT)
                .open(path)?;
            Self::from_windows_file(&file)
        }
        #[cfg(not(windows))]
        {
            Self::from_metadata(&fs::symlink_metadata(path)?)
        }
    }

    /// Returns whether `path` still names the captured entry.
    pub(crate) fn matches_path(&self, path: &Path) -> io::Result<bool> {
        Ok(*self == Self::from_path(path)?)
    }

    /// Captures the stable native identity reported by metadata.
    #[cfg(not(windows))]
    fn from_metadata(metadata: &fs::Metadata) -> io::Result<Self> {
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;

            Ok(Self {
                device: metadata.dev(),
                inode: metadata.ino(),
            })
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

    /// Captures volume and file identifiers from an opened Windows handle.
    #[cfg(windows)]
    fn from_windows_file(file: &fs::File) -> io::Result<Self> {
        use std::os::windows::io::AsRawHandle;

        use windows_sys::Win32::Storage::FileSystem::BY_HANDLE_FILE_INFORMATION;
        use windows_sys::Win32::Storage::FileSystem::GetFileInformationByHandle;

        let mut information = BY_HANDLE_FILE_INFORMATION::default();
        // SAFETY: `file` owns a live handle and `information` is the matching
        // writable output structure.
        let result =
            unsafe { GetFileInformationByHandle(file.as_raw_handle(), &raw mut information) };
        if result == 0 {
            return Err(io::Error::last_os_error());
        }
        Ok(Self {
            volume: u64::from(information.dwVolumeSerialNumber),
            file: (u64::from(information.nFileIndexHigh) << 32)
                | u64::from(information.nFileIndexLow),
        })
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
