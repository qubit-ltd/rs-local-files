// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Windows handle-relative rooted directory operations.
// qubit-style: allow source-test-pair

use std::ffi::OsString;
use std::fs::File;
use std::io::ErrorKind;
use std::io::Result;
use std::path::Path;

use windows_sys::Wdk::Storage::FileSystem::FILE_CREATE;
use windows_sys::Wdk::Storage::FileSystem::FILE_DIRECTORY_FILE;
use windows_sys::Wdk::Storage::FileSystem::FILE_OPEN;
use windows_sys::Wdk::Storage::FileSystem::FILE_OPEN_IF;
use windows_sys::Win32::Storage::FileSystem::DELETE;
use windows_sys::Win32::Storage::FileSystem::FILE_LIST_DIRECTORY;
use windows_sys::Win32::Storage::FileSystem::FILE_READ_ATTRIBUTES;
use windows_sys::Win32::Storage::FileSystem::SYNCHRONIZE;

use super::handle::open_entry;
use super::handle::open_entry_no_follow;
use super::namespace_mutation::delete_open_entry;
use super::native::nt_open_at;
use super::native::verify_real_directory;
use crate::local::LocalRelativePath;
use crate::local::internal::rooted_directory_reader::RootedDirectoryReader;

/// Opens a lazy reader for immediate children of the opened root.
///
/// Returns an I/O error when the root handle cannot be duplicated.
pub(crate) fn open_root_directory_reader(root: &File, _diagnostic_root: &Path) -> Result<RootedDirectoryReader> {
    root.try_clone().map(RootedDirectoryReader::new)
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
