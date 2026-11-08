// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Identity-checked read access to a retained Unix namespace entry.

use std::ffi::CString;
use std::fs::File;
use std::io;
use std::os::fd::AsRawFd;
use std::os::fd::FromRawFd;
use std::os::unix::fs::MetadataExt;
use std::path::PathBuf;

use super::super::RecoveryAccess;
use crate::LocalFileReader;
use crate::LocalResult;

/// Retains the native authority and identity needed to reopen one namespace
/// entry.
///
/// The diagnostic path is only for error reporting; operations use the retained
/// parent descriptor and single-component name. Reopened entries must remain
/// regular files with the original device and inode identity.
#[derive(Debug)]
pub(super) struct EntryAccess {
    /// Directory descriptor that anchors the entry name.
    parent: File,
    /// NUL-terminated single component used with `openat`.
    name: CString,
    /// Device number captured when the entry was retained.
    device: u64,
    /// Inode number captured when the entry was retained.
    inode: u64,
    /// Optional path used only to enrich diagnostics.
    diagnostic: Option<PathBuf>,
}

impl EntryAccess {
    /// Creates access bound to a retained parent descriptor and entry identity.
    ///
    /// # Parameters
    ///
    /// - `parent`: Retained directory descriptor used as the namespace
    ///   authority.
    /// - `name`: Native single-component name relative to `parent`.
    /// - `device`: Device number of the entry when it was retained.
    /// - `inode`: Inode number of the entry when it was retained.
    /// - `diagnostic`: Optional path used only in error context.
    ///
    /// # Returns
    ///
    /// Access that can reopen only the retained file identity.
    #[must_use]
    #[inline]
    pub(super) fn new(parent: File, name: CString, device: u64, inode: u64, diagnostic: Option<PathBuf>) -> Self {
        Self {
            parent,
            name,
            device,
            inode,
            diagnostic,
        }
    }
}

impl RecoveryAccess for EntryAccess {
    /// Opens the named entry without following symlinks and verifies its
    /// identity.
    ///
    /// # Returns
    ///
    /// A reader for the retained regular file after its identity is verified.
    ///
    /// # Errors
    ///
    /// Returns a contextual error when opening, metadata inspection, identity
    /// validation, or reader construction fails.
    fn open_reader(&self) -> LocalResult<LocalFileReader> {
        // SAFETY: the parent descriptor and single-component C string remain
        // live for openat; O_NOFOLLOW prevents substituting a symlink target.
        let descriptor = unsafe {
            libc::openat(
                self.parent.as_raw_fd(),
                self.name.as_ptr(),
                libc::O_RDONLY | libc::O_CLOEXEC | libc::O_NOFOLLOW | libc::O_NONBLOCK,
            )
        };
        if descriptor == -1 {
            return Err(crate::LocalFileError::from_io(
                crate::LocalFileOperation::OpenReader,
                self.diagnostic.clone(),
                None,
                io::Error::last_os_error(),
            ));
        }
        // SAFETY: openat returned a new descriptor uniquely owned here.
        let file = unsafe { File::from_raw_fd(descriptor) };
        #[cfg(test)]
        let metadata_result = crate::tests::native_io_fault_tests::metadata(&file);
        #[cfg(not(test))]
        let metadata_result = file.metadata();
        let metadata = metadata_result.map_err(|error| {
            crate::LocalFileError::from_io(
                crate::LocalFileOperation::OpenReader,
                self.diagnostic.clone(),
                None,
                error,
            )
        })?;
        if !metadata.is_file() || metadata.dev() != self.device || metadata.ino() != self.inode {
            return Err(crate::LocalFileError::from_io(
                crate::LocalFileOperation::OpenReader,
                self.diagnostic.clone(),
                None,
                io::Error::new(io::ErrorKind::NotFound, "recovery entry identity changed"),
            ));
        }
        LocalFileReader::from_file(file).map_err(|error| {
            crate::LocalFileError::from_io(
                crate::LocalFileOperation::OpenReader,
                self.diagnostic.clone(),
                None,
                error,
            )
        })
    }

    /// Synchronizes the verified entry through its retained parent descriptor.
    ///
    /// # Returns
    ///
    /// `Ok(())` after the verified file contents are synchronized.
    ///
    /// # Errors
    ///
    /// Returns an I/O error if the entry cannot be opened, its identity
    /// changed, or synchronization fails.
    fn sync_file(&self) -> io::Result<()> {
        // SAFETY: the retained parent and single-component name remain live
        // for openat; O_NOFOLLOW prevents syncing a substituted symlink.
        let descriptor = unsafe {
            libc::openat(
                self.parent.as_raw_fd(),
                self.name.as_ptr(),
                libc::O_RDONLY | libc::O_CLOEXEC | libc::O_NOFOLLOW | libc::O_NONBLOCK,
                0,
            )
        };
        if descriptor == -1 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: openat returned a new descriptor uniquely owned here.
        let file = unsafe { File::from_raw_fd(descriptor) };
        let metadata = file.metadata()?;
        if !metadata.is_file() || metadata.dev() != self.device || metadata.ino() != self.inode {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "conditional entry identity changed before file synchronization",
            ));
        }
        file.sync_all()
    }
}

#[cfg(test)]
mod tests {
    use std::ffi::CString;
    use std::fs::File;
    use std::io;
    use std::os::unix::fs::MetadataExt;
    use std::path::PathBuf;

    use super::super::super::RecoveryAccess;
    use super::EntryAccess;
    use crate::tests::native_io_fault_tests::NativeIoFaultGuard;
    use crate::tests::native_io_fault_tests::NativeIoPoint;

    #[test]
    fn test_retained_native_reader_metadata_failures_preserve_identity_and_diagnostic_context() {
        let fixture = tempfile::tempdir().expect("fixture");
        let path = fixture.path().join("retained");
        std::fs::write(&path, b"retained candidate").expect("retained entry");
        let original = std::fs::metadata(&path).expect("native identity");
        for diagnostic in [None, Some(PathBuf::from("/display-only/retained"))] {
            let access = EntryAccess::new(
                File::open(fixture.path()).expect("real native parent"),
                CString::new("retained").expect("single native component"),
                original.dev(),
                original.ino(),
                diagnostic.clone(),
            );
            for point in [NativeIoPoint::HandleMetadata, NativeIoPoint::ReaderMetadata] {
                let fault = NativeIoFaultGuard::fail_on(point, 1);
                let error = access.open_reader().expect_err("native reader metadata fails");
                assert!(fault.fired());
                assert_eq!(error.operation(), crate::LocalFileOperation::OpenReader);
                assert_eq!(error.path(), diagnostic.as_deref());
                assert_eq!(error.current_directory(), None);
                assert_eq!(error.io_error_kind(), io::ErrorKind::Other);
                assert!(error.to_string().contains("injected native I/O contract failure"));
                assert_eq!((access.device, access.inode), (original.dev(), original.ino()));
                assert_eq!(
                    std::fs::read(&path).expect("retained bytes unchanged"),
                    b"retained candidate"
                );
                drop(fault);
                let mut reader = access.open_reader().expect("retry identity-bound recovery reader");
                let mut bytes = Vec::new();
                qubit_io::Streams::copy_input_to_output(&mut reader, &mut bytes).expect("read recovery bytes");
                assert_eq!(bytes, b"retained candidate");
                access.sync_file().expect("same retained identity remains syncable");
            }
        }
    }
}
