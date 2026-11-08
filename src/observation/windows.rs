// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
use std::ffi::c_void;
use std::fs::File;
use std::io;
use std::mem::size_of;
use std::os::windows::io::AsRawHandle;
use std::ptr;

use windows_sys::Win32::Foundation::ERROR_INVALID_FUNCTION;
use windows_sys::Win32::Foundation::ERROR_INVALID_PARAMETER;
use windows_sys::Win32::Foundation::ERROR_NOT_SUPPORTED;
use windows_sys::Win32::Foundation::HANDLE;
use windows_sys::Win32::Storage::FileSystem::FILE_BASIC_INFO;
use windows_sys::Win32::Storage::FileSystem::FILE_ID_INFO;
use windows_sys::Win32::Storage::FileSystem::FILE_INFO_BY_HANDLE_CLASS;
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
pub(crate) type NativeQuery = unsafe extern "system" fn(HANDLE, FILE_INFO_BY_HANDLE_CLASS, *mut c_void, u32) -> i32;

/// Queries identity and timestamps through the operating system API.
///
/// The handle remains borrowed from `file`. When optional facts cannot be
/// queried, the result retains normalized metadata with absent native details;
/// required facts turn that condition into `RequirementNotMet`.
///
/// # Parameters
///
/// * `file` is the live file whose native identity and timestamps are queried.
/// * `requirement` determines whether unavailable native facts are acceptable.
///
/// # Returns
///
/// Returns normalized metadata with native details when available. Optional
/// unsupported facts produce an observation without native details.
///
/// # Errors
///
/// Returns an error if metadata cannot be read, an optional native query fails
/// for a reason other than explicit lack of support, or required native facts
/// cannot be queried.
#[inline]
pub(crate) fn query_native(file: &File, requirement: LocalObservationRequirement) -> LocalResult<LocalFileObservation> {
    query_native_with(file, requirement, GetFileInformationByHandleEx)
}

/// Runs a supplied native query function, primarily to exercise failure paths.
///
/// Metadata failure is returned as an I/O error regardless of requirement. A
/// Explicitly unsupported native facts are omitted for `Optional`. Other
/// native query failures are returned as I/O errors. `Required` returns
/// `RequirementNotMet` for native query failures or unavailable facts while
/// retaining the underlying I/O source when present.
///
/// # Parameters
///
/// * `file` is the live file whose metadata and native details are queried.
/// * `requirement` controls how unavailable native facts are handled.
/// * `query` supplies the native information query operation.
///
/// # Returns
///
/// Returns an observation with normalized metadata and any available native
/// details.
///
/// # Errors
///
/// Returns an I/O error for metadata or optional native query failures, and a
/// `RequirementNotMet` error for required native query failures.
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

    let mut failed_class = None;
    let native = (|| -> io::Result<_> {
        let mut id = FILE_ID_INFO::default();
        failed_class = Some(FileIdInfo);
        query_info(query, handle, FileIdInfo, &mut id)?;
        let mut basic = FILE_BASIC_INFO::default();
        failed_class = Some(FileBasicInfo);
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
        Err(error)
            if requirement == LocalObservationRequirement::Optional
                && explicitly_unsupported(&error, failed_class == Some(FileIdInfo)) =>
        {
            Ok(LocalFileObservation::from_parts(normalized, None, None))
        }
        Err(error) if requirement == LocalObservationRequirement::Optional => {
            Err(LocalFileError::from_io(LocalFileOperation::Metadata, None, None, error))
        }
        Err(error) => Err(LocalFileError::from_io(LocalFileOperation::Metadata, None, None, error)
            .with_kind(LocalFileErrorKind::RequirementNotMet)),
    }
}

/// Determines whether a native query failure explicitly reports unsupported
/// information for the requested query.
///
/// `ERROR_INVALID_PARAMETER` counts as unsupported only for a file ID query.
///
/// # Parameters
///
/// * `error` is the native query failure to classify.
/// * `querying_file_id` indicates whether the failed query requested file ID
///   information.
fn explicitly_unsupported(error: &io::Error, querying_file_id: bool) -> bool {
    match error.raw_os_error() {
        Some(code) => {
            code == ERROR_NOT_SUPPORTED as i32
                || code == ERROR_INVALID_FUNCTION as i32
                || (querying_file_id && code == ERROR_INVALID_PARAMETER as i32)
        }
        None => false,
    }
}

/// Calls one information class with a correctly sized output value.
///
/// # Type Parameters
///
/// * `T` is the default-initializable output structure required by the
///   information class.
///
/// # Parameters
///
/// * `query` is the native operation to invoke.
/// * `handle` is a live file handle borrowed from its owner.
/// * `class` selects the information structure requested from the system.
/// * `output` is writable storage for a value of the selected structure type.
///
/// # Returns
///
/// Returns `Ok(())` when the native query succeeds.
///
/// # Errors
///
/// Returns the operating system's last error when the native query fails.
fn query_info<T: Default>(
    query: NativeQuery,
    handle: HANDLE,
    class: FILE_INFO_BY_HANDLE_CLASS,
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
