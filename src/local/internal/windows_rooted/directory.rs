// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Windows handle-relative rooted directory operations.

use std::ffi::OsString;
use std::fs::File;
use std::io::ErrorKind;
use std::io::Result;
use std::os::windows::io::AsRawHandle;
use std::os::windows::io::FromRawHandle;
use std::path::Path;

use windows_sys::Wdk::Storage::FileSystem::FILE_CREATE;
use windows_sys::Wdk::Storage::FileSystem::FILE_DIRECTORY_FILE;
use windows_sys::Wdk::Storage::FileSystem::FILE_OPEN;
use windows_sys::Wdk::Storage::FileSystem::FILE_OPEN_IF;
use windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE;
use windows_sys::Win32::Storage::FileSystem::DELETE;
use windows_sys::Win32::Storage::FileSystem::FILE_FLAG_BACKUP_SEMANTICS;
use windows_sys::Win32::Storage::FileSystem::FILE_LIST_DIRECTORY;
use windows_sys::Win32::Storage::FileSystem::FILE_READ_ATTRIBUTES;
use windows_sys::Win32::Storage::FileSystem::ReOpenFile;
use windows_sys::Win32::Storage::FileSystem::SYNCHRONIZE;

use super::handle::open_entry;
use super::handle::open_entry_no_follow;
use super::namespace_mutation::delete_open_entry;
use super::namespace_mutation::delete_owned_read_only_entry;
use super::native::nt_open_at;
use super::native::verify_real_directory;
use crate::local::LocalRelativePath;
use crate::local::internal::rooted_directory_reader::RootedDirectoryReader;
use crate::rooted::EntryKind;
use crate::rooted::Metadata;

/// Opens a lazy reader for immediate children of the opened root.
///
/// # Errors
///
/// Returns an I/O error when an independent enumeration handle cannot be
/// reopened from the retained root authority.
pub(crate) fn open_root_directory_reader(root: &File, _diagnostic_root: &Path) -> Result<RootedDirectoryReader> {
    // SAFETY: `root` owns a live CreateFileW handle and ReOpenFile does not
    // retain the handle value. The returned handle is uniquely owned here.
    let handle = unsafe {
        ReOpenFile(
            root.as_raw_handle(),
            FILE_LIST_DIRECTORY | FILE_READ_ATTRIBUTES | SYNCHRONIZE,
            super::native::ROOTED_SHARE_MODE,
            FILE_FLAG_BACKUP_SEMANTICS,
        )
    };
    if handle == INVALID_HANDLE_VALUE {
        return Err(std::io::Error::last_os_error());
    }
    // SAFETY: ReOpenFile succeeded and returned a uniquely owned handle.
    let directory = unsafe { File::from_raw_handle(handle) };
    verify_real_directory(&directory)?;
    Ok(RootedDirectoryReader::new(directory))
}

/// Lists immediate children of a rooted descendant directory.
///
/// # Errors
///
/// Returns an I/O error when traversal, enumeration, or child inspection
/// fails.
pub(crate) fn read_rooted_directory(
    root: &File,
    diagnostic_root: &Path,
    path: &LocalRelativePath,
) -> Result<Vec<(OsString, File)>> {
    let directory = open_entry(
        root,
        path,
        FILE_LIST_DIRECTORY | FILE_READ_ATTRIBUTES | SYNCHRONIZE,
        FILE_OPEN,
        FILE_DIRECTORY_FILE,
    )?;
    verify_real_directory(&directory)?;
    read_directory_handle(&directory, diagnostic_root)
}

/// Opens a lazy reader for immediate children of a rooted descendant.
///
/// # Errors
///
/// Returns an I/O error when secure traversal or directory opening fails.
pub(crate) fn open_rooted_directory_reader(
    root: &File,
    _diagnostic_root: &Path,
    path: &LocalRelativePath,
) -> Result<RootedDirectoryReader> {
    let directory = open_entry(
        root,
        path,
        FILE_LIST_DIRECTORY | FILE_READ_ATTRIBUTES | SYNCHRONIZE,
        FILE_OPEN,
        FILE_DIRECTORY_FILE,
    )?;
    verify_real_directory(&directory)?;
    Ok(RootedDirectoryReader::new(directory))
}

/// Creates one rooted directory or directory chain.
/// `recursive` allows creation of missing intermediate directories;
/// `exists_ok` accepts an existing final directory. Earlier creations remain
/// if a later component fails.
///
/// # Errors
///
/// Returns an I/O error when secure traversal or creation fails.
pub(crate) fn create_rooted_directory(
    root: &File,
    _diagnostic_root: &Path,
    path: &LocalRelativePath,
    recursive: bool,
    exists_ok: bool,
) -> Result<()> {
    let components: Vec<OsString> = path
        .as_path()
        .components()
        .map(|component| component.as_os_str().to_os_string())
        .collect();
    let mut parent = root.try_clone()?;
    for (index, component) in components.iter().enumerate() {
        let final_component = index + 1 == components.len();
        let disposition = if final_component {
            if exists_ok { FILE_OPEN_IF } else { FILE_CREATE }
        } else if recursive {
            FILE_OPEN_IF
        } else {
            FILE_OPEN
        };
        match nt_open_at(
            &parent,
            component,
            FILE_LIST_DIRECTORY | FILE_READ_ATTRIBUTES | SYNCHRONIZE,
            disposition,
            FILE_DIRECTORY_FILE,
        ) {
            Ok(directory) => {
                verify_real_directory(&directory)?;
                parent = directory;
            }
            Err(source_error) if !recursive && !final_component && source_error.kind() == ErrorKind::NotFound => {
                return Err(source_error);
            }
            Err(source_error) => return Err(source_error),
        }
    }
    Ok(())
}

/// Removes one rooted entry or directory tree without following reparse points.
/// Recursive callers use the shared deletion scheduler before this boundary.
/// Direct removal requires a leaf or an empty directory.
///
/// # Errors
///
/// Returns an I/O error when traversal, enumeration, or handle deletion fails.
pub(crate) fn remove_rooted_entry(root: &File, _diagnostic_root: &Path, path: &LocalRelativePath) -> Result<()> {
    delete_rooted_entry(root, path)
}

/// Removes one no-follow handle after checking the requested entry type.
pub(crate) fn remove_rooted_entry_typed(
    root: &File,
    _diagnostic_root: &Path,
    path: &LocalRelativePath,
    directory: bool,
) -> Result<()> {
    let entry = open_entry_no_follow(root, path, DELETE | FILE_READ_ATTRIBUTES | SYNCHRONIZE, FILE_OPEN, 0)?;
    let kind = Metadata::from_open_file(&entry)?.kind();
    let mismatch = match (directory, kind) {
        (true, EntryKind::Directory) | (false, EntryKind::File | EntryKind::Symlink) => None,
        (true, _) => Some((
            ErrorKind::NotADirectory,
            "rooted empty-directory removal requires a real directory",
        )),
        (false, EntryKind::Directory) => Some((
            ErrorKind::IsADirectory,
            "rooted file removal does not remove directories",
        )),
        (false, EntryKind::Other) => Some((
            ErrorKind::InvalidInput,
            "rooted file removal rejects unsupported entry types",
        )),
    };
    if let Some((kind, message)) = mismatch {
        return Err(std::io::Error::new(kind, message));
    }
    delete_open_entry(&entry)
}

/// Removes one owned rooted file only when its opened entry retains the
/// creation-time identity.
pub(crate) fn remove_rooted_owned_read_only_file(
    root: &File,
    path: &LocalRelativePath,
    expected: &Metadata,
) -> Result<()> {
    let entry = open_entry_no_follow(
        root,
        path,
        DELETE | FILE_READ_ATTRIBUTES | windows_sys::Win32::Storage::FileSystem::FILE_WRITE_ATTRIBUTES | SYNCHRONIZE,
        FILE_OPEN,
        0,
    )?;
    if Metadata::from_open_file(&entry)?.kind() != EntryKind::File {
        return Err(ErrorKind::InvalidInput.into());
    }
    delete_owned_read_only_entry(&entry, expected)
}

/// Opens and deletes one rooted entry without following a reparse point.
///
/// # Errors
///
/// Returns an I/O error when the entry cannot be opened or deleted.
#[inline]
fn delete_rooted_entry(root: &File, path: &LocalRelativePath) -> Result<()> {
    let entry = open_entry_no_follow(root, path, DELETE | FILE_READ_ATTRIBUTES | SYNCHRONIZE, FILE_OPEN, 0)?;
    delete_open_entry(&entry)
}

/// Enumerates one already opened directory with `NtQueryDirectoryFile`.
/// Returns an eager list sorted by native name, retaining a handle per child.
/// Propagates handle duplication, native enumeration, and child-open errors.
fn read_directory_handle(directory: &File, _diagnostic_root: &Path) -> Result<Vec<(OsString, File)>> {
    let mut entries = Vec::new();
    let mut reader = RootedDirectoryReader::new(directory.try_clone()?);
    while let Some(entry) = reader.next_entry()? {
        entries.push(entry);
    }
    entries.sort_by(|(left_name, _), (right_name, _)| left_name.cmp(right_name));
    Ok(entries)
}
