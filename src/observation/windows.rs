// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
use std::fs::File;
use std::io;
use std::mem::size_of;
use std::os::windows::io::AsRawHandle;
use std::ptr;

use windows_sys::Win32::Storage::FileSystem::FILE_BASIC_INFO;
use windows_sys::Win32::Storage::FileSystem::FILE_ID_INFO;
use windows_sys::Win32::Storage::FileSystem::FileBasicInfo;
use windows_sys::Win32::Storage::FileSystem::FileIdInfo;
use windows_sys::Win32::Storage::FileSystem::GetFileInformationByHandleEx;

use crate::LocalFileError;
use crate::LocalFileMetadata;
use crate::LocalResult;
use crate::error::LocalFileErrorKind;
use crate::error::LocalFileOperation;
use crate::observation::LocalFileIdentity;
use crate::observation::LocalFileObservation;
use crate::observation::LocalNativeFileDetails;
use crate::observation::LocalObservationRequirement;
use crate::observation::LocalWindowsFileDetails;

/// Native handle query function used to retrieve Windows file information.
///
/// The caller must provide a valid live handle and a writable buffer with the
/// size required by the selected information class.
pub(crate) type NativeQuery = unsafe extern "system" fn(
    windows_sys::Win32::Foundation::HANDLE,
    windows_sys::Win32::Storage::FileSystem::FILE_INFO_BY_HANDLE_CLASS,
    *mut std::ffi::c_void,
    u32,
) -> i32;

/// Queries identity and timestamps through the operating system API.
///
/// The handle remains borrowed from `file`. When optional facts cannot be
/// queried, the result retains normalized metadata with absent native details;
/// required facts turn that condition into `RequirementNotMet`.
pub(crate) fn query_native(file: &File, requirement: LocalObservationRequirement) -> LocalResult<LocalFileObservation> {
    query_native_with(file, requirement, GetFileInformationByHandleEx)
}

/// Runs a supplied native query function, primarily to exercise failure paths.
///
/// Metadata failure is returned as an I/O error regardless of requirement. A
/// failed native facts query is omitted for `Optional` and returned as
/// `RequirementNotMet` for `Required`.
pub(crate) fn query_native_with(
    file: &File,
    requirement: LocalObservationRequirement,
    query: NativeQuery,
) -> LocalResult<LocalFileObservation> {
    let metadata = file
        .metadata()
        .map_err(|error| LocalFileError::from_io(LocalFileOperation::Metadata, None, None, error))?;
    let normalized = LocalFileMetadata::from_native(&metadata);
    let handle = file.as_raw_handle();

    let native = (|| -> io::Result<_> {
        let mut id = FILE_ID_INFO::default();
        query_info(query, handle, FileIdInfo, &mut id)?;
        let mut basic = FILE_BASIC_INFO::default();
        query_info(query, handle, FileBasicInfo, &mut basic)?;
        Ok((id, basic))
    })();

    match native {
        Ok((id, basic)) => {
            let file_id = id.FileId.Identifier;
            let identity = LocalFileIdentity::windows(id.VolumeSerialNumber, file_id);
            let details =
                LocalWindowsFileDetails::new(id.VolumeSerialNumber, file_id, basic.LastWriteTime, basic.ChangeTime);
            Ok(LocalFileObservation::from_parts(
                normalized,
                Some(identity),
                Some(LocalNativeFileDetails::from_windows(details)),
            ))
        }
        Err(_error) if requirement == LocalObservationRequirement::Optional => {
            Ok(LocalFileObservation::from_parts(normalized, None, None))
        }
        Err(error) => Err(LocalFileError::from_io(LocalFileOperation::Metadata, None, None, error)
            .with_kind(LocalFileErrorKind::RequirementNotMet)),
    }
}

/// Calls one information class with a correctly sized output value.
fn query_info<T: Default>(
    query: NativeQuery,
    handle: windows_sys::Win32::Foundation::HANDLE,
    class: windows_sys::Win32::Storage::FileSystem::FILE_INFO_BY_HANDLE_CLASS,
    output: &mut T,
) -> io::Result<()> {
    // SAFETY: `handle` is borrowed from a live `File`, and `output` points to
    // a writable, correctly sized value for the requested information class.
    let succeeded = unsafe { query(handle, class, ptr::from_mut(output).cast(), size_of::<T>() as u32) };
    if succeeded == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}
