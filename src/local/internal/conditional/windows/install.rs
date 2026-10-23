// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Windows native conditional publication operations.

use std::ffi::OsStr;
use std::io;
use std::os::windows::ffi::OsStrExt;
use std::path::Path;

use windows_sys::Win32::Storage::FileSystem::ReplaceFileW;

use super::namespace::Namespace;

/// Calls `ReplaceFileW` with a nonempty same-volume backup path.
pub(super) fn replace_with_backup(
    target: &Path,
    replacement: &Path,
    backup: &Path,
) -> io::Result<()> {
    let target = wide_path(target.as_os_str())?;
    let replacement = wide_path(replacement.as_os_str())?;
    let backup = wide_path(backup.as_os_str())?;
    // SAFETY: every path is a live, NUL-terminated UTF-16 buffer; all are
    // absolute paths in the same parent volume and remain live for the call.
    // Flags are zero: REPLACEFILE_WRITE_THROUGH is explicitly unsupported.
    let result = unsafe {
        ReplaceFileW(
            target.as_ptr(),
            replacement.as_ptr(),
            backup.as_ptr(),
            0,
            std::ptr::null(),
            std::ptr::null(),
        )
    };
    if result == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

/// Installs a candidate only when the destination name is absent.
pub(super) fn install_absent(namespace: &Namespace, candidate: &OsStr) -> io::Result<()> {
    if namespace.is_rooted() {
        namespace.rename(candidate, namespace.target_name(), false)
    } else {
        std::fs::hard_link(
            namespace.private_file_path(candidate)?,
            namespace.target_path(),
        )
    }
}

fn wide_path(value: &OsStr) -> io::Result<Vec<u16>> {
    let units: Vec<u16> = value.encode_wide().collect();
    if units.contains(&0) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "path contains an interior NUL",
        ));
    }
    Ok(units.into_iter().chain(Some(0)).collect())
}
