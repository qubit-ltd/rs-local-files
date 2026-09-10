// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! High-level rooted Windows file-opening workflows.
// qubit-style: allow source-test-pair

use std::fs::File;
use std::io::Result;
use std::path::Path;

use windows_sys::Wdk::Storage::FileSystem::FILE_CREATE;
use windows_sys::Wdk::Storage::FileSystem::FILE_NON_DIRECTORY_FILE;
use windows_sys::Wdk::Storage::FileSystem::FILE_OPEN;
use windows_sys::Wdk::Storage::FileSystem::FILE_OPEN_IF;
use windows_sys::Wdk::Storage::FileSystem::FILE_OVERWRITE_IF;
use windows_sys::Win32::Foundation::GENERIC_WRITE;
use windows_sys::Win32::Storage::FileSystem::FILE_APPEND_DATA;
use windows_sys::Win32::Storage::FileSystem::FILE_READ_ATTRIBUTES;
use windows_sys::Win32::Storage::FileSystem::FILE_WRITE_ATTRIBUTES;
use windows_sys::Win32::Storage::FileSystem::SYNCHRONIZE;

use super::directory::create_rooted_directory;
use super::handle::open_entry;
use super::native::verify_not_name_surrogate;
use crate::local::LocalRelativePath;
use crate::write;

/// Opens a rooted regular file for writing.
///
/// # Errors
///
/// Returns an I/O error when parent traversal, creation, or final verification
/// fails.
pub(crate) fn open_rooted_native_writer(
    root: &File,
    _diagnostic_root: &Path,
    path: &LocalRelativePath,
    options: &write::OpenOptions,
) -> Result<File> {
    if options.creates_parents()
        && let Some(parent) = path.as_path().parent().filter(|parent| !parent.as_os_str().is_empty())
    {
        create_rooted_directory(root, Path::new(""), &LocalRelativePath::new(parent)?, true, true)?;
    }
    let (access, disposition) = match options.mode() {
        write::Mode::CreateOrTruncate => (GENERIC_WRITE, FILE_OVERWRITE_IF),
        write::Mode::CreateNew => (GENERIC_WRITE, FILE_CREATE),
        write::Mode::OpenExistingAtStart => (GENERIC_WRITE, FILE_OPEN),
        write::Mode::AppendExisting => (FILE_APPEND_DATA, FILE_OPEN),
        write::Mode::AppendOrCreate => (FILE_APPEND_DATA, FILE_OPEN_IF),
    };
    let file = open_entry(
        root,
        path,
        access | FILE_READ_ATTRIBUTES | FILE_WRITE_ATTRIBUTES | SYNCHRONIZE,
        disposition,
        FILE_NON_DIRECTORY_FILE,
    )?;
    verify_not_name_surrogate(&file)?;
    Ok(file)
}
