// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Identity checked, read-only recovery access for Unix namespace entries.

use std::ffi::CString;
use std::fs::File;
use std::io;
use std::os::fd::AsRawFd;
use std::os::fd::FromRawFd;
use std::os::unix::fs::MetadataExt;
use std::path::PathBuf;

use super::RecoveryAccess;
use crate::LocalFileReader;
use crate::LocalResult;

#[derive(Debug)]
pub(super) struct EntryAccess {
    parent: File,
    name: CString,
    device: u64,
    inode: u64,
    diagnostic: Option<PathBuf>,
}

impl EntryAccess {
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

pub(super) fn name(entry: &super::super::ConditionalEntry) -> io::Result<CString> {
    use std::os::unix::ffi::OsStrExt;
    let name = entry
        .namespace_path()
        .file_name()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "entry path has no final component"))?;
    CString::new(name.as_bytes()).map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "entry name contains NUL"))
}

#[cfg(test)]
mod invalid_name_contract_tests {
    use super::*;

    #[test]
    fn retained_native_reader_metadata_failures_preserve_identity_and_diagnostic_context() {
        use crate::tests::native_io_fault_tests::NativeIoFaultGuard;
        use crate::tests::native_io_fault_tests::NativeIoPoint;

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

    #[test]
    fn recovery_name_rejects_missing_component_and_nul_without_using_display_as_authority() {
        let fixture = tempfile::tempdir().expect("fixture");
        let path = fixture.path().join("retained");
        std::fs::write(&path, b"retained").expect("retained entry");
        let metadata = std::fs::metadata(&path).expect("real identity");
        for namespace_path in [PathBuf::from("/"), PathBuf::from("bad\0name")] {
            let entry = super::super::super::ConditionalEntry::new(
                crate::recovery::LocalRecoveryFileRole::CandidateCopy,
                namespace_path,
                None,
                Box::new(EntryAccess::new(
                    File::open(fixture.path()).expect("real retained parent"),
                    CString::new("retained").expect("native component"),
                    metadata.dev(),
                    metadata.ino(),
                    None,
                )),
            );
            assert_eq!(
                name(&entry).expect_err("invalid namespace name").kind(),
                io::ErrorKind::InvalidInput
            );
            entry.sync_file().expect("retained authority remains valid");
            assert_eq!(entry.open_reader().expect("retained reader").metadata().len(), 8);
        }
    }
}
