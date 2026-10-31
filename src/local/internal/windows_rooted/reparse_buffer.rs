// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::fs::File;
use std::io::Error;
use std::io::Result;
use std::mem::size_of;
use std::os::windows::io::AsRawHandle;
use std::ptr::null;
use std::ptr::null_mut;

use windows_sys::Win32::Storage::FileSystem::MAXIMUM_REPARSE_DATA_BUFFER_SIZE;
use windows_sys::Win32::System::IO::DeviceIoControl;
use windows_sys::Win32::System::Ioctl::FSCTL_GET_REPARSE_POINT;

use super::symlink::corrupt_reparse;

/// Aligned storage and the initialized byte length returned by the native
/// query.
pub(crate) struct ReparseBuffer {
    /// Word-aligned allocation keeps the native reparse structure suitably
    /// aligned.
    storage: Vec<usize>,
    /// Number of initialized bytes reported by the successful native call.
    length: usize,
}

impl ReparseBuffer {
    /// Borrows exactly the initialized native record bytes.
    ///
    /// The slice cannot outlive this buffer's allocation; its length excludes
    /// unused capacity in the aligned workspace.
    pub(crate) fn as_bytes(&self) -> &[u8] {
        // SAFETY: storage is initialized, aligned native workspace; `length`
        // was checked against the native maximum and DeviceIoControl wrote it.
        unsafe { std::slice::from_raw_parts(self.storage.as_ptr().cast::<u8>(), self.length) }
    }
}

/// Retrieves the opaque reparse record for an already-opened link handle.
///
/// # Errors
/// Returns the native query error, or invalid data if the reported length
/// exceeds the allocated native buffer.
pub(crate) fn read_reparse_buffer(link: &File) -> Result<ReparseBuffer> {
    let mut storage = vec![0_usize; (MAXIMUM_REPARSE_DATA_BUFFER_SIZE as usize).div_ceil(size_of::<usize>())];
    let mut returned = 0_u32;
    // SAFETY: `link` remains open, the output allocation is writable for the
    // advertised capacity, and this synchronous call retains no pointers.
    let result = unsafe {
        DeviceIoControl(
            link.as_raw_handle(),
            FSCTL_GET_REPARSE_POINT,
            null(),
            0,
            storage.as_mut_ptr().cast(),
            MAXIMUM_REPARSE_DATA_BUFFER_SIZE,
            &raw mut returned,
            null_mut(),
        )
    };
    if result == 0 {
        return Err(Error::last_os_error());
    }
    let returned = returned as usize;
    if returned > MAXIMUM_REPARSE_DATA_BUFFER_SIZE as usize {
        return Err(corrupt_reparse("reparse query returned an oversized buffer"));
    }
    Ok(ReparseBuffer {
        storage,
        length: returned,
    })
}
// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
