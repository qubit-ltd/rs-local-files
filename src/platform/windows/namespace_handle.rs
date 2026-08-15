// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Windows handle-relative namespace authority.

use std::ffi::OsStr;
use std::ffi::OsString;
use std::fs::File;
use std::io;
use std::io::ErrorKind;
use std::mem::size_of;
use std::os::windows::ffi::OsStrExt;
use std::os::windows::ffi::OsStringExt;
use std::os::windows::io::AsRawHandle;
use std::os::windows::io::FromRawHandle;
use std::path::Path;
use std::ptr::null;
use std::ptr::null_mut;

use windows_sys::Wdk::Foundation::OBJECT_ATTRIBUTES;
use windows_sys::Wdk::Storage::FileSystem::FILE_CREATE;
use windows_sys::Wdk::Storage::FileSystem::FILE_DIRECTORY_FILE;
use windows_sys::Wdk::Storage::FileSystem::FILE_NON_DIRECTORY_FILE;
use windows_sys::Wdk::Storage::FileSystem::FILE_OPEN;
use windows_sys::Wdk::Storage::FileSystem::FILE_OPEN_IF;
use windows_sys::Wdk::Storage::FileSystem::FILE_OPEN_REPARSE_POINT;
use windows_sys::Wdk::Storage::FileSystem::FILE_SYNCHRONOUS_IO_NONALERT;
use windows_sys::Wdk::Storage::FileSystem::NtCreateFile;
use windows_sys::Wdk::Storage::FileSystem::RtlNtStatusToDosErrorNoTeb;
use windows_sys::Win32::Foundation::GENERIC_READ;
use windows_sys::Win32::Foundation::GENERIC_WRITE;
use windows_sys::Win32::Foundation::HANDLE;
use windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE;
use windows_sys::Win32::Foundation::OBJ_CASE_INSENSITIVE;
use windows_sys::Win32::Foundation::UNICODE_STRING;
use windows_sys::Win32::Storage::FileSystem::CreateFileW;
use windows_sys::Win32::Storage::FileSystem::DELETE;
use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_DIRECTORY;
use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_NORMAL;
use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT;
use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_TAG_INFO;
use windows_sys::Win32::Storage::FileSystem::FILE_FLAG_BACKUP_SEMANTICS;
use windows_sys::Win32::Storage::FileSystem::FILE_FLAG_OPEN_REPARSE_POINT;
use windows_sys::Win32::Storage::FileSystem::FILE_LIST_DIRECTORY;
use windows_sys::Win32::Storage::FileSystem::FILE_NAME_NORMALIZED;
use windows_sys::Win32::Storage::FileSystem::FILE_READ_ATTRIBUTES;
use windows_sys::Win32::Storage::FileSystem::FILE_SHARE_DELETE;
use windows_sys::Win32::Storage::FileSystem::FILE_SHARE_READ;
use windows_sys::Win32::Storage::FileSystem::FILE_SHARE_WRITE;
use windows_sys::Win32::Storage::FileSystem::FILE_WRITE_ATTRIBUTES;
use windows_sys::Win32::Storage::FileSystem::FileAttributeTagInfo;
use windows_sys::Win32::Storage::FileSystem::GetFileInformationByHandleEx;
use windows_sys::Win32::Storage::FileSystem::GetFinalPathNameByHandleW;
use windows_sys::Win32::Storage::FileSystem::OPEN_EXISTING;
use windows_sys::Win32::Storage::FileSystem::SYNCHRONIZE;
use windows_sys::Win32::System::IO::IO_STATUS_BLOCK;

use super::DirectoryCursor;
use super::EntryIdentity;
use super::OpenedFile;
use super::StagedFile;
use super::filesystem_probe;
use crate::LocalFileError;
use crate::LocalFileErrorKind;
use crate::LocalFileMetadata;
use crate::LocalFileOperation;
use crate::LocalFileSystemLimits;
use crate::LocalFileSystemSpace;
use crate::LocalResult;
use crate::RelativePath;

/// Reparse-tag bit identifying name-surrogate entries.
const IO_REPARSE_TAG_NAME_SURROGATE: u32 = 0x2000_0000;
/// Share mode used by all synchronous relative opens.
const SHARE_MODE: u32 = FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE;

/// An opened Windows directory handle authorizing relative operations.
#[derive(Debug)]
#[must_use]
pub(crate) struct NamespaceHandle {
    /// Directory handle used as the sole namespace authority.
    pub(super) handle: File,
}

impl NamespaceHandle {
    /// Opens a root directory without following its final reparse point.
    ///
    /// # Errors
    ///
    /// Returns an open-root error when the path cannot be opened as a real
    /// directory or denotes a name-surrogate reparse point.
    pub(crate) fn open_root(path: &Path) -> LocalResult<Self> {
        let wide = wide_path(path).map_err(|error| {
            io_error(LocalFileOperation::OpenRoot, path, None, error)
        })?;
        // SAFETY: `wide` is a live NUL-terminated UTF-16 path, optional
        // pointers are null, and successful ownership is transferred to File.
        let raw = unsafe {
            CreateFileW(
                wide.as_ptr(),
                FILE_LIST_DIRECTORY | FILE_READ_ATTRIBUTES | SYNCHRONIZE,
                SHARE_MODE,
                null(),
                OPEN_EXISTING,
                FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
                null_mut(),
            )
        };
        if raw == INVALID_HANDLE_VALUE {
            return Err(io_error(
                LocalFileOperation::OpenRoot,
                path,
                None,
                io::Error::last_os_error(),
            ));
        }
        // SAFETY: successful CreateFileW returned a uniquely owned handle.
        let handle = unsafe { File::from_raw_handle(raw) };
        verify_real_directory(&handle).map_err(|error| {
            io_error(LocalFileOperation::OpenRoot, path, None, error)
        })?;
        Ok(Self { handle })
    }

    /// Duplicates this authority without resolving its original path again.
    ///
    /// # Errors
    ///
    /// Returns an open-root error when the handle cannot be duplicated.
    pub(crate) fn clone_handle(&self) -> LocalResult<Self> {
        self.handle
            .try_clone()
            .map(|handle| Self { handle })
            .map_err(|error| {
                io_error(
                    LocalFileOperation::OpenRoot,
                    Path::new(""),
                    None,
                    error,
                )
            })
    }

    /// Reads no-follow metadata for a validated relative path.
    ///
    /// # Errors
    ///
    /// Returns a metadata error when the entry cannot be opened or inspected.
    pub(crate) fn metadata(
        &self,
        path: &RelativePath,
    ) -> LocalResult<LocalFileMetadata> {
        let entry = self.open_for_metadata(path)?;
        entry
            .metadata()
            .map(|metadata| LocalFileMetadata::from_native(&metadata))
            .map_err(|error| {
                io_error(
                    LocalFileOperation::Metadata,
                    path.as_path(),
                    None,
                    error,
                )
            })
    }

    /// Reads the target stored in a final name-surrogate reparse point.
    ///
    /// The parent is first opened handle-relatively without following any
    /// intermediate reparse point.
    ///
    /// # Errors
    ///
    /// Returns a bind-path error when parent traversal, handle-path recovery,
    /// or native link reading fails.
    pub(crate) fn read_link(
        &self,
        path: &RelativePath,
    ) -> LocalResult<std::path::PathBuf> {
        let (parent, name) =
            open_parent(&self.handle, path).map_err(|error| {
                io_error(
                    LocalFileOperation::BindPath,
                    path.as_path(),
                    None,
                    error,
                )
            })?;
        let parent = handle_path(&parent).map_err(|error| {
            io_error(LocalFileOperation::BindPath, path.as_path(), None, error)
        })?;
        std::fs::read_link(parent.join(name)).map_err(|error| {
            io_error(LocalFileOperation::BindPath, path.as_path(), None, error)
        })
    }

    /// Opens a regular file while rejecting name-surrogate reparse points.
    ///
    /// # Errors
    ///
    /// Returns an open-reader error when traversal, opening, verification,
    /// metadata, or identity capture fails.
    pub(crate) fn open_reader(
        &self,
        path: &RelativePath,
    ) -> LocalResult<OpenedFile> {
        let file = open_entry(
            &self.handle,
            path,
            GENERIC_READ | FILE_READ_ATTRIBUTES | SYNCHRONIZE,
            FILE_OPEN,
            FILE_NON_DIRECTORY_FILE,
        )
        .map_err(|error| {
            io_error(
                LocalFileOperation::OpenReader,
                path.as_path(),
                None,
                error,
            )
        })?;
        let metadata = file.metadata().map_err(|error| {
            io_error(
                LocalFileOperation::OpenReader,
                path.as_path(),
                None,
                error,
            )
        })?;
        if !metadata.is_file() {
            return Err(LocalFileError::new(
                LocalFileErrorKind::TypeConflict,
                LocalFileOperation::OpenReader,
            )
            .with_path(path.as_path().to_path_buf())
            .with_reason("the opened entry is not a regular file"));
        }
        let identity = EntryIdentity::from_file(&file).map_err(|error| {
            io_error(
                LocalFileOperation::OpenReader,
                path.as_path(),
                None,
                error,
            )
        })?;
        Ok(OpenedFile::new(
            file,
            LocalFileMetadata::from_native(&metadata),
            identity,
        ))
    }

    /// Opens a lazy cursor for a relative directory.
    ///
    /// # Errors
    ///
    /// Returns a list error when traversal or directory verification fails.
    pub(crate) fn open_directory(
        &self,
        path: &RelativePath,
    ) -> LocalResult<DirectoryCursor> {
        let directory = if path.as_path().as_os_str().is_empty() {
            self.handle.try_clone().map_err(|error| {
                io_error(LocalFileOperation::List, path.as_path(), None, error)
            })?
        } else {
            open_entry(
                &self.handle,
                path,
                FILE_LIST_DIRECTORY | FILE_READ_ATTRIBUTES | SYNCHRONIZE,
                FILE_OPEN,
                FILE_DIRECTORY_FILE,
            )
            .map_err(|error| {
                io_error(LocalFileOperation::List, path.as_path(), None, error)
            })?
        };
        verify_real_directory(&directory).map_err(|error| {
            io_error(LocalFileOperation::List, path.as_path(), None, error)
        })?;
        Ok(DirectoryCursor::new(
            directory,
            path.as_path().to_path_buf(),
        ))
    }

    /// Creates a directory and accepts an existing real directory.
    ///
    /// # Errors
    ///
    /// Returns a create-directory error when traversal or creation fails.
    pub(crate) fn create_directory(
        &self,
        path: &RelativePath,
    ) -> LocalResult<()> {
        self.create_directory_impl(path, FILE_OPEN_IF)
    }

    /// Creates a directory only when the final entry does not exist.
    ///
    /// # Errors
    ///
    /// Returns a create-directory error when traversal or exclusive creation
    /// fails.
    pub(crate) fn create_directory_new(
        &self,
        path: &RelativePath,
    ) -> LocalResult<()> {
        self.create_directory_impl(path, FILE_CREATE)
    }

    /// Creates and opens a new private regular file.
    ///
    /// # Errors
    ///
    /// Returns an open-writer error when traversal or exclusive creation fails.
    pub(crate) fn create_file_new(
        &self,
        path: &RelativePath,
    ) -> LocalResult<File> {
        open_entry(
            &self.handle,
            path,
            GENERIC_READ
                | GENERIC_WRITE
                | DELETE
                | FILE_READ_ATTRIBUTES
                | FILE_WRITE_ATTRIBUTES
                | SYNCHRONIZE,
            FILE_CREATE,
            FILE_NON_DIRECTORY_FILE,
        )
        .map_err(|error| {
            io_error(
                LocalFileOperation::OpenWriter,
                path.as_path(),
                None,
                error,
            )
        })
    }

    /// Deletes a non-directory final entry without following it.
    ///
    /// # Errors
    ///
    /// Returns a delete-file error when opening, type checking, or deletion
    /// fails.
    pub(crate) fn delete_file(&self, path: &RelativePath) -> LocalResult<()> {
        self.delete(path, false, LocalFileOperation::DeleteFile)
    }

    /// Deletes an empty directory without following its final component.
    ///
    /// # Errors
    ///
    /// Returns a delete-directory error when opening, type checking, or
    /// deletion fails.
    pub(crate) fn delete_directory(
        &self,
        path: &RelativePath,
    ) -> LocalResult<()> {
        self.delete(path, true, LocalFileOperation::DeleteDirectory)
    }

    /// Renames one entry within this authority.
    ///
    /// # Errors
    ///
    /// Returns a rename error when source opening or handle-relative rename
    /// fails. `overwrite` is passed directly to the atomic native operation.
    pub(crate) fn rename(
        &self,
        source: &RelativePath,
        target: &RelativePath,
        overwrite: bool,
    ) -> LocalResult<()> {
        self.rename_to(source, self, target, overwrite)
    }

    /// Renames an entry between two opened namespace handles.
    ///
    /// # Errors
    ///
    /// Returns a rename error when source opening or handle-relative target
    /// publication fails.
    pub(crate) fn rename_to(
        &self,
        source: &RelativePath,
        target_namespace: &Self,
        target: &RelativePath,
        overwrite: bool,
    ) -> LocalResult<()> {
        let source_file = open_entry_no_follow(
            &self.handle,
            source,
            DELETE | FILE_READ_ATTRIBUTES | SYNCHRONIZE,
            FILE_OPEN,
            0,
        )
        .map_err(|error| {
            io_error(
                LocalFileOperation::Rename,
                source.as_path(),
                Some(target.as_path()),
                error,
            )
        })?;
        rename_handle(&source_file, &target_namespace.handle, target, overwrite)
            .map_err(|error| {
                io_error(
                    LocalFileOperation::Rename,
                    source.as_path(),
                    Some(target.as_path()),
                    error,
                )
            })
    }

    /// Creates a private staging file beside `target`.
    ///
    /// # Errors
    ///
    /// Returns an open-writer error when the parent cannot be opened or a
    /// unique private entry cannot be created.
    pub(crate) fn create_staged_file(
        &self,
        target: &RelativePath,
    ) -> LocalResult<StagedFile> {
        let (parent, _) =
            open_parent(&self.handle, target).map_err(|error| {
                io_error(
                    LocalFileOperation::OpenWriter,
                    target.as_path(),
                    None,
                    error,
                )
            })?;
        StagedFile::create(parent, target.as_path())
    }

    /// Reads native identity without following the final component.
    ///
    /// # Errors
    ///
    /// Returns a metadata error when opening or identity capture fails.
    pub(crate) fn entry_identity(
        &self,
        path: &RelativePath,
    ) -> LocalResult<EntryIdentity> {
        let file = self.open_for_metadata(path)?;
        EntryIdentity::for_file(&file, path)
    }

    /// Synchronizes the directory containing `path`.
    ///
    /// # Errors
    ///
    /// Returns a commit error when the parent cannot be opened or flushed.
    pub(crate) fn sync_parent(&self, path: &RelativePath) -> LocalResult<()> {
        let parent = if path.as_path().as_os_str().is_empty() {
            self.handle.try_clone().map_err(|error| {
                io_error(
                    LocalFileOperation::Commit,
                    path.as_path(),
                    None,
                    error,
                )
            })?
        } else {
            open_parent(&self.handle, path)
                .map(|(parent, _)| parent)
                .map_err(|error| {
                    io_error(
                        LocalFileOperation::Commit,
                        path.as_path(),
                        None,
                        error,
                    )
                })?
        };
        parent.sync_all().map_err(|error| {
            io_error(LocalFileOperation::Commit, path.as_path(), None, error)
        })
    }

    /// Reads best-effort path limits for the filesystem containing `path`.
    ///
    /// # Errors
    ///
    /// Returns a metadata error when the nearest parent cannot be opened.
    pub(crate) fn filesystem_limits(
        &self,
        path: &RelativePath,
    ) -> LocalResult<LocalFileSystemLimits> {
        self.probe_handle(path)
            .map(|file| filesystem_probe::limits(&file))
    }

    /// Reads best-effort capacity values for the filesystem containing `path`.
    ///
    /// # Errors
    ///
    /// Returns a metadata error when the nearest parent cannot be opened.
    pub(crate) fn filesystem_space(
        &self,
        path: &RelativePath,
    ) -> LocalResult<LocalFileSystemSpace> {
        self.probe_handle(path)
            .map(|file| filesystem_probe::space(&file))
    }

    /// Implements idempotent or exclusive directory creation.
    fn create_directory_impl(
        &self,
        path: &RelativePath,
        disposition: u32,
    ) -> LocalResult<()> {
        let directory = open_entry_no_follow(
            &self.handle,
            path,
            FILE_LIST_DIRECTORY | FILE_READ_ATTRIBUTES | SYNCHRONIZE,
            disposition,
            FILE_DIRECTORY_FILE,
        )
        .map_err(|error| {
            io_error(
                LocalFileOperation::CreateDirectory,
                path.as_path(),
                None,
                error,
            )
        })?;
        verify_real_directory(&directory).map_err(|error| {
            io_error(
                LocalFileOperation::CreateDirectory,
                path.as_path(),
                None,
                error,
            )
        })
    }

    /// Deletes an opened entry after validating its directory expectation.
    fn delete(
        &self,
        path: &RelativePath,
        directory: bool,
        operation: LocalFileOperation,
    ) -> LocalResult<()> {
        let entry = open_entry_no_follow(
            &self.handle,
            path,
            DELETE | FILE_READ_ATTRIBUTES | SYNCHRONIZE,
            FILE_OPEN,
            0,
        )
        .map_err(|error| io_error(operation, path.as_path(), None, error))?;
        let is_directory =
            entry.metadata().map(|metadata| metadata.is_dir()).map_err(
                |error| io_error(operation, path.as_path(), None, error),
            )?;
        if is_directory != directory {
            return Err(LocalFileError::new(
                LocalFileErrorKind::TypeConflict,
                operation,
            )
            .with_path(path.as_path().to_path_buf())
            .with_reason(
                "the entry kind does not match the delete operation",
            ));
        }
        delete_handle(&entry)
            .map_err(|error| io_error(operation, path.as_path(), None, error))
    }

    /// Opens the authority root or nearest parent for filesystem probing.
    fn probe_handle(&self, path: &RelativePath) -> LocalResult<File> {
        if path.as_path().as_os_str().is_empty() {
            return self.handle.try_clone().map_err(|error| {
                io_error(
                    LocalFileOperation::Metadata,
                    path.as_path(),
                    None,
                    error,
                )
            });
        }
        open_parent(&self.handle, path)
            .map(|(parent, _)| parent)
            .map_err(|error| {
                io_error(
                    LocalFileOperation::Metadata,
                    path.as_path(),
                    None,
                    error,
                )
            })
    }

    /// Opens the root or one final entry for no-follow metadata.
    fn open_for_metadata(&self, path: &RelativePath) -> LocalResult<File> {
        if path.as_path().as_os_str().is_empty() {
            return self.handle.try_clone().map_err(|error| {
                io_error(
                    LocalFileOperation::Metadata,
                    path.as_path(),
                    None,
                    error,
                )
            });
        }
        open_entry_no_follow(
            &self.handle,
            path,
            FILE_READ_ATTRIBUTES | SYNCHRONIZE,
            FILE_OPEN,
            0,
        )
        .map_err(|error| {
            io_error(LocalFileOperation::Metadata, path.as_path(), None, error)
        })
    }
}

/// Opens one entry and rejects name-surrogate reparse points.
pub(super) fn open_entry(
    root: &File,
    path: &RelativePath,
    access: u32,
    disposition: u32,
    options: u32,
) -> io::Result<File> {
    let entry = open_entry_no_follow(root, path, access, disposition, options)?;
    verify_not_name_surrogate(&entry)?;
    Ok(entry)
}

/// Opens one entry without following or rejecting its final reparse point.
pub(super) fn open_entry_no_follow(
    root: &File,
    path: &RelativePath,
    access: u32,
    disposition: u32,
    options: u32,
) -> io::Result<File> {
    let (parent, name) = open_parent(root, path)?;
    nt_open_at(&parent, &name, access, disposition, options)
}

/// Opens every parent component beneath `root` without following reparses.
pub(super) fn open_parent(
    root: &File,
    path: &RelativePath,
) -> io::Result<(File, OsString)> {
    let mut components: Vec<OsString> = path
        .as_path()
        .components()
        .map(|component| component.as_os_str().to_os_string())
        .collect();
    let name = components.pop().ok_or_else(|| {
        io::Error::new(ErrorKind::InvalidInput, "relative path is empty")
    })?;
    let mut parent = root.try_clone()?;
    for component in components {
        let directory = nt_open_at(
            &parent,
            &component,
            FILE_LIST_DIRECTORY | FILE_READ_ATTRIBUTES | SYNCHRONIZE,
            FILE_OPEN,
            FILE_DIRECTORY_FILE,
        )?;
        verify_real_directory(&directory)?;
        parent = directory;
    }
    Ok((parent, name))
}

/// Opens one child name relative to an already-opened directory handle.
pub(super) fn nt_open_at(
    parent: &File,
    name: &OsStr,
    access: u32,
    disposition: u32,
    options: u32,
) -> io::Result<File> {
    let name = unicode_string(name)?;
    let attributes = OBJECT_ATTRIBUTES {
        Length: size_of::<OBJECT_ATTRIBUTES>() as u32,
        RootDirectory: parent.as_raw_handle(),
        ObjectName: &raw const name.header,
        Attributes: OBJ_CASE_INSENSITIVE,
        SecurityDescriptor: null(),
        SecurityQualityOfService: null(),
    };
    let mut status_block = IO_STATUS_BLOCK::default();
    let mut handle: HANDLE = null_mut();
    // SAFETY: all pointers refer to live stack values or the live UTF-16
    // buffer. `parent` remains open and NtCreateFile retains no input pointer.
    let status = unsafe {
        NtCreateFile(
            &raw mut handle,
            access,
            &raw const attributes,
            &raw mut status_block,
            null(),
            FILE_ATTRIBUTE_NORMAL,
            SHARE_MODE,
            disposition,
            options | FILE_OPEN_REPARSE_POINT | FILE_SYNCHRONOUS_IO_NONALERT,
            null(),
            0,
        )
    };
    nt_result(status)?;
    if handle.is_null() || handle == INVALID_HANDLE_VALUE {
        return Err(io::Error::other(
            "NtCreateFile returned an invalid handle",
        ));
    }
    // SAFETY: successful NtCreateFile returned a uniquely owned handle.
    Ok(unsafe { File::from_raw_handle(handle) })
}

/// Renames an open entry beneath `root`.
///
/// Every destination parent is opened component-by-component first, so the
/// rename cannot traverse a name-surrogate reparse point while resolving a
/// multi-component destination.
pub(super) fn rename_handle(
    source: &File,
    root: &File,
    destination: &RelativePath,
    overwrite: bool,
) -> io::Result<()> {
    use windows_sys::Win32::Storage::FileSystem::FILE_RENAME_INFO;
    use windows_sys::Win32::Storage::FileSystem::FileRenameInfo;
    use windows_sys::Win32::Storage::FileSystem::SetFileInformationByHandle;

    let (destination_parent, destination_name) =
        open_parent(root, destination)?;
    let units: Vec<u16> = destination_name.encode_wide().collect();
    let allocation = size_of::<FILE_RENAME_INFO>()
        .checked_add(units.len().saturating_sub(1) * size_of::<u16>())
        .ok_or_else(|| io::Error::other("rename buffer is too large"))?;
    let mut buffer = vec![0_usize; allocation.div_ceil(size_of::<usize>())];
    // SAFETY: Vec<usize> provides sufficient alignment and `allocation`
    // reserves the complete trailing UTF-16 name.
    let information =
        unsafe { &mut *buffer.as_mut_ptr().cast::<FILE_RENAME_INFO>() };
    information.Anonymous.ReplaceIfExists = overwrite;
    information.RootDirectory = destination_parent.as_raw_handle();
    information.FileNameLength = u32::try_from(units.len() * size_of::<u16>())
        .map_err(|_| {
            io::Error::new(ErrorKind::InvalidInput, "rename name is too long")
        })?;
    // SAFETY: the allocation includes the complete trailing name and both
    // slices remain live for this non-overlapping copy.
    unsafe {
        std::ptr::copy_nonoverlapping(
            units.as_ptr(),
            information.FileName.as_mut_ptr(),
            units.len(),
        );
    }
    // SAFETY: `source` is open with DELETE access and the buffer contains a
    // complete FILE_RENAME_INFO structure.
    let result = unsafe {
        SetFileInformationByHandle(
            source.as_raw_handle(),
            FileRenameInfo,
            buffer.as_ptr().cast(),
            u32::try_from(allocation)
                .map_err(|_| io::Error::other("rename buffer is too large"))?,
        )
    };
    if result == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

/// Deletes the entry identified by an already-opened handle.
pub(super) fn delete_handle(entry: &File) -> io::Result<()> {
    use windows_sys::Win32::Storage::FileSystem::FILE_DISPOSITION_INFO;
    use windows_sys::Win32::Storage::FileSystem::FileDispositionInfo;
    use windows_sys::Win32::Storage::FileSystem::SetFileInformationByHandle;

    let disposition = FILE_DISPOSITION_INFO { DeleteFile: true };
    // SAFETY: `entry` is open with DELETE access and `disposition` has the
    // exact advertised layout and size.
    let result = unsafe {
        SetFileInformationByHandle(
            entry.as_raw_handle(),
            FileDispositionInfo,
            (&raw const disposition).cast(),
            size_of::<FILE_DISPOSITION_INFO>() as u32,
        )
    };
    if result == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}

/// Rejects name-surrogate reparse points for an opened handle.
pub(super) fn verify_not_name_surrogate(file: &File) -> io::Result<()> {
    let attributes = handle_attributes(file)?;
    if attributes.FileAttributes & FILE_ATTRIBUTE_REPARSE_POINT != 0
        && attributes.ReparseTag & IO_REPARSE_TAG_NAME_SURROGATE != 0
    {
        Err(io::Error::new(
            ErrorKind::InvalidInput,
            "handle-relative traversal rejected a name-surrogate reparse point",
        ))
    } else {
        Ok(())
    }
}

/// Verifies an opened handle is a real directory.
fn verify_real_directory(directory: &File) -> io::Result<()> {
    let attributes = handle_attributes(directory)?;
    if attributes.FileAttributes & FILE_ATTRIBUTE_DIRECTORY == 0 {
        return Err(io::Error::new(
            ErrorKind::NotADirectory,
            "handle-relative component is not a directory",
        ));
    }
    verify_not_name_surrogate(directory)
}

/// Recovers the current native path of an already-opened handle.
fn handle_path(file: &File) -> io::Result<std::path::PathBuf> {
    let mut buffer = vec![0_u16; 512];
    loop {
        // SAFETY: `file` owns a live handle and `buffer` provides writable
        // UTF-16 storage of exactly the advertised capacity.
        let length = unsafe {
            GetFinalPathNameByHandleW(
                file.as_raw_handle(),
                buffer.as_mut_ptr(),
                u32::try_from(buffer.len()).map_err(|_| {
                    io::Error::other("handle path buffer is too large")
                })?,
                FILE_NAME_NORMALIZED,
            )
        };
        if length == 0 {
            return Err(io::Error::last_os_error());
        }
        let length = usize::try_from(length)
            .map_err(|_| io::Error::other("invalid handle path length"))?;
        if length < buffer.len() {
            return Ok(std::path::PathBuf::from(OsString::from_wide(
                &buffer[..length],
            )));
        }
        buffer.resize(length.saturating_add(1), 0);
    }
}

/// Reads file attributes and the reparse tag from an opened handle.
fn handle_attributes(file: &File) -> io::Result<FILE_ATTRIBUTE_TAG_INFO> {
    let mut attributes = FILE_ATTRIBUTE_TAG_INFO::default();
    // SAFETY: `file` owns a live handle and `attributes` is correctly sized
    // writable storage for FileAttributeTagInfo.
    let result = unsafe {
        GetFileInformationByHandleEx(
            file.as_raw_handle(),
            FileAttributeTagInfo,
            (&raw mut attributes).cast(),
            size_of::<FILE_ATTRIBUTE_TAG_INFO>() as u32,
        )
    };
    if result == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(attributes)
    }
}

/// Owns UTF-16 storage and its borrowed NT string header.
struct OwnedUnicodeString {
    /// Stable storage referenced by `header`.
    _units: Vec<u16>,
    /// NT string header passed through object attributes.
    header: UNICODE_STRING,
}

/// Builds one counted NT Unicode string without a trailing NUL.
fn unicode_string(value: &OsStr) -> io::Result<OwnedUnicodeString> {
    let mut units: Vec<u16> = value.encode_wide().collect();
    if units.contains(&0) {
        return Err(io::Error::new(
            ErrorKind::InvalidInput,
            "relative component contains an interior NUL",
        ));
    }
    let byte_len = units
        .len()
        .checked_mul(size_of::<u16>())
        .and_then(|length| u16::try_from(length).ok())
        .ok_or_else(|| {
            io::Error::new(ErrorKind::InvalidInput, "component is too long")
        })?;
    let header = UNICODE_STRING {
        Length: byte_len,
        MaximumLength: byte_len,
        Buffer: units.as_mut_ptr(),
    };
    Ok(OwnedUnicodeString {
        _units: units,
        header,
    })
}

/// Converts an absolute path to NUL-terminated UTF-16.
fn wide_path(path: &Path) -> io::Result<Vec<u16>> {
    let units: Vec<u16> = path.as_os_str().encode_wide().collect();
    if units.contains(&0) {
        return Err(io::Error::new(
            ErrorKind::InvalidInput,
            "root path contains an interior NUL",
        ));
    }
    Ok(units.into_iter().chain(Some(0)).collect())
}

/// Converts an NTSTATUS value into a standard I/O result.
pub(super) fn nt_result(status: i32) -> io::Result<()> {
    if status >= 0 {
        return Ok(());
    }
    // SAFETY: this conversion accepts any NTSTATUS and retains no pointers.
    let code = unsafe { RtlNtStatusToDosErrorNoTeb(status) };
    Err(io::Error::from_raw_os_error(code as i32))
}

/// Converts a native I/O failure into the crate's structured error.
pub(super) fn io_error(
    operation: LocalFileOperation,
    path: &Path,
    target: Option<&Path>,
    error: io::Error,
) -> LocalFileError {
    LocalFileError::from_io(
        operation,
        Some(path.to_path_buf()),
        target.map(Path::to_path_buf),
        error,
    )
}
