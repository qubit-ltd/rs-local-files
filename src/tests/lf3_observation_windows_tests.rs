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
fn lf3_native_windows_optional_io_failure_preserves_source_and_required_classification() {
    use std::error::Error;
    use std::sync::atomic::AtomicU32;
    use std::sync::atomic::Ordering;

    use windows_sys::Win32::Foundation::ERROR_ACCESS_DENIED;
    use windows_sys::Win32::Foundation::SetLastError;
    use windows_sys::Win32::Storage::FileSystem::FILE_INFO_BY_HANDLE_CLASS;
    use windows_sys::Win32::Storage::FileSystem::FileBasicInfo;
    use windows_sys::Win32::Storage::FileSystem::FileIdInfo;
    use windows_sys::Win32::Storage::FileSystem::GetFileInformationByHandleEx;

    use crate::error::LocalFileErrorKind;
    use crate::error::LocalFileErrorSource;
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
            .expect_err("optional native I/O failure must be reported");
        assert_eq!(optional.kind(), LocalFileErrorKind::Io);
        assert!(matches!(
            optional.typed_source(),
            Some(LocalFileErrorSource::Io(source)) if source.raw_os_error() == Some(ERROR_ACCESS_DENIED as i32)
        ));

        let required = query_native_with(file.as_file(), LocalObservationRequirement::Required, query)
            .expect_err("required native failure must be reported");
        assert_eq!(required.kind(), LocalFileErrorKind::RequirementNotMet);
        assert!(required.source().is_some(), "native I/O source is retained");
    }
}

#[cfg(windows)]
#[test]
fn lf3_native_windows_optional_explicitly_unsupported_query_keeps_metadata() {
    use std::sync::atomic::AtomicU32;
    use std::sync::atomic::Ordering;

    use windows_sys::Win32::Foundation::ERROR_INVALID_PARAMETER;
    use windows_sys::Win32::Foundation::ERROR_NOT_SUPPORTED;
    use windows_sys::Win32::Foundation::SetLastError;
    use windows_sys::Win32::Storage::FileSystem::FILE_INFO_BY_HANDLE_CLASS;
    use windows_sys::Win32::Storage::FileSystem::FileIdInfo;
    use windows_sys::Win32::Storage::FileSystem::GetFileInformationByHandleEx;

    use crate::LocalFileKind;
    use crate::observation::windows::NativeQuery;
    use crate::observation::windows::query_native_with;

    static ERROR_CODE: AtomicU32 = AtomicU32::new(ERROR_NOT_SUPPORTED);

    unsafe extern "system" fn reject_file_id_info(
        handle: windows_sys::Win32::Foundation::HANDLE,
        class: FILE_INFO_BY_HANDLE_CLASS,
        output: *mut std::ffi::c_void,
        size: u32,
    ) -> i32 {
        if class == FileIdInfo {
            // SAFETY: SetLastError records the error on this test thread.
            unsafe { SetLastError(ERROR_CODE.load(Ordering::Relaxed)) };
            0
        } else {
            // SAFETY: forwards the live handle and output buffer from query_info.
            unsafe { GetFileInformationByHandleEx(handle, class, output, size) }
        }
    }

    let file = tempfile::NamedTempFile::new().expect("create fixture");
    for error_code in [ERROR_NOT_SUPPORTED, ERROR_INVALID_PARAMETER] {
        ERROR_CODE.store(error_code, Ordering::Relaxed);
        let observation = query_native_with(
            file.as_file(),
            LocalObservationRequirement::Optional,
            reject_file_id_info as NativeQuery,
        )
        .expect("an explicitly unsupported optional fact should be omitted");
        assert_eq!(observation.metadata().kind(), LocalFileKind::File);
        assert!(observation.identity().is_none());
        assert!(observation.native_details().is_none());
    }
}

#[cfg(windows)]
#[test]
fn test_windows_observation_does_not_require_file_standard_info() {
    use std::cell::RefCell;

    use windows_sys::Win32::Foundation::ERROR_ACCESS_DENIED;
    use windows_sys::Win32::Foundation::SetLastError;
    use windows_sys::Win32::Storage::FileSystem::FILE_INFO_BY_HANDLE_CLASS;
    use windows_sys::Win32::Storage::FileSystem::FileBasicInfo;
    use windows_sys::Win32::Storage::FileSystem::FileIdInfo;
    use windows_sys::Win32::Storage::FileSystem::FileStandardInfo;
    use windows_sys::Win32::Storage::FileSystem::GetFileInformationByHandleEx;

    use crate::observation::windows::NativeQuery;
    use crate::observation::windows::query_native_with;

    thread_local! {
        static QUERIED_CLASSES: RefCell<Vec<FILE_INFO_BY_HANDLE_CLASS>> = const { RefCell::new(Vec::new()) };
    }

    unsafe extern "system" fn reject_file_standard_info(
        handle: windows_sys::Win32::Foundation::HANDLE,
        class: FILE_INFO_BY_HANDLE_CLASS,
        output: *mut std::ffi::c_void,
        size: u32,
    ) -> i32 {
        QUERIED_CLASSES.with(|classes| classes.borrow_mut().push(class));
        if class == FileStandardInfo {
            // SAFETY: SetLastError records the error on this test thread.
            unsafe { SetLastError(ERROR_ACCESS_DENIED) };
            0
        } else {
            // SAFETY: forwards the live handle and output buffer from query_info.
            unsafe { GetFileInformationByHandleEx(handle, class, output, size) }
        }
    }

    let file = tempfile::NamedTempFile::new().expect("create fixture");
    let query = reject_file_standard_info as NativeQuery;

    for requirement in [
        LocalObservationRequirement::Required,
        LocalObservationRequirement::Optional,
    ] {
        QUERIED_CLASSES.with(|classes| classes.borrow_mut().clear());
        let observation = query_native_with(file.as_file(), requirement, query)
            .expect("observation must not require FileStandardInfo");
        assert!(observation.identity().is_some());
        assert!(observation.native_details().is_some());

        QUERIED_CLASSES.with(|classes| {
            let mut classes = classes.borrow_mut().clone();
            classes.sort_unstable();
            let mut expected = vec![FileBasicInfo, FileIdInfo];
            expected.sort_unstable();
            assert_eq!(classes, expected, "only required information classes are queried");
        });
    }
}
