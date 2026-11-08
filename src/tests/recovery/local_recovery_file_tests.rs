// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::fs::File;
use std::fs::OpenOptions;
use std::fs::read;
use std::fs::remove_file;
use std::fs::write;
use std::io::ErrorKind;
use std::io::Result;
use std::path::Path;
use std::path::PathBuf;

use qubit_io::Streams;
use tempfile::tempdir;

use crate::LocalFileError;
use crate::LocalFileOperation;
use crate::LocalFileReader;
use crate::LocalResult;
use crate::local::conditional::ConditionalEntry;
use crate::local::conditional::RecoveryAccess;
use crate::recovery::LocalRecoveryFileRole;
use crate::tests::native_io_fault_tests::NativeIoFaultGuard;
use crate::tests::native_io_fault_tests::NativeIoPoint;

#[derive(Debug)]
struct FakeRecoveryAccess {
    path: PathBuf,
}

impl RecoveryAccess for FakeRecoveryAccess {
    fn open_reader(&self) -> LocalResult<LocalFileReader> {
        let file = File::open(&self.path).map_err(|error| {
            LocalFileError::from_io(LocalFileOperation::OpenReader, Some(self.path.clone()), None, error)
        })?;
        LocalFileReader::from_file(file).map_err(|error| {
            LocalFileError::from_io(LocalFileOperation::OpenReader, Some(self.path.clone()), None, error)
        })
    }

    fn sync_file(&self) -> Result<()> {
        OpenOptions::new().read(true).write(true).open(&self.path)?.sync_all()
    }
}

#[test]
fn test_recovery_access_reports_removal_and_sync_preserves_existing_content() {
    let directory = tempdir().expect("fixture");
    let path = directory.path().join("retained");
    write(&path, b"candidate").expect("retained candidate");
    let access = FakeRecoveryAccess { path: path.clone() };
    access.sync_file().expect("retained file synchronization");
    remove_file(&path).expect("external entry removal");
    let error = access.open_reader().expect_err("removed recovery entry must fail");
    assert_eq!(error.operation(), LocalFileOperation::OpenReader);
    assert_eq!(error.io_error_kind(), ErrorKind::NotFound);
    assert_eq!(error.path(), Some(path.as_path()));
    assert_eq!(
        access.sync_file().expect_err("removed entry cannot sync").kind(),
        ErrorKind::NotFound
    );
}

#[test]
fn test_recovery_reader_initialization_failure_preserves_role_paths_and_source() {
    let fixture = tempdir().expect("fixture");
    let native = fixture.path().join("retained");
    write(&native, b"candidate").expect("retained candidate");
    let entry = ConditionalEntry::new(
        LocalRecoveryFileRole::CandidateCopy,
        PathBuf::from("/logical/retained"),
        None,
        Box::new(FakeRecoveryAccess { path: native.clone() }),
    );
    let recovery = entry.into_recovery(LocalRecoveryFileRole::CandidateCopy);
    let fault = NativeIoFaultGuard::fail_on(NativeIoPoint::ReaderMetadata, 1);
    let error = recovery
        .open_reader()
        .expect_err("recovery reader initialization fails");
    assert!(fault.fired());
    assert_eq!(error.operation(), LocalFileOperation::OpenReader);
    assert_eq!(error.path(), Some(native.as_path()));
    assert_eq!(error.io_error_kind(), ErrorKind::Other);
    assert!(error.to_string().contains("injected native I/O contract failure"));
    assert_eq!(recovery.role(), LocalRecoveryFileRole::CandidateCopy);
    assert_eq!(recovery.namespace_path(), Path::new("/logical/retained"));
    assert_eq!(recovery.diagnostic_path(), None);
    assert_eq!(read(&native).expect("retained source unchanged"), b"candidate");
    drop(fault);
    let mut reader = recovery.open_reader().expect("retry recovery reader");
    let mut bytes = Vec::new();
    Streams::copy_input_to_output(&mut reader, &mut bytes).expect("retry recovery bytes");
    assert_eq!(bytes, b"candidate");
    drop(recovery);
    assert_eq!(
        read(&native).expect("dropping recovery never deletes source"),
        b"candidate"
    );
}
