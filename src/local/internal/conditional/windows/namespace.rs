// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Windows conditional namespace permissions and entry identity checks.
//!
//! Retained handles and verified file identities grant access authority.
//! Diagnostic paths describe namespace entries but never grant permission to
//! reopen them; identity checks bind reopened entries to their original handle.

#[cfg(test)]
use std::cell::Cell;
use std::cell::RefCell;
use std::ffi::OsStr;
use std::ffi::OsString;
use std::fs;
use std::fs::File;
use std::io;
use std::io::ErrorKind;
use std::os::windows::ffi::OsStrExt;
use std::os::windows::io::AsRawHandle;
use std::os::windows::io::FromRawHandle;
use std::path::Path;
use std::path::PathBuf;
use std::ptr::null;
use std::ptr::null_mut;

use windows_sys::Wdk::Storage::FileSystem::FILE_CREATE;
use windows_sys::Wdk::Storage::FileSystem::FILE_DIRECTORY_FILE;
use windows_sys::Wdk::Storage::FileSystem::FILE_NON_DIRECTORY_FILE;
use windows_sys::Wdk::Storage::FileSystem::FILE_OPEN;
use windows_sys::Win32::Foundation::GENERIC_READ;
use windows_sys::Win32::Foundation::GENERIC_WRITE;
use windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE;
use windows_sys::Win32::Storage::FileSystem::CreateFileW;
use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT;
use windows_sys::Win32::Storage::FileSystem::FILE_FLAG_BACKUP_SEMANTICS;
use windows_sys::Win32::Storage::FileSystem::FILE_FLAG_OPEN_REPARSE_POINT;
use windows_sys::Win32::Storage::FileSystem::FILE_ID_INFO;
use windows_sys::Win32::Storage::FileSystem::FILE_LIST_DIRECTORY;
use windows_sys::Win32::Storage::FileSystem::FILE_READ_ATTRIBUTES;
use windows_sys::Win32::Storage::FileSystem::FILE_SHARE_DELETE;
use windows_sys::Win32::Storage::FileSystem::FILE_SHARE_READ;
use windows_sys::Win32::Storage::FileSystem::FILE_SHARE_WRITE;
use windows_sys::Win32::Storage::FileSystem::FILE_TRAVERSE;
use windows_sys::Win32::Storage::FileSystem::FileAttributeTagInfo;
use windows_sys::Win32::Storage::FileSystem::FileIdInfo;
use windows_sys::Win32::Storage::FileSystem::GetFileInformationByHandleEx;
use windows_sys::Win32::Storage::FileSystem::OPEN_EXISTING;
use windows_sys::Win32::Storage::FileSystem::SYNCHRONIZE;

use super::super::ConditionalEntry;
use super::super::RecoveryAccess;
use crate::LocalFileReader;
use crate::LocalRelativePath;
use crate::LocalResult;
use crate::local::internal::windows_rooted::create_rooted_directory;
use crate::local::internal::windows_rooted::native::nt_open_at_with_share_mode;
use crate::local::internal::windows_rooted::open_conditional_entry_at;
use crate::local::internal::windows_rooted::open_conditional_parent;
use crate::local::internal::windows_rooted::open_root_directory;
use crate::local::internal::windows_rooted::rename_conditional_entry_at;
use crate::options::LocalWriteOptions;
use crate::recovery::LocalRecoveryFileRole;

pub(super) const SHARE_ALL: u32 = FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE;
pub(super) const ENTRY_ACCESS: u32 = GENERIC_READ | FILE_READ_ATTRIBUTES | SYNCHRONIZE;
const FILE_ATTRIBUTE_DIRECTORY: u32 = 0x10;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) struct FileIdentity {
    volume_serial: u64,
    file_id: [u8; 16],
}

#[derive(Debug)]
enum Scope {
    Host { parent_path: PathBuf },
    Rooted { namespace_absolute: PathBuf },
}

#[derive(Debug)]
pub(super) struct Namespace {
    scope: Scope,
    parent: File,
    target_name: OsString,
    target_path: PathBuf,
    diagnostic_path: Option<PathBuf>,
    recovery_parents: RefCell<Vec<(PathBuf, File)>>,
    #[cfg(test)]
    fail_container_verification: Cell<bool>,
}

impl Namespace {
    pub(super) fn open_host(path: &Path, options: &LocalWriteOptions) -> io::Result<Self> {
        let parent_path = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."))
            .to_path_buf();
        if options.creates_parent() {
            fs::create_dir_all(&parent_path)?;
        }
        let target_name = path
            .file_name()
            .ok_or_else(|| io::Error::new(ErrorKind::InvalidInput, "conditional target has no filename"))?
            .to_os_string();
        let parent = open_root_directory(&parent_path)?;
        Ok(Self {
            scope: Scope::Host {
                parent_path: parent_path.clone(),
            },
            parent,
            target_name,
            target_path: path.to_path_buf(),
            diagnostic_path: Some(path.to_path_buf()),
            recovery_parents: RefCell::new(Vec::new()),
            #[cfg(test)]
            fail_container_verification: Cell::new(false),
        })
    }

    pub(super) fn open_rooted(
        root: File,
        relative: &Path,
        namespace_absolute: &Path,
        diagnostic: Option<&Path>,
        options: &LocalWriteOptions,
    ) -> io::Result<Self> {
        let relative = LocalRelativePath::new(relative)?;
        if options.creates_parent()
            && let Some(parent) = relative.as_path().parent().filter(|path| !path.as_os_str().is_empty())
        {
            let parent = LocalRelativePath::new(parent)?;
            create_rooted_directory(&root, Path::new(""), &parent, true, true)?;
        }
        let (parent, target_name) = open_conditional_parent(&root, &relative)?;
        Ok(Self {
            scope: Scope::Rooted {
                namespace_absolute: namespace_absolute.to_path_buf(),
            },
            parent,
            target_path: namespace_absolute.to_path_buf(),
            diagnostic_path: diagnostic.map(Path::to_path_buf),
            target_name,
            recovery_parents: RefCell::new(Vec::new()),
            #[cfg(test)]
            fail_container_verification: Cell::new(false),
        })
    }

    pub(super) fn target_name(&self) -> &OsStr {
        &self.target_name
    }
    pub(super) fn target_path(&self) -> &Path {
        &self.target_path
    }
    pub(super) fn is_rooted(&self) -> bool {
        matches!(self.scope, Scope::Rooted { .. })
    }

    pub(super) fn host_parent_path(&self) -> io::Result<&Path> {
        match &self.scope {
            Scope::Host { parent_path } => Ok(parent_path),
            Scope::Rooted { .. } => Err(io::Error::new(
                ErrorKind::Unsupported,
                "Rooted publication has no Host path",
            )),
        }
    }

    pub(super) fn private_file_path(&self, name: &OsStr) -> io::Result<PathBuf> {
        self.host_child_path(name)
    }

    pub(super) fn backup_container(&self) -> io::Result<(PathBuf, PathBuf)> {
        let parent = self.host_parent_path()?;
        for _ in 0..32 {
            let dirname = crate::local::internal::file_name_generation::try_random_file_name(
                ".qubit-conditional-recovery-",
                None,
                None,
            )?;
            let backup_name =
                crate::local::internal::file_name_generation::try_random_file_name("displaced-", None, Some(".bak"))?;
            let container = parent.join(&dirname);
            let handle = nt_open_at_with_share_mode(
                &self.parent,
                OsStr::new(&dirname),
                windows_sys::Win32::Storage::FileSystem::DELETE | FILE_READ_ATTRIBUTES | SYNCHRONIZE,
                FILE_CREATE,
                FILE_DIRECTORY_FILE,
                FILE_SHARE_READ | FILE_SHARE_WRITE,
            );
            match handle {
                Ok(handle) => {
                    if let Err(primary) = self.verify_new_container(&handle) {
                        let cleanup = delete_open_entry(&handle);
                        return Err(match cleanup {
                            Ok(()) => primary,
                            Err(cleanup) => io::Error::new(
                                primary.kind(),
                                format!(
                                    "{}; bound recovery container cleanup failed at {}: {}",
                                    primary,
                                    container.display(),
                                    cleanup
                                ),
                            ),
                        });
                    }
                    self.recovery_parents.borrow_mut().push((container.clone(), handle));
                    return Ok((container.clone(), container.join(backup_name)));
                }
                Err(error) if error.kind() == ErrorKind::AlreadyExists => continue,
                Err(error) => {
                    return Err(io::Error::new(
                        error.kind(),
                        format!(
                            "failed to create and bind recovery container {}: {}",
                            container.display(),
                            error
                        ),
                    ));
                }
            }
        }
        Err(io::Error::new(
            ErrorKind::AlreadyExists,
            "could not allocate a unique recovery container",
        ))
    }

    fn verify_new_container(&self, handle: &File) -> io::Result<()> {
        #[cfg(test)]
        if self.fail_container_verification.replace(false) {
            return Err(io::Error::other("injected recovery container verification failure"));
        }
        verify_directory_no_reparse(handle)
    }

    #[cfg(test)]
    pub(super) fn fail_next_container_verification(&self) {
        self.fail_container_verification.set(true);
    }

    #[cfg(test)]
    pub(super) fn backup_container_named(&self, name: &Path) -> io::Result<PathBuf> {
        let container = self.host_parent_path()?.join(name);
        fs::create_dir(&container)?;
        Ok(container)
    }

    pub(super) fn entry_in_container(
        &self,
        role: LocalRecoveryFileRole,
        container: &Path,
        backup_path: &Path,
    ) -> io::Result<ConditionalEntry> {
        let name = backup_path
            .file_name()
            .ok_or_else(|| io::Error::new(ErrorKind::InvalidInput, "backup path has no leaf"))?
            .to_os_string();
        let parent = open_host_directory_no_reparse(container)?;
        let file = open_conditional_entry_at(&parent, &name, ENTRY_ACCESS, FILE_OPEN, FILE_NON_DIRECTORY_FILE)?;
        let identity = file_identity(&file)?;
        self.recovery_parents
            .borrow_mut()
            .push((container.to_path_buf(), parent.try_clone()?));
        let access = EntryAccess {
            parent,
            name: name.clone(),
            diagnostic_path: self.diagnostic_path.clone(),
            identity,
            _retained: None,
        };
        Ok(ConditionalEntry::new(
            role,
            backup_path.to_path_buf(),
            self.diagnostic_path.clone(),
            Box::new(access),
        ))
    }

    pub(super) fn entry_bound_to(
        &self,
        role: LocalRecoveryFileRole,
        name: OsString,
        file: &File,
    ) -> io::Result<ConditionalEntry> {
        let identity = file_identity(file)?;
        verify_regular_no_reparse(file)?;
        let access = EntryAccess {
            parent: self.parent.try_clone()?,
            name: name.clone(),
            diagnostic_path: self.diagnostic_path.clone(),
            identity,
            _retained: Some(file.try_clone()?),
        };
        Ok(ConditionalEntry::new(
            role,
            self.child_path(&name),
            self.diagnostic_path.clone(),
            Box::new(access),
        ))
    }

    pub(super) fn remove_container(&self, container: &Path) -> io::Result<()> {
        if self.is_rooted() {
            return Err(io::Error::new(
                ErrorKind::Unsupported,
                "Rooted publication has no Host recovery container",
            ));
        }
        let mut recovery_parents = self.recovery_parents.borrow_mut();
        let index = recovery_parents
            .iter()
            .position(|(path, _)| path == container)
            .ok_or_else(|| {
                io::Error::new(
                    ErrorKind::NotFound,
                    "conditional recovery container handle is unavailable",
                )
            })?;
        let directory = &recovery_parents[index].1;
        verify_directory_no_reparse(directory)?;
        delete_open_entry(directory)?;
        recovery_parents.remove(index);
        Ok(())
    }

    pub(super) fn child_path(&self, name: &OsStr) -> PathBuf {
        match &self.scope {
            Scope::Host { parent_path } => parent_path.join(name),
            Scope::Rooted { namespace_absolute, .. } => {
                namespace_absolute.parent().unwrap_or_else(|| Path::new("/")).join(name)
            }
        }
    }

    pub(super) fn host_child_path(&self, name: &OsStr) -> io::Result<PathBuf> {
        match &self.scope {
            Scope::Host { parent_path } => Ok(parent_path.join(name)),
            Scope::Rooted { .. } => Err(io::Error::new(
                ErrorKind::Unsupported,
                "Rooted paths cannot use Host namespace operations",
            )),
        }
    }

    pub(super) fn entry(&self, role: LocalRecoveryFileRole, name: OsString) -> io::Result<ConditionalEntry> {
        let file = self.open_entry(&name)?;
        let identity = file_identity(&file)?;
        verify_regular_no_reparse(&file)?;
        let access = EntryAccess {
            parent: self.parent.try_clone()?,
            name: name.clone(),
            diagnostic_path: self.diagnostic_path.clone(),
            identity,
            _retained: None,
        };
        Ok(ConditionalEntry::new(
            role,
            self.child_path(&name),
            self.diagnostic_path.clone(),
            Box::new(access),
        ))
    }

    pub(super) fn create_private(&self, prefix: &str) -> io::Result<(OsString, File)> {
        for _ in 0..32 {
            let name = OsString::from(crate::local::internal::file_name_generation::try_random_file_name(
                &format!(".qubit-{prefix}-"),
                None,
                None,
            )?);
            match crate::temp::internal::privacy_windows::create_private_file_at(&self.parent, &name) {
                Ok(file) => return Ok((name, file)),
                Err(error) => {
                    let crate::temp::types::NativePrivateCreateError { source, retained } = error;
                    if let Some(mut retained) = retained {
                        let path = self.child_path(&retained.name);
                        let cleanup = match retained.file.take() {
                            Some(file) => self.delete_created_private(&file, None),
                            None => Err(io::Error::other(
                                "private creation retained no handle that can safely identify the entry",
                            )),
                        };
                        return Err(match cleanup {
                            Ok(()) => source,
                            Err(cleanup) => io::Error::new(
                                source.kind(),
                                super::private_entry_setup_error::PrivateEntrySetupError {
                                    primary: source,
                                    cleanup,
                                    path,
                                },
                            ),
                        });
                    }
                    if source.kind() == ErrorKind::AlreadyExists {
                        continue;
                    }
                    return Err(source);
                }
            }
        }
        Err(io::Error::new(
            ErrorKind::AlreadyExists,
            "could not allocate a unique conditional entry",
        ))
    }

    pub(super) fn open_entry(&self, name: &OsStr) -> io::Result<File> {
        match &self.scope {
            Scope::Host { .. } => open_host_file(&self.host_child_path(name)?, ENTRY_ACCESS),
            Scope::Rooted { .. } => {
                open_conditional_entry_at(&self.parent, name, ENTRY_ACCESS, FILE_OPEN, FILE_NON_DIRECTORY_FILE)
            }
        }
    }

    pub(super) fn identity_at(&self, name: &OsStr) -> io::Result<Option<FileIdentity>> {
        match self.open_entry(name) {
            Ok(file) => Ok(Some(file_identity(&file)?)),
            Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
            Err(error) => Err(error),
        }
    }

    pub(super) fn verify_entry(&self, entry: &ConditionalEntry) -> io::Result<()> {
        let retained = entry.open_reader().map_err(io::Error::other)?;
        let path = entry.namespace_path();
        let name = path
            .file_name()
            .ok_or_else(|| io::Error::new(ErrorKind::InvalidInput, "entry has no filename"))?;
        let current = if self.is_rooted() || path.parent() == Some(self.host_parent_path()?) {
            self.entry(entry.role(), name.to_os_string())?
        } else {
            let parent = path
                .parent()
                .ok_or_else(|| io::Error::new(ErrorKind::InvalidInput, "entry has no parent"))?;
            self.entry_in_container(entry.role(), parent, path)?
        };
        let current = current.open_reader().map_err(io::Error::other)?;
        if file_identity(retained.as_file())? != file_identity(current.as_file())? {
            return Err(io::Error::new(
                ErrorKind::InvalidData,
                "conditional entry name changed identity",
            ));
        }
        Ok(())
    }

    pub(super) fn rename(&self, source: &OsStr, destination: &OsStr, overwrite: bool) -> io::Result<()> {
        rename_conditional_entry_at(&self.parent, source, &self.parent, destination, overwrite)
    }

    pub(super) fn remove(&self, entry: &ConditionalEntry) -> io::Result<()> {
        let verified = entry.open_reader().map_err(io::Error::other)?;
        let expected_identity = file_identity(verified.as_file())?;
        match &self.scope {
            Scope::Host { .. } => {
                let parent = self.parent_handle_for(entry.namespace_path())?;
                let name = entry
                    .namespace_path()
                    .file_name()
                    .ok_or_else(|| io::Error::new(ErrorKind::InvalidInput, "entry has no filename"))?;
                let file = open_conditional_entry_at(
                    &parent,
                    name,
                    windows_sys::Win32::Storage::FileSystem::DELETE | FILE_READ_ATTRIBUTES | SYNCHRONIZE,
                    FILE_OPEN,
                    FILE_NON_DIRECTORY_FILE,
                )?;
                if file_identity(&file)? != expected_identity {
                    return Err(io::Error::new(
                        ErrorKind::InvalidData,
                        "conditional entry identity changed before removal",
                    ));
                }
                delete_open_entry(&file)
            }
            Scope::Rooted { .. } => {
                let parent = self.parent.try_clone()?;
                let name = entry
                    .namespace_path()
                    .file_name()
                    .ok_or_else(|| io::Error::new(ErrorKind::InvalidInput, "entry has no filename"))?;
                let file = open_conditional_entry_at(
                    &parent,
                    name,
                    windows_sys::Win32::Storage::FileSystem::DELETE | FILE_READ_ATTRIBUTES | SYNCHRONIZE,
                    FILE_OPEN,
                    FILE_NON_DIRECTORY_FILE,
                )?;
                if file_identity(&file)? != expected_identity {
                    return Err(io::Error::new(
                        ErrorKind::InvalidData,
                        "conditional entry identity changed before removal",
                    ));
                }
                delete_open_entry(&file)
            }
        }
    }

    pub(super) fn delete_created_private(&self, file: &File, expected: Option<FileIdentity>) -> io::Result<()> {
        if let Some(expected) = expected
            && file_identity(file)? != expected
        {
            return Err(io::Error::new(
                ErrorKind::InvalidData,
                "private entry handle identity changed before cleanup",
            ));
        }
        verify_regular_no_reparse(file)?;
        delete_open_entry(file)
    }

    fn parent_handle_for(&self, path: &Path) -> io::Result<File> {
        let parent_path = path
            .parent()
            .ok_or_else(|| io::Error::new(ErrorKind::InvalidInput, "entry path has no parent"))?;
        if matches!(&self.scope, Scope::Host { parent_path: expected } if expected == parent_path) {
            return self.parent.try_clone();
        }
        self.recovery_parents
            .borrow()
            .iter()
            .find(|(container, _)| container == parent_path)
            .map(|(_, parent)| parent.try_clone())
            .transpose()?
            .ok_or_else(|| io::Error::new(ErrorKind::NotFound, "conditional recovery parent handle is unavailable"))
    }
}

#[derive(Debug)]
struct EntryAccess {
    parent: File,
    name: OsString,
    diagnostic_path: Option<PathBuf>,
    identity: FileIdentity,
    _retained: Option<File>,
}

impl RecoveryAccess for EntryAccess {
    fn open_reader(&self) -> LocalResult<LocalFileReader> {
        let file = open_conditional_entry_at(
            &self.parent,
            &self.name,
            ENTRY_ACCESS,
            FILE_OPEN,
            FILE_NON_DIRECTORY_FILE,
        )
        .map_err(|error| {
            crate::LocalFileError::from_io(
                crate::LocalFileOperation::OpenReader,
                self.diagnostic_path.clone(),
                None,
                error,
            )
        })?;
        let observed = file_identity(&file).map_err(|error| {
            crate::LocalFileError::from_io(
                crate::LocalFileOperation::OpenReader,
                self.diagnostic_path.clone(),
                None,
                error,
            )
        })?;
        verify_regular_no_reparse(&file).map_err(|error| {
            crate::LocalFileError::from_io(
                crate::LocalFileOperation::OpenReader,
                self.diagnostic_path.clone(),
                None,
                error,
            )
        })?;
        if observed != self.identity {
            return Err(crate::LocalFileError::from_io(
                crate::LocalFileOperation::OpenReader,
                self.diagnostic_path.clone(),
                None,
                io::Error::new(ErrorKind::InvalidData, "conditional recovery entry identity changed"),
            ));
        }
        LocalFileReader::from_file(file).map_err(|error| {
            crate::LocalFileError::from_io(
                crate::LocalFileOperation::OpenReader,
                self.diagnostic_path.clone(),
                None,
                error,
            )
        })
    }

    fn sync_file(&self) -> io::Result<()> {
        let file = open_conditional_entry_at(
            &self.parent,
            &self.name,
            GENERIC_READ | GENERIC_WRITE | FILE_READ_ATTRIBUTES | SYNCHRONIZE,
            FILE_OPEN,
            FILE_NON_DIRECTORY_FILE,
        )?;
        verify_regular_no_reparse(&file)?;
        if file_identity(&file)? != self.identity {
            return Err(io::Error::new(
                ErrorKind::InvalidData,
                "conditional entry identity changed before file synchronization",
            ));
        }
        file.sync_all()
    }
}

pub(super) fn file_identity(file: &File) -> io::Result<FileIdentity> {
    let mut identity = FILE_ID_INFO::default();
    // SAFETY: `file` owns a live handle and `identity` is a correctly sized
    // writable buffer for FileIdInfo.
    let success = unsafe {
        GetFileInformationByHandleEx(
            file.as_raw_handle(),
            FileIdInfo,
            (&raw mut identity).cast(),
            std::mem::size_of::<FILE_ID_INFO>() as u32,
        )
    };
    if success == 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(FileIdentity {
        volume_serial: identity.VolumeSerialNumber,
        file_id: identity.FileId.Identifier,
    })
}

pub(super) fn verify_regular_no_reparse(file: &File) -> io::Result<()> {
    let metadata = file.metadata()?;
    if !metadata.is_file() {
        return Err(io::Error::new(
            ErrorKind::InvalidInput,
            "conditional entry is not a regular file",
        ));
    }
    let mut tag = windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_TAG_INFO::default();
    // SAFETY: `file` owns a live handle and `tag` is a correctly sized output
    // buffer.
    let success = unsafe {
        GetFileInformationByHandleEx(
            file.as_raw_handle(),
            windows_sys::Win32::Storage::FileSystem::FileAttributeTagInfo,
            (&raw mut tag).cast(),
            std::mem::size_of::<windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_TAG_INFO>() as u32,
        )
    };
    if success == 0 {
        return Err(io::Error::last_os_error());
    }
    if tag.FileAttributes & FILE_ATTRIBUTE_REPARSE_POINT != 0 || tag.FileAttributes & FILE_ATTRIBUTE_DIRECTORY != 0 {
        return Err(io::Error::new(
            ErrorKind::InvalidInput,
            "conditional entry is a reparse point or directory",
        ));
    }
    Ok(())
}

pub(super) fn open_host_file(path: &Path, access: u32) -> io::Result<File> {
    let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    if wide[..wide.len() - 1].contains(&0) {
        return Err(io::Error::new(ErrorKind::InvalidInput, "path contains an interior NUL"));
    }
    // SAFETY: `wide` is NUL terminated and remains live for the synchronous
    // CreateFileW call. The returned handle is checked before ownership transfer.
    let handle = unsafe {
        CreateFileW(
            wide.as_ptr(),
            access,
            SHARE_ALL,
            null(),
            OPEN_EXISTING,
            FILE_FLAG_OPEN_REPARSE_POINT,
            null_mut(),
        )
    };
    if handle == INVALID_HANDLE_VALUE {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: CreateFileW returned a uniquely owned valid handle.
    let file = unsafe { File::from_raw_handle(handle) };
    verify_regular_no_reparse(&file)?;
    Ok(file)
}

fn open_host_directory_no_reparse(path: &Path) -> io::Result<File> {
    let wide: Vec<u16> = path.as_os_str().encode_wide().chain(Some(0)).collect();
    if wide[..wide.len() - 1].contains(&0) {
        return Err(io::Error::new(ErrorKind::InvalidInput, "path contains an interior NUL"));
    }
    // SAFETY: `wide` remains live through the synchronous call; the returned
    // handle is validated before ownership transfers to `File`.
    let handle = unsafe {
        CreateFileW(
            wide.as_ptr(),
            FILE_LIST_DIRECTORY | FILE_READ_ATTRIBUTES | FILE_TRAVERSE | SYNCHRONIZE,
            SHARE_ALL,
            null(),
            OPEN_EXISTING,
            FILE_FLAG_BACKUP_SEMANTICS | FILE_FLAG_OPEN_REPARSE_POINT,
            null_mut(),
        )
    };
    if handle == INVALID_HANDLE_VALUE {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: CreateFileW returned a uniquely owned valid handle.
    let file = unsafe { File::from_raw_handle(handle) };
    verify_directory_no_reparse(&file)?;
    Ok(file)
}

fn verify_directory_no_reparse(file: &File) -> io::Result<()> {
    let mut tag = windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_TAG_INFO::default();
    // SAFETY: `file` owns a live handle and `tag` is a correctly sized output
    // buffer.
    let success = unsafe {
        GetFileInformationByHandleEx(
            file.as_raw_handle(),
            FileAttributeTagInfo,
            (&raw mut tag).cast(),
            std::mem::size_of_val(&tag) as u32,
        )
    };
    if success == 0 {
        return Err(io::Error::last_os_error());
    }
    if tag.FileAttributes & FILE_ATTRIBUTE_REPARSE_POINT != 0 || tag.FileAttributes & FILE_ATTRIBUTE_DIRECTORY == 0 {
        return Err(io::Error::new(
            ErrorKind::InvalidInput,
            "conditional recovery container is not a real directory",
        ));
    }
    Ok(())
}

fn delete_open_entry(file: &File) -> io::Result<()> {
    use windows_sys::Win32::Storage::FileSystem::FILE_DISPOSITION_INFO;
    use windows_sys::Win32::Storage::FileSystem::FileDispositionInfo;
    use windows_sys::Win32::Storage::FileSystem::SetFileInformationByHandle;
    let disposition = FILE_DISPOSITION_INFO { DeleteFile: true };
    // SAFETY: The handle was opened with DELETE and the buffer matches the info
    // class.
    let success = unsafe {
        SetFileInformationByHandle(
            file.as_raw_handle(),
            FileDispositionInfo,
            (&raw const disposition).cast(),
            std::mem::size_of::<FILE_DISPOSITION_INFO>() as u32,
        )
    };
    if success == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(())
    }
}
