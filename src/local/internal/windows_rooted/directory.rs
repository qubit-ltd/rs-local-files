// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Windows handle-relative rooted directory operations.

use std::ffi::OsStr;
use std::ffi::OsString;
use std::fs::File;
use std::io::Error;
use std::io::ErrorKind;
use std::io::Result;
use std::os::windows::io::AsRawHandle;
use std::os::windows::io::FromRawHandle;
use std::path::Path;
use std::ptr::null;
use std::ptr::null_mut;

use windows_sys::Wdk::Storage::FileSystem::FILE_CREATE;
use windows_sys::Wdk::Storage::FileSystem::FILE_DIRECTORY_FILE;
use windows_sys::Wdk::Storage::FileSystem::FILE_NON_DIRECTORY_FILE;
use windows_sys::Wdk::Storage::FileSystem::FILE_OPEN;
use windows_sys::Wdk::Storage::FileSystem::FILE_OPEN_IF;
use windows_sys::Win32::Foundation::GENERIC_READ;
use windows_sys::Win32::Foundation::GENERIC_WRITE;
use windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE;
use windows_sys::Win32::Storage::FileSystem::CreateFileW;
use windows_sys::Win32::Storage::FileSystem::DELETE;
use windows_sys::Win32::Storage::FileSystem::FILE_ADD_FILE;
use windows_sys::Win32::Storage::FileSystem::FILE_ADD_SUBDIRECTORY;
use windows_sys::Win32::Storage::FileSystem::FILE_DELETE_CHILD;
use windows_sys::Win32::Storage::FileSystem::FILE_FLAG_BACKUP_SEMANTICS;
use windows_sys::Win32::Storage::FileSystem::FILE_LIST_DIRECTORY;
use windows_sys::Win32::Storage::FileSystem::FILE_NAME_NORMALIZED;
use windows_sys::Win32::Storage::FileSystem::FILE_READ_ATTRIBUTES;
use windows_sys::Win32::Storage::FileSystem::FILE_TRAVERSE;
use windows_sys::Win32::Storage::FileSystem::FILE_WRITE_ATTRIBUTES;
use windows_sys::Win32::Storage::FileSystem::GetFinalPathNameByHandleW;
use windows_sys::Win32::Storage::FileSystem::OPEN_EXISTING;
use windows_sys::Win32::Storage::FileSystem::SYNCHRONIZE;
use windows_sys::Win32::Storage::FileSystem::VOLUME_NAME_DOS;

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
use crate::temp::types::NativePrivateCreateError;

/// Creates one temporary child using the inherited default Windows ACL.
///
/// # Parameters
///
/// - `parent` is the opened root directory that owns the new child.
/// - `name` is the single child name to create beneath `parent`.
/// - `directory` selects a directory (`true`) or regular file (`false`).
///
/// # Returns
///
/// Returns an open handle to the newly created child.
///
/// # Errors
/// Returns the native open/create error. `FILE_CREATE` creates the entry and
/// obtains its handle in one operation, so a failed call has no retained
/// partial entry.
pub(crate) fn create_rooted_temp_child(
    parent: &File,
    name: &OsStr,
    directory: bool,
) -> std::result::Result<File, NativePrivateCreateError> {
    let access = if directory {
        FILE_LIST_DIRECTORY
            | FILE_READ_ATTRIBUTES
            | FILE_TRAVERSE
            | FILE_ADD_FILE
            | FILE_ADD_SUBDIRECTORY
            | FILE_DELETE_CHILD
            | DELETE
            | SYNCHRONIZE
    } else {
        GENERIC_READ | GENERIC_WRITE | FILE_READ_ATTRIBUTES | DELETE | SYNCHRONIZE
    };
    nt_open_at(
        parent,
        name,
        access,
        FILE_CREATE,
        if directory {
            FILE_DIRECTORY_FILE
        } else {
            FILE_NON_DIRECTORY_FILE
        },
    )
    .map_err(|source| NativePrivateCreateError { source, retained: None })
}

/// Opens a lazy reader for immediate children of the opened root.
///
/// # Parameters
///
/// - `root` is the opened directory handle used to enumerate immediate
///   children.
/// - `_diagnostic_root` is unused by this handle-relative backend.
///
/// # Returns
///
/// Returns a lazy reader backed by an independent enumeration handle.
///
/// # Errors
///
/// Reopens an independent enumeration handle when the path and access policy
/// permit; otherwise reports that independent enumeration is unavailable.
///
/// Returns an I/O error when final-path inspection, metadata validation, or
/// handle duplication fails unexpectedly.
pub(crate) fn open_root_directory_reader(root: &File, _diagnostic_root: &Path) -> Result<RootedDirectoryReader> {
    open_root_directory_reader_with_reopen(root, open_independent_root_directory)
}

/// Opens a root enumerator, allowing tests to force the handle-reopen fallback.
///
/// `reopen` attempts to obtain a distinct file object for enumeration. If it
/// returns `None`, this function returns `Unsupported`: cloning a Windows
/// directory handle would share its native enumeration cursor.
///
/// # Type Parameters
///
/// - `F` is a one-shot operation that attempts to reopen `root` independently.
///
/// # Parameters
///
/// - `root` is the directory handle passed to `reopen`.
/// - `reopen` attempts to produce an independent enumeration handle.
///
/// # Returns
///
/// Returns a lazy reader backed by the independent handle.
///
/// # Errors
///
/// Propagates reopen errors, returns `Unsupported` when no independent handle
/// is available, and returns an error if the resulting handle is not a real
/// directory.
fn open_root_directory_reader_with_reopen<F>(root: &File, reopen: F) -> Result<RootedDirectoryReader>
where
    F: FnOnce(&File) -> Result<Option<File>>,
{
    let directory = reopen(root)?.ok_or_else(|| {
        Error::new(
            ErrorKind::Unsupported,
            "cannot open an independent Windows directory enumeration handle",
        )
    })?;
    verify_real_directory(&directory)?;
    Ok(RootedDirectoryReader::new(directory))
}

#[cfg(test)]
mod fallback_tests;

/// Opens the root's normalized final path as a new file object.
///
/// # Parameters
///
/// - `root` supplies the identity and path used to reopen the directory.
///
/// # Returns
///
/// Returns `None` if the path is unavailable or resolves to a different file;
/// otherwise returns the independently opened directory handle.
///
/// # Errors
///
/// Propagates native path lookup and metadata errors other than disappearance,
/// identity change, and access denial, which return `None`.
fn open_independent_root_directory(root: &File) -> Result<Option<File>> {
    let mut path = vec![0_u16; 256];
    loop {
        let capacity = u32::try_from(path.len())
            .map_err(|_| Error::new(ErrorKind::InvalidInput, "rooted directory path is too long"))?;
        // SAFETY: `root` is a live handle and `path` is writable for
        // `capacity` UTF-16 units, including the required NUL terminator.
        let length = unsafe {
            GetFinalPathNameByHandleW(
                root.as_raw_handle(),
                path.as_mut_ptr(),
                capacity,
                FILE_NAME_NORMALIZED | VOLUME_NAME_DOS,
            )
        };
        if length == 0 {
            let error = Error::last_os_error();
            return if matches!(error.kind(), ErrorKind::NotFound | ErrorKind::PermissionDenied) {
                Ok(None)
            } else {
                Err(error)
            };
        }
        let length = length as usize;
        if length < path.len() {
            path.truncate(length + 1);
            break;
        }
        let required = length
            .checked_add(1)
            .ok_or_else(|| Error::new(ErrorKind::InvalidInput, "rooted directory path length overflow"))?;
        path.try_reserve_exact(required - path.len())
            .map_err(|error| Error::new(ErrorKind::OutOfMemory, error))?;
        path.resize(required, 0);
    }

    // SAFETY: `path` is a live NUL-terminated UTF-16 path obtained from the
    // retained handle. The returned handle is validated before ownership.
    let handle = unsafe {
        CreateFileW(
            path.as_ptr(),
            FILE_LIST_DIRECTORY | FILE_READ_ATTRIBUTES | FILE_TRAVERSE | SYNCHRONIZE,
            super::native::ROOTED_SHARE_MODE,
            null(),
            OPEN_EXISTING,
            FILE_FLAG_BACKUP_SEMANTICS,
            null_mut(),
        )
    };
    if handle == INVALID_HANDLE_VALUE {
        let error = Error::last_os_error();
        return if matches!(error.kind(), ErrorKind::NotFound | ErrorKind::PermissionDenied) {
            Ok(None)
        } else {
            Err(error)
        };
    }
    // SAFETY: CreateFileW succeeded and returned a uniquely owned handle.
    let directory = unsafe { File::from_raw_handle(handle) };
    if !Metadata::from_open_file(root)?.is_same_file(&Metadata::from_open_file(&directory)?) {
        return Ok(None);
    }
    Ok(Some(directory))
}

/// Lists immediate children of a rooted descendant directory.
///
/// # Parameters
///
/// - `root` is the opened root capability used for secure traversal.
/// - `diagnostic_root` is retained for the backend interface; enumeration uses
///   only the opened directory handle and does not inspect this path.
/// - `path` is the descendant directory to enumerate relative to `root`.
///
/// # Returns
///
/// Returns the immediate children in native-name order, each with an open
/// handle.
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
/// # Parameters
///
/// - `root` is the opened root capability used for secure traversal.
/// - `_diagnostic_root` is unused by this handle-relative backend.
/// - `path` is the descendant directory to enumerate relative to `root`.
///
/// # Returns
///
/// Returns a lazy reader for the descendant's immediate children.
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
/// # Parameters
///
/// - `root` is the opened root capability used for secure traversal.
/// - `_diagnostic_root` is unused by this handle-relative backend.
/// - `path` identifies the directory path to create beneath `root`.
/// - `recursive` permits creating missing intermediate directories.
/// - `exists_ok` permits the final directory to already exist.
///
/// # Returns
///
/// Returns `()` after all path components have been processed.
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
/// # Parameters
///
/// - `root` is the opened root capability used for secure traversal.
/// - `_diagnostic_root` is unused by this handle-relative backend.
/// - `path` identifies the entry to remove beneath `root`.
///
/// # Returns
///
/// Returns `()` after the entry or directory tree has been removed.
///
/// # Errors
///
/// Returns an I/O error when traversal, enumeration, or handle deletion fails.
pub(crate) fn remove_rooted_entry(root: &File, _diagnostic_root: &Path, path: &LocalRelativePath) -> Result<()> {
    delete_rooted_entry(root, path)
}

/// Removes a rooted entry only when the current no-follow name still has the
/// identity observed through the caller's retained handle.
///
/// # Parameters
///
/// - `root` is the opened root capability used for secure traversal.
/// - `path` identifies the entry whose current identity is checked.
/// - `expected` is the retained identity that must match before deletion.
///
/// # Returns
///
/// Returns `true` when the matching entry was removed, or `false` when its
/// current identity differs from `expected`.
///
/// # Errors
///
/// Returns an I/O error when the entry cannot be opened, inspected, or deleted.
pub(crate) fn remove_rooted_entry_if_same_file(
    root: &File,
    path: &LocalRelativePath,
    expected: &Metadata,
) -> Result<bool> {
    let entry = open_entry_no_follow(root, path, DELETE | FILE_READ_ATTRIBUTES | SYNCHRONIZE, FILE_OPEN, 0)?;
    let actual = Metadata::from_open_file(&entry)?;
    if !expected.is_same_file(&actual) {
        return Ok(false);
    }
    delete_open_entry(&entry)?;
    Ok(true)
}

/// Removes one no-follow handle after checking the requested entry type.
///
/// # Parameters
///
/// - `root` is the opened root capability used for secure traversal.
/// - `_diagnostic_root` is unused by this handle-relative backend.
/// - `path` identifies the entry to remove beneath `root`.
/// - `directory` requires a directory when `true` and a non-directory file or
///   symlink when `false`.
///
/// # Returns
///
/// Returns `()` after the entry of the requested type has been removed.
///
/// # Errors
///
/// Returns an I/O error when the entry cannot be opened, its type does not
/// match `directory`, or deletion fails.
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
///
/// # Parameters
///
/// - `root` is the opened root capability used for secure traversal.
/// - `path` identifies the owned file beneath `root`.
/// - `expected` is the file identity recorded at creation time.
///
/// # Returns
///
/// Returns `()` after the matching owned file has been removed.
///
/// # Errors
///
/// Returns an I/O error when the entry cannot be opened, is not a regular file,
/// or fails the ownership check or removal.
pub(crate) fn remove_rooted_owned_read_only_file(
    root: &File,
    path: &LocalRelativePath,
    expected: &Metadata,
) -> Result<()> {
    let entry = open_entry_no_follow(
        root,
        path,
        DELETE | FILE_READ_ATTRIBUTES | FILE_WRITE_ATTRIBUTES | SYNCHRONIZE,
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
/// # Parameters
///
/// - `root` is the opened root capability used for secure traversal.
/// - `path` identifies the entry to delete beneath `root`.
///
/// # Returns
///
/// Returns `()` after the entry has been removed.
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
///
/// # Parameters
///
/// - `directory` is the already opened directory handle to enumerate.
/// - `_diagnostic_root` is unused by this handle-relative backend.
///
/// # Returns
///
/// Returns the immediate children in native-name order, each with an open
/// handle.
///
/// # Errors
///
/// Returns an I/O error when handle duplication, native enumeration, or child
/// opening fails.
fn read_directory_handle(directory: &File, _diagnostic_root: &Path) -> Result<Vec<(OsString, File)>> {
    let mut entries = Vec::new();
    let mut reader = RootedDirectoryReader::new(directory.try_clone()?);
    while let Some(entry) = reader.next_entry()? {
        entries.push(entry);
    }
    entries.sort_by(|(left_name, _), (right_name, _)| left_name.cmp(right_name));
    Ok(entries)
}
