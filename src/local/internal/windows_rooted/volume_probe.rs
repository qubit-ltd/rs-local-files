// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Filesystem observations queried from an already-opened Windows handle.

use std::fs::File;
use std::io::Error;
use std::io::ErrorKind;
use std::io::Result;
use std::mem::size_of;
use std::os::windows::io::AsRawHandle;

use windows_sys::Wdk::Storage::FileSystem::FILE_FS_ATTRIBUTE_INFORMATION;
use windows_sys::Wdk::Storage::FileSystem::FileFsAttributeInformation;
use windows_sys::Wdk::Storage::FileSystem::FileFsFullSizeInformation;
use windows_sys::Wdk::Storage::FileSystem::NtQueryVolumeInformationFile;
use windows_sys::Wdk::Storage::FileSystem::RtlNtStatusToDosErrorNoTeb;
use windows_sys::Wdk::System::SystemServices::FILE_FS_FULL_SIZE_INFORMATION;
use windows_sys::Win32::System::IO::IO_STATUS_BLOCK;

use crate::capability::LocalFileSystemLimits;
use crate::capability::LocalFileSystemSpace;
use crate::capability::LocalPathLengthUnit;
use crate::capability::SizeLimit;

/// Queries native path limits for the volume containing `file`.
///
/// Windows reports component capacity in UTF-16 code units. No fixed complete
/// path limit is claimed for handle-relative NT traversal.
///
/// # Errors
///
/// Returns the native volume-information error when the opened handle cannot
/// provide filesystem attributes.
pub(crate) fn probe_windows_limits(file: &File) -> Result<LocalFileSystemLimits> {
    const BUFFER_BYTES: usize = 4096;
    let mut storage = vec![0_usize; BUFFER_BYTES.div_ceil(size_of::<usize>())];
    query_volume_information(
        file,
        storage.as_mut_ptr().cast(),
        BUFFER_BYTES,
        FileFsAttributeInformation,
    )?;
    // SAFETY: the successful native query initialized at least the fixed
    // FILE_FS_ATTRIBUTE_INFORMATION header in aligned storage.
    let attributes = unsafe { &*storage.as_ptr().cast::<FILE_FS_ATTRIBUTE_INFORMATION>() };
    let component = u64::try_from(attributes.MaximumComponentNameLength).map_or(SizeLimit::Unknown, SizeLimit::Maximum);
    Ok(LocalFileSystemLimits::new(
        SizeLimit::Unknown,
        component,
        LocalPathLengthUnit::Utf16CodeUnits,
    ))
}

/// Queries total, free, and caller-available capacity from an opened handle.
///
/// # Errors
///
/// Returns the native volume-information error when the capacity query fails.
pub(crate) fn probe_windows_space(file: &File) -> Result<LocalFileSystemSpace> {
    let mut information = FILE_FS_FULL_SIZE_INFORMATION::default();
    query_volume_information(
        file,
        (&raw mut information).cast(),
        size_of::<FILE_FS_FULL_SIZE_INFORMATION>(),
        FileFsFullSizeInformation,
    )?;
    let bytes_per_allocation_unit =
        u64::from(information.SectorsPerAllocationUnit).checked_mul(u64::from(information.BytesPerSector));
    let bytes = |units| allocation_bytes(units, bytes_per_allocation_unit);
    Ok(LocalFileSystemSpace::new(
        bytes(information.TotalAllocationUnits),
        bytes(information.ActualAvailableAllocationUnits),
        bytes(information.CallerAvailableAllocationUnits),
    ))
}

/// Runs one synchronous `NtQueryVolumeInformationFile` request.
fn query_volume_information(
    file: &File,
    output: *mut core::ffi::c_void,
    output_length: usize,
    information_class: i32,
) -> Result<()> {
    let output_length = u32::try_from(output_length)
        .map_err(|_| Error::new(ErrorKind::InvalidInput, "volume query buffer is too large"))?;
    let mut status_block = IO_STATUS_BLOCK::default();
    // SAFETY: `file` remains open, `output` is writable for `output_length`,
    // and the synchronous query retains no pointers.
    let status = unsafe {
        NtQueryVolumeInformationFile(
            file.as_raw_handle(),
            &raw mut status_block,
            output,
            output_length,
            information_class,
        )
    };
    if status >= 0 {
        return Ok(());
    }
    // SAFETY: conversion accepts every NTSTATUS value and retains no pointers.
    let code = unsafe { RtlNtStatusToDosErrorNoTeb(status) };
    Err(Error::from_raw_os_error(code as i32))
}

/// Converts signed allocation units into bytes without overflow.
fn allocation_bytes(units: i64, bytes_per_allocation_unit: Option<u64>) -> Option<u64> {
    let units = u64::try_from(units).ok()?;
    let unit_bytes = bytes_per_allocation_unit?;
    u64::try_from(u128::from(units).checked_mul(u128::from(unit_bytes))?).ok()
}

#[cfg(test)]
mod tests {
    use super::allocation_bytes;

    /// Verifies invalid native counts become unavailable dimensions.
    #[test]
    fn test_allocation_bytes_rejects_negative_and_overflowing_values() {
        assert_eq!(None, allocation_bytes(-1, Some(4096)));
        assert_eq!(None, allocation_bytes(i64::MAX, Some(u64::MAX)));
    }

    /// Verifies ordinary allocation counts use checked multiplication.
    #[test]
    fn test_allocation_bytes_converts_valid_counts() {
        assert_eq!(Some(4096), allocation_bytes(2, Some(2048)));
    }
}
