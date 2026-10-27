// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
#[cfg(windows)]
use crate::observation::LocalObservationRequirement;

#[cfg(windows)]
#[test]
fn lf3_native_windows_preserves_full_identity() {
    use crate::LocalFileKind;
    use crate::observation::windows::query_native;

    let file = tempfile::NamedTempFile::new().expect("create fixture");
    let first =
        query_native(file.as_file(), LocalObservationRequirement::Required).expect("query required observation");
    let second =
        query_native(file.as_file(), LocalObservationRequirement::Required).expect("refresh required observation");
    assert_eq!(first.identity(), second.identity());
    let details = first
        .native_details()
        .and_then(|details| details.windows())
        .expect("Windows native details");
    assert_eq!(details.file_id().len(), 16);
    assert!(first.native_details().and_then(|details| details.unix()).is_none());
    assert_eq!(first.metadata().kind(), LocalFileKind::File);
}

#[cfg(windows)]
#[test]
fn lf3_native_windows_observes_directories_and_read_only_handles() {
    use std::fs::File;
    use std::fs::OpenOptions;
    use std::os::windows::fs::OpenOptionsExt;

    use windows_sys::Win32::Storage::FileSystem::FILE_FLAG_BACKUP_SEMANTICS;

    use crate::LocalFileKind;
    use crate::observation::windows::query_native;

    let directory = tempfile::tempdir().expect("create directory fixture");
    let directory_file = OpenOptions::new()
        .read(true)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS)
        .open(directory.path())
        .expect("open directory handle");
    let observation =
        query_native(&directory_file, LocalObservationRequirement::Required).expect("observe directory handle");
    assert!(observation.identity().is_some());
    assert_eq!(observation.metadata().kind(), LocalFileKind::Directory);

    let file = tempfile::NamedTempFile::new().expect("create file fixture");
    let readonly = File::open(file.path()).expect("open read-only file handle");
    let observation =
        query_native(&readonly, LocalObservationRequirement::Required).expect("observe read-only file handle");
    assert!(observation.identity().is_some());
    assert_eq!(observation.metadata().kind(), LocalFileKind::File);
}

#[cfg(windows)]
#[test]
fn lf3_native_windows_optional_query_failure_keeps_metadata_and_required_keeps_source() {
    use std::error::Error;
    use std::sync::atomic::AtomicU32;
    use std::sync::atomic::Ordering;

    use windows_sys::Win32::Foundation::ERROR_ACCESS_DENIED;
    use windows_sys::Win32::Foundation::SetLastError;
    use windows_sys::Win32::Storage::FileSystem::FILE_INFO_BY_HANDLE_CLASS;
    use windows_sys::Win32::Storage::FileSystem::FileBasicInfo;
    use windows_sys::Win32::Storage::FileSystem::FileIdInfo;
    use windows_sys::Win32::Storage::FileSystem::GetFileInformationByHandleEx;

    use crate::LocalFileKind;
    use crate::error::LocalFileErrorKind;
    use crate::observation::windows::NativeQuery;
    use crate::observation::windows::query_native_with;

    static FAILED_CLASS: AtomicU32 = AtomicU32::new(0);

    unsafe extern "system" fn fail_selected_class(
        handle: windows_sys::Win32::Foundation::HANDLE,
        class: FILE_INFO_BY_HANDLE_CLASS,
        output: *mut std::ffi::c_void,
        size: u32,
    ) -> i32 {
        if class == FAILED_CLASS.load(Ordering::Relaxed) as FILE_INFO_BY_HANDLE_CLASS {
            // SAFETY: SetLastError records thread-local error state and has no
            // pointer or handle preconditions.
            unsafe { SetLastError(ERROR_ACCESS_DENIED) };
            0
        } else {
            // SAFETY: this forwards the live handle and correctly sized output
            // buffer supplied by the native query helper.
            unsafe { GetFileInformationByHandleEx(handle, class, output, size) }
        }
    }

    let file = tempfile::NamedTempFile::new().expect("create fixture");
    let query = fail_selected_class as NativeQuery;
    for class in [FileIdInfo, FileBasicInfo] {
        FAILED_CLASS.store(class as u32, Ordering::Relaxed);
        let optional = query_native_with(file.as_file(), LocalObservationRequirement::Optional, query)
            .expect("optional native failure retains ordinary metadata");
        assert_eq!(optional.metadata().kind(), LocalFileKind::File);
        assert!(optional.identity().is_none());
        assert!(optional.native_details().is_none());

        let required = query_native_with(file.as_file(), LocalObservationRequirement::Required, query)
            .expect_err("required native failure must be reported");
        assert_eq!(required.kind(), LocalFileErrorKind::RequirementNotMet);
        assert!(required.source().is_some(), "native I/O source is retained");
    }
}
