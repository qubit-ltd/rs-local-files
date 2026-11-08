// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Identity checked, read-only recovery access for Unix namespace entries.

use std::ffi::CString;
use std::io;
use std::os::unix::ffi::OsStrExt;

/// Converts an entry's namespace basename to the native component format.
///
/// The display path is not used as authority; the returned component is
/// suitable only alongside the retained parent descriptor owned by its access.
///
/// # Parameters
///
/// - `entry`: Entry whose namespace basename is converted.
///
/// # Returns
///
/// The NUL-terminated native basename for use with its retained directory
/// descriptor.
///
/// # Errors
///
/// Returns `InvalidInput` when the namespace path has no final component or its
/// native name contains a NUL byte.
pub(super) fn name(entry: &super::super::ConditionalEntry) -> io::Result<CString> {
    let name = entry
        .namespace_path()
        .file_name()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "entry path has no final component"))?;
    CString::new(name.as_bytes()).map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "entry name contains NUL"))
}

#[cfg(test)]
mod invalid_name_contract_tests {
    use std::ffi::CString;
    use std::fs::File;
    use std::io;
    use std::os::unix::fs::MetadataExt;
    use std::path::PathBuf;

    use super::super::entry_access::EntryAccess;
    use super::name;

    #[test]
    fn test_recovery_name_rejects_missing_component_and_nul_without_using_display_as_authority() {
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
