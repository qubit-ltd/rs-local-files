// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Windows private temporary-resource creation and access control.

#[cfg(test)]
use std::cell::Cell;
use std::ffi::OsStr;
use std::fs::File;
use std::io;
use std::mem::size_of;
use std::os::windows::ffi::OsStrExt;
use std::os::windows::fs::OpenOptionsExt;
use std::os::windows::io::AsRawHandle;
use std::os::windows::io::FromRawHandle;
use std::ptr;

use windows_sys::Wdk::Foundation::OBJECT_ATTRIBUTES;
use windows_sys::Wdk::Storage::FileSystem::FILE_CREATE;
use windows_sys::Wdk::Storage::FileSystem::FILE_DIRECTORY_FILE;
use windows_sys::Wdk::Storage::FileSystem::FILE_NON_DIRECTORY_FILE;
use windows_sys::Wdk::Storage::FileSystem::FILE_OPEN;
use windows_sys::Wdk::Storage::FileSystem::FILE_OPEN_REPARSE_POINT;
use windows_sys::Wdk::Storage::FileSystem::FILE_SYNCHRONOUS_IO_NONALERT;
use windows_sys::Wdk::Storage::FileSystem::NtCreateFile;
use windows_sys::Wdk::Storage::FileSystem::RtlNtStatusToDosErrorNoTeb;
use windows_sys::Win32::Foundation::CloseHandle;
use windows_sys::Win32::Foundation::GetLastError;
use windows_sys::Win32::Foundation::HANDLE;
use windows_sys::Win32::Foundation::INVALID_HANDLE_VALUE;
use windows_sys::Win32::Foundation::LocalFree;
use windows_sys::Win32::Foundation::UNICODE_STRING;
use windows_sys::Win32::Security::ACCESS_ALLOWED_ACE;
use windows_sys::Win32::Security::ACL;
use windows_sys::Win32::Security::ACL_REVISION;
use windows_sys::Win32::Security::AddAccessAllowedAce;
use windows_sys::Win32::Security::DACL_SECURITY_INFORMATION;
use windows_sys::Win32::Security::GetAce;
use windows_sys::Win32::Security::GetLengthSid;
use windows_sys::Win32::Security::GetSecurityDescriptorControl;
use windows_sys::Win32::Security::GetSecurityDescriptorDacl;
use windows_sys::Win32::Security::GetTokenInformation;
use windows_sys::Win32::Security::InitializeAcl;
use windows_sys::Win32::Security::InitializeSecurityDescriptor;
use windows_sys::Win32::Security::PROTECTED_DACL_SECURITY_INFORMATION;
use windows_sys::Win32::Security::PSECURITY_DESCRIPTOR;
use windows_sys::Win32::Security::PSID;
use windows_sys::Win32::Security::SE_DACL_PROTECTED;
use windows_sys::Win32::Security::SECURITY_DESCRIPTOR;
use windows_sys::Win32::Security::SetSecurityDescriptorDacl;
use windows_sys::Win32::Security::TOKEN_QUERY;
use windows_sys::Win32::Security::TOKEN_USER;
use windows_sys::Win32::Security::TokenUser;
use windows_sys::Win32::Storage::FileSystem::DELETE;
use windows_sys::Win32::Storage::FileSystem::FILE_ADD_FILE;
use windows_sys::Win32::Storage::FileSystem::FILE_ADD_SUBDIRECTORY;
use windows_sys::Win32::Storage::FileSystem::FILE_APPEND_DATA;
use windows_sys::Win32::Storage::FileSystem::FILE_DELETE_CHILD;
use windows_sys::Win32::Storage::FileSystem::FILE_FLAG_BACKUP_SEMANTICS;
use windows_sys::Win32::Storage::FileSystem::FILE_LIST_DIRECTORY;
use windows_sys::Win32::Storage::FileSystem::FILE_READ_ATTRIBUTES;
use windows_sys::Win32::Storage::FileSystem::FILE_READ_DATA;
use windows_sys::Win32::Storage::FileSystem::FILE_READ_EA;
use windows_sys::Win32::Storage::FileSystem::FILE_TRAVERSE;
use windows_sys::Win32::Storage::FileSystem::FILE_WRITE_ATTRIBUTES;
use windows_sys::Win32::Storage::FileSystem::FILE_WRITE_DATA;
use windows_sys::Win32::Storage::FileSystem::FILE_WRITE_EA;
use windows_sys::Win32::Storage::FileSystem::READ_CONTROL;
use windows_sys::Win32::Storage::FileSystem::SYNCHRONIZE;
use windows_sys::Win32::Storage::FileSystem::WRITE_DAC;
use windows_sys::Win32::System::IO::IO_STATUS_BLOCK;
use windows_sys::Win32::System::Threading::GetCurrentProcess;
use windows_sys::Win32::System::Threading::OpenProcessToken;

use crate::policy::LocalOwnerAccessPolicy;
use crate::temp::types::NativePrivateCreateError;
use crate::temp::types::NativePrivateEntry;

const ACL_BUFFER_SIZE: usize = 1024;
const FILE_INITIAL_ACCESS: u32 = FILE_READ_DATA
    | FILE_WRITE_DATA
    | FILE_APPEND_DATA
    | FILE_READ_ATTRIBUTES
    | FILE_READ_EA
    | FILE_WRITE_ATTRIBUTES
    | FILE_WRITE_EA
    | READ_CONTROL
    | WRITE_DAC
    | DELETE
    | SYNCHRONIZE;
const DIRECTORY_ACCESS: u32 = FILE_LIST_DIRECTORY
    | FILE_TRAVERSE
    | FILE_ADD_FILE
    | FILE_ADD_SUBDIRECTORY
    | FILE_DELETE_CHILD
    | FILE_READ_ATTRIBUTES
    | FILE_READ_EA
    | FILE_WRITE_ATTRIBUTES
    | READ_CONTROL
    | WRITE_DAC
    | DELETE
    | SYNCHRONIZE;
const OWNER_ACCESS_MASKS: u32 = 0x001f_01ff;

#[cfg(test)]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FailurePoint {
    Token,
    Sid,
    Descriptor,
    Create,
    AclApply,
    AccessApplyBefore,
    AclReadback,
    ReadOnly,
}

#[cfg(test)]
thread_local! {
    static FAILURE_POINT: Cell<Option<(FailurePoint, usize)>> = const { Cell::new(None) };
}

#[cfg(test)]
pub(crate) fn fail_once_at(point: FailurePoint) {
    fail_after(point, 0);
}

#[cfg(test)]
pub(crate) fn fail_after(point: FailurePoint, successful_steps: usize) {
    FAILURE_POINT.with(|slot| slot.set(Some((point, successful_steps))));
}

#[cfg(test)]
fn fail_if(point: FailurePoint) -> io::Result<()> {
    let should_fail = FAILURE_POINT.with(|slot| match slot.get() {
        Some((scheduled, 0)) if scheduled == point => {
            slot.set(None);
            true
        }
        Some((scheduled, remaining)) if scheduled == point => {
            slot.set(Some((scheduled, remaining - 1)));
            false
        }
        _ => false,
    });
    if should_fail {
        Err(io::Error::other(format!("injected {point:?} failure")))
    } else {
        Ok(())
    }
}

macro_rules! check_failure {
    ($point:ident) => {{
        #[cfg(test)]
        fail_if(FailurePoint::$point)?;
    }};
}

#[derive(Debug)]
struct Token(HANDLE);

impl Drop for Token {
    fn drop(&mut self) {
        if !self.0.is_null() && self.0 != INVALID_HANDLE_VALUE {
            // The token handle was returned by OpenProcessToken.
            unsafe { CloseHandle(self.0) };
        }
    }
}

#[derive(Debug)]
struct CurrentUser {
    _token: Token,
    information: Vec<usize>,
}

impl CurrentUser {
    fn acquire() -> io::Result<Self> {
        check_failure!(Token);
        let mut token = ptr::null_mut();
        // GetCurrentProcess is a pseudo-handle; OpenProcessToken returns an owned
        // handle.
        if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) } == 0 {
            return Err(last_os_error());
        }
        let token = Token(token);

        let mut required = 0_u32;
        // The first call obtains the required buffer size.
        unsafe {
            GetTokenInformation(token.0, TokenUser, ptr::null_mut(), 0, &mut required);
        }
        if required == 0 {
            return Err(last_os_error());
        }
        let words = (required as usize).div_ceil(size_of::<usize>());
        let mut information = vec![0_usize; words];
        if unsafe {
            GetTokenInformation(
                token.0,
                TokenUser,
                information.as_mut_ptr().cast(),
                required,
                &mut required,
            )
        } == 0
        {
            return Err(last_os_error());
        }
        Ok(Self {
            _token: token,
            information,
        })
    }

    fn sid(&self) -> io::Result<PSID> {
        check_failure!(Sid);
        // GetTokenInformation returns TOKEN_USER at the start of this owned buffer.
        let sid = unsafe { (*(self.information.as_ptr().cast::<TOKEN_USER>())).User.Sid };
        if sid.is_null() || unsafe { GetLengthSid(sid) } == 0 {
            return Err(last_os_error());
        }
        Ok(sid)
    }
}

struct SecurityDescriptor {
    descriptor: SECURITY_DESCRIPTOR,
    acl: Vec<usize>,
}

impl SecurityDescriptor {
    fn for_current_user(directory: bool, read_only: bool) -> io::Result<Self> {
        let access = default_access_mask(directory, read_only);
        Self::for_current_user_with_mask(access)
    }

    fn for_current_user_with_mask(access: u32) -> io::Result<Self> {
        check_failure!(Descriptor);
        let current_user = CurrentUser::acquire()?;
        let current_sid = current_user.sid()?;

        let mut acl = vec![0_usize; ACL_BUFFER_SIZE.div_ceil(size_of::<usize>())];
        let acl_ptr = acl.as_mut_ptr().cast::<ACL>();
        if unsafe { InitializeAcl(acl_ptr, (acl.len() * size_of::<usize>()) as u32, ACL_REVISION) } == 0 {
            return Err(last_os_error());
        }
        if unsafe { AddAccessAllowedAce(acl_ptr, ACL_REVISION, access, current_sid) } == 0 {
            return Err(last_os_error());
        }

        let mut descriptor = unsafe { std::mem::zeroed::<SECURITY_DESCRIPTOR>() };
        if unsafe { InitializeSecurityDescriptor((&mut descriptor as *mut SECURITY_DESCRIPTOR).cast(), 1) } == 0 {
            return Err(last_os_error());
        }
        if unsafe { SetSecurityDescriptorDacl((&mut descriptor as *mut SECURITY_DESCRIPTOR).cast(), 1, acl_ptr, 0) }
            == 0
        {
            return Err(last_os_error());
        }
        // NtCreateFile with this descriptor creates an explicit DACL without inheriting
        // ACEs.
        descriptor.Control |= SE_DACL_PROTECTED;
        Ok(Self { descriptor, acl })
    }

    fn as_ptr(&self) -> PSECURITY_DESCRIPTOR {
        (&self.descriptor as *const SECURITY_DESCRIPTOR).cast_mut().cast()
    }
}

fn default_access_mask(directory: bool, read_only: bool) -> u32 {
    if directory {
        if read_only {
            DIRECTORY_ACCESS
                & !(FILE_ADD_FILE | FILE_ADD_SUBDIRECTORY | FILE_DELETE_CHILD | FILE_WRITE_ATTRIBUTES | DELETE)
        } else {
            DIRECTORY_ACCESS
        }
    } else if read_only {
        FILE_INITIAL_ACCESS & !(FILE_WRITE_DATA | FILE_APPEND_DATA)
    } else {
        FILE_INITIAL_ACCESS
    }
}

fn owner_access_mask(policy: &LocalOwnerAccessPolicy, directory: bool) -> io::Result<u32> {
    let (directory_mask, file_mask) = policy.windows_masks().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "Windows owner access policy is not configured",
        )
    })?;
    if directory_mask & !OWNER_ACCESS_MASKS != 0 || file_mask & !OWNER_ACCESS_MASKS != 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Windows owner access policy contains unsupported access bits",
        ));
    }
    Ok(if directory { directory_mask } else { file_mask })
}

#[derive(Debug)]
struct NativeHandle(HANDLE);

impl Drop for NativeHandle {
    fn drop(&mut self) {
        if !self.0.is_null() && self.0 != INVALID_HANDLE_VALUE {
            // NtCreateFile returned an owned kernel handle.
            unsafe { CloseHandle(self.0) };
        }
    }
}

fn last_os_error() -> io::Error {
    io::Error::from_raw_os_error(unsafe { GetLastError() } as i32)
}

/// Converts an NTSTATUS failure into its corresponding standard I/O error.
fn nt_status_error(status: i32) -> io::Error {
    // SAFETY: RtlNtStatusToDosErrorNoTeb accepts any NTSTATUS value and does
    // not retain pointers.
    let error_code = unsafe { RtlNtStatusToDosErrorNoTeb(status) };
    io::Error::from_raw_os_error(error_code as i32)
}

fn create_at(parent: &File, name: &OsStr, directory: bool) -> io::Result<File> {
    create_at_with_mask(
        parent,
        name,
        directory,
        default_access_mask(directory, false),
        if directory {
            DIRECTORY_ACCESS
        } else {
            FILE_INITIAL_ACCESS
        },
    )
}

fn create_at_with_mask(
    parent: &File,
    name: &OsStr,
    directory: bool,
    access_mask: u32,
    handle_access: u32,
) -> io::Result<File> {
    let mut name: Vec<u16> = name.encode_wide().collect();
    if name.is_empty()
        || name.contains(&0)
        || name.contains(&(b'/' as u16))
        || name.contains(&(b'\\' as u16))
        || name.contains(&(b':' as u16))
        || name.as_slice() == [b'.' as u16]
        || name.as_slice() == [b'.' as u16, b'.' as u16]
    {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "invalid entry name"));
    }
    let byte_length = name
        .len()
        .checked_mul(size_of::<u16>())
        .and_then(|length| u16::try_from(length).ok())
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "entry name is too long"))?;
    let unicode_name = UNICODE_STRING {
        Length: byte_length,
        MaximumLength: byte_length,
        Buffer: name.as_mut_ptr(),
    };
    let mut attributes = OBJECT_ATTRIBUTES {
        Length: size_of::<OBJECT_ATTRIBUTES>() as u32,
        RootDirectory: parent.as_raw_handle().cast(),
        ObjectName: &unicode_name,
        Attributes: 0,
        SecurityDescriptor: ptr::null(),
        SecurityQualityOfService: ptr::null(),
    };
    let descriptor = SecurityDescriptor::for_current_user_with_mask(access_mask)?;
    attributes.SecurityDescriptor = descriptor.as_ptr().cast();
    check_failure!(Create);
    let mut handle = ptr::null_mut();
    let mut io_status = unsafe { std::mem::zeroed::<IO_STATUS_BLOCK>() };
    let status = unsafe {
        NtCreateFile(
            &mut handle,
            handle_access,
            &attributes,
            &mut io_status,
            ptr::null(),
            0,
            0x7, // FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE
            FILE_CREATE,
            FILE_SYNCHRONOUS_IO_NONALERT
                | if directory {
                    FILE_DIRECTORY_FILE
                } else {
                    FILE_NON_DIRECTORY_FILE
                },
            ptr::null(),
            0,
        )
    };
    if status < 0 {
        return Err(nt_status_error(status));
    }
    let handle = NativeHandle(handle);
    // Transfer the successfully created native handle into File ownership.
    let file = unsafe { File::from_raw_handle(handle.0.cast()) };
    std::mem::forget(handle);
    Ok(file)
}

/// Opens one existing child by native directory handle with rights to update
/// its DACL.
pub(crate) fn open_existing_entry_at(parent: &File, name: &OsStr, directory: bool) -> io::Result<File> {
    open_existing_entry_at_with_sync(parent, name, directory, false)
}

pub(crate) fn open_existing_entry_at_for_sync(parent: &File, name: &OsStr, directory: bool) -> io::Result<File> {
    open_existing_entry_at_with_sync(parent, name, directory, true)
}

fn open_existing_entry_at_with_sync(parent: &File, name: &OsStr, directory: bool, for_sync: bool) -> io::Result<File> {
    let mut name: Vec<u16> = name.encode_wide().collect();
    if name.is_empty()
        || name.contains(&0)
        || name.contains(&(b'/' as u16))
        || name.contains(&(b'\\' as u16))
        || name.contains(&(b':' as u16))
        || name.as_slice() == [b'.' as u16]
        || name.as_slice() == [b'.' as u16, b'.' as u16]
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "name must be one native path component",
        ));
    }
    let byte_length = name
        .len()
        .checked_mul(size_of::<u16>())
        .and_then(|length| u16::try_from(length).ok())
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "entry name is too long"))?;
    let unicode_name = UNICODE_STRING {
        Length: byte_length,
        MaximumLength: byte_length,
        Buffer: name.as_mut_ptr(),
    };
    let attributes = OBJECT_ATTRIBUTES {
        Length: size_of::<OBJECT_ATTRIBUTES>() as u32,
        RootDirectory: parent.as_raw_handle().cast(),
        ObjectName: &unicode_name,
        Attributes: 0,
        SecurityDescriptor: ptr::null(),
        SecurityQualityOfService: ptr::null(),
    };
    let access = READ_CONTROL
        | WRITE_DAC
        | SYNCHRONIZE
        | FILE_READ_ATTRIBUTES
        | FILE_WRITE_ATTRIBUTES
        | if for_sync && !directory { FILE_WRITE_DATA } else { 0 }
        | if directory {
            FILE_LIST_DIRECTORY | FILE_TRAVERSE
        } else {
            0
        };
    let mut handle = ptr::null_mut();
    let mut io_status = unsafe { std::mem::zeroed::<IO_STATUS_BLOCK>() };
    let status = unsafe {
        NtCreateFile(
            &mut handle,
            access,
            &attributes,
            &mut io_status,
            ptr::null(),
            0,
            0x7,
            FILE_OPEN,
            FILE_SYNCHRONOUS_IO_NONALERT
                | FILE_OPEN_REPARSE_POINT
                | if directory {
                    FILE_DIRECTORY_FILE
                } else {
                    FILE_NON_DIRECTORY_FILE
                },
            ptr::null(),
            0,
        )
    };
    if status < 0 {
        return Err(nt_status_error(status));
    }
    let handle = NativeHandle(handle);
    let file = unsafe { File::from_raw_handle(handle.0.cast()) };
    std::mem::forget(handle);
    let metadata = file.metadata()?;
    use std::os::windows::fs::MetadataExt;
    if metadata.file_attributes() & windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT != 0
        || (directory && !metadata.is_dir())
        || (!directory && !metadata.is_file())
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "entry is a reparse point or has an unexpected type",
        ));
    }
    Ok(file)
}

/// Opens a Host file through its retained parent with DACL and attribute
/// rights.
pub(crate) fn open_existing_host_file(path: &std::path::Path) -> io::Result<File> {
    let parent_path = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "file path has no parent"))?;
    let name = path
        .file_name()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "file path has no entry name"))?;
    let parent = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS)
        .open(parent_path)?;
    open_existing_entry_at(&parent, name, false)
}

/// Opens an existing relative path one component at a time with no-follow
/// semantics.
pub(crate) fn open_existing_relative(parent: &File, relative: &std::path::Path, directory: bool) -> io::Result<File> {
    let mut current = parent.try_clone()?;
    let mut components = relative.components().peekable();
    while let Some(component) = components.next() {
        let std::path::Component::Normal(name) = component else {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "path must contain only normal components",
            ));
        };
        let is_directory = components.peek().is_some() || directory;
        current = open_existing_entry_at(&current, name, is_directory)?;
    }
    Ok(current)
}

/// Creates a file relative to a retained parent handle with a protected
/// current-user DACL.
pub(crate) fn create_private_file_at(parent: &File, name: &OsStr) -> Result<File, NativePrivateCreateError> {
    create_private_entry_at(parent, name, false)
}

/// Creates a directory relative to a retained parent handle with a protected
/// current-user DACL.
pub(crate) fn create_private_directory_at(parent: &File, name: &OsStr) -> Result<File, NativePrivateCreateError> {
    create_private_entry_at(parent, name, true)
}

pub(crate) fn create_owner_entry_at(
    parent: &File,
    name: &OsStr,
    directory: bool,
    policy: &LocalOwnerAccessPolicy,
) -> Result<File, NativePrivateCreateError> {
    let mask = match owner_access_mask(policy, directory) {
        Ok(mask) => mask,
        Err(source) => {
            return Err(NativePrivateCreateError { source, retained: None });
        }
    };
    let retained_parent = parent
        .try_clone()
        .map_err(|source| NativePrivateCreateError { source, retained: None })?;
    let retained_name = name.to_os_string();
    let file = create_at_with_mask(parent, name, directory, mask, mask)
        .map_err(|source| NativePrivateCreateError { source, retained: None })?;
    if let Err(source) = verify_owner_access(&file, directory, policy) {
        let identity = crate::observation::query::query_observation(
            &file,
            crate::observation::LocalObservationRequirement::Optional,
        )
        .ok()
        .and_then(|observation| observation.identity().cloned());
        return Err(NativePrivateCreateError {
            source,
            retained: Some(NativePrivateEntry {
                parent: retained_parent,
                name: retained_name,
                file: Some(file),
                identity,
                directory,
            }),
        });
    }
    Ok(file)
}

fn create_private_entry_at(parent: &File, name: &OsStr, directory: bool) -> Result<File, NativePrivateCreateError> {
    let retained_parent = parent
        .try_clone()
        .map_err(|source| NativePrivateCreateError { source, retained: None })?;
    let retained_name = name.to_os_string();
    let file =
        create_at(parent, name, directory).map_err(|source| NativePrivateCreateError { source, retained: None })?;
    if let Err(source) = verify_current_user_access(&file, directory, false) {
        let identity = crate::observation::query::query_observation(
            &file,
            crate::observation::LocalObservationRequirement::Optional,
        )
        .ok()
        .and_then(|observation| observation.identity().cloned());
        return Err(NativePrivateCreateError {
            source,
            retained: Some(NativePrivateEntry {
                parent: retained_parent,
                name: retained_name,
                file: Some(file),
                identity,
                directory,
            }),
        });
    }
    Ok(file)
}

/// Applies the current user's access policy to an already-open entry.
pub(crate) fn apply_current_user_access(file: &File, directory: bool, read_only: bool) -> io::Result<()> {
    let descriptor = SecurityDescriptor::for_current_user(directory, read_only)?;
    check_failure!(AccessApplyBefore);
    let status = unsafe {
        windows_sys::Win32::Security::Authorization::SetSecurityInfo(
            file.as_raw_handle().cast(),
            windows_sys::Win32::Security::Authorization::SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
            ptr::null_mut(),
            ptr::null_mut(),
            descriptor.acl.as_ptr().cast::<ACL>(),
            ptr::null_mut(),
        )
    };
    if status != 0 {
        return Err(io::Error::from_raw_os_error(status as i32));
    }
    check_failure!(AclApply);
    if read_only && !directory {
        check_failure!(ReadOnly);
        // Attribute changes are performed through the retained handle, not a path.
        let mut information = unsafe { std::mem::zeroed::<windows_sys::Win32::Storage::FileSystem::FILE_BASIC_INFO>() };
        if unsafe {
            windows_sys::Win32::Storage::FileSystem::GetFileInformationByHandleEx(
                file.as_raw_handle().cast(),
                windows_sys::Win32::Storage::FileSystem::FileBasicInfo,
                (&mut information as *mut windows_sys::Win32::Storage::FileSystem::FILE_BASIC_INFO).cast(),
                size_of::<windows_sys::Win32::Storage::FileSystem::FILE_BASIC_INFO>() as u32,
            )
        } == 0
        {
            return Err(last_os_error());
        }
        information.FileAttributes = windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_READONLY;
        if unsafe {
            windows_sys::Win32::Storage::FileSystem::SetFileInformationByHandle(
                file.as_raw_handle().cast(),
                windows_sys::Win32::Storage::FileSystem::FileBasicInfo,
                (&information as *const windows_sys::Win32::Storage::FileSystem::FILE_BASIC_INFO).cast(),
                size_of::<windows_sys::Win32::Storage::FileSystem::FILE_BASIC_INFO>() as u32,
            )
        } == 0
        {
            return Err(last_os_error());
        }
    }
    verify_current_user_access(file, directory, read_only)
}

pub(crate) fn apply_owner_access(file: &File, directory: bool, policy: &LocalOwnerAccessPolicy) -> io::Result<()> {
    let mask = owner_access_mask(policy, directory)?;
    let descriptor = SecurityDescriptor::for_current_user_with_mask(mask)?;
    check_failure!(AccessApplyBefore);
    let status = unsafe {
        windows_sys::Win32::Security::Authorization::SetSecurityInfo(
            file.as_raw_handle().cast(),
            windows_sys::Win32::Security::Authorization::SE_FILE_OBJECT,
            DACL_SECURITY_INFORMATION | PROTECTED_DACL_SECURITY_INFORMATION,
            ptr::null_mut(),
            ptr::null_mut(),
            descriptor.acl.as_ptr().cast::<ACL>(),
            ptr::null_mut(),
        )
    };
    if status != 0 {
        return Err(io::Error::from_raw_os_error(status as i32));
    }
    check_failure!(AclApply);
    verify_owner_access(file, directory, policy)
}

/// Verifies owner, protected DACL, current-user ACE and required read-only
/// attribute.
pub(crate) fn verify_current_user_access(file: &File, directory: bool, read_only: bool) -> io::Result<()> {
    verify_access_mask(file, directory, default_access_mask(directory, read_only), read_only)
}

pub(crate) fn verify_owner_access(file: &File, directory: bool, policy: &LocalOwnerAccessPolicy) -> io::Result<()> {
    let mask = owner_access_mask(policy, directory)?;
    verify_access_mask(file, directory, mask, false)
}

fn verify_access_mask(file: &File, directory: bool, expected_access: u32, read_only: bool) -> io::Result<()> {
    check_failure!(AclReadback);
    let current_user = CurrentUser::acquire()?;
    let current_sid = current_user.sid()?;
    let mut owner: PSID = ptr::null_mut();
    let mut dacl: *mut ACL = ptr::null_mut();
    let mut descriptor_ptr: PSECURITY_DESCRIPTOR = ptr::null_mut();
    let mut dacl_present = 0;
    let mut dacl_defaulted = 0;
    let mut control = 0_u16;
    let mut revision = 0_u32;
    let needed = unsafe {
        windows_sys::Win32::Security::Authorization::GetSecurityInfo(
            file.as_raw_handle().cast(),
            windows_sys::Win32::Security::Authorization::SE_FILE_OBJECT,
            windows_sys::Win32::Security::OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
            &mut owner,
            ptr::null_mut(),
            &mut dacl,
            ptr::null_mut(),
            &mut descriptor_ptr,
        )
    };
    if needed != 0 {
        return Err(io::Error::from_raw_os_error(needed as i32));
    }
    // GetSecurityInfo allocates this descriptor with LocalAlloc.
    let _descriptor = LocalSecurityDescriptor(descriptor_ptr);
    if owner.is_null() || unsafe { windows_sys::Win32::Security::EqualSid(owner, current_sid) } == 0 {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "owner SID is not current user",
        ));
    }
    if unsafe { GetSecurityDescriptorControl(descriptor_ptr, &mut control, &mut revision) } == 0
        || control & SE_DACL_PROTECTED == 0
    {
        return Err(io::Error::new(io::ErrorKind::PermissionDenied, "DACL is not protected"));
    }
    if unsafe { GetSecurityDescriptorDacl(descriptor_ptr, &mut dacl_present, &mut dacl, &mut dacl_defaulted) } == 0
        || dacl_present == 0
        || dacl.is_null()
    {
        return Err(io::Error::new(io::ErrorKind::PermissionDenied, "missing DACL"));
    }
    let ace_count = unsafe { (*dacl).AceCount };
    if ace_count != 1 {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "unexpected DACL ACE count",
        ));
    }
    let mut ace = ptr::null_mut();
    if unsafe { GetAce(dacl, 0, &mut ace) } == 0 {
        return Err(last_os_error());
    }
    let ace_header = unsafe { &*ace.cast::<windows_sys::Win32::Security::ACE_HEADER>() };
    if ace_header.AceType != 0 || ace_header.AceFlags != 0 {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "unexpected ACE type or flags",
        ));
    }
    let allowed = unsafe { &*ace.cast::<ACCESS_ALLOWED_ACE>() };
    let ace_sid = ptr::addr_of!(allowed.SidStart).cast_mut().cast();
    if unsafe { windows_sys::Win32::Security::EqualSid(ace_sid, current_sid) } == 0 || allowed.Mask != expected_access {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "DACL does not match current-user policy",
        ));
    }
    if read_only && !directory {
        let metadata = file.metadata()?;
        if !metadata.permissions().readonly() {
            return Err(io::Error::new(io::ErrorKind::PermissionDenied, "file is not read-only"));
        }
    }
    Ok(())
}

#[derive(Debug)]
struct LocalSecurityDescriptor(PSECURITY_DESCRIPTOR);

impl Drop for LocalSecurityDescriptor {
    fn drop(&mut self) {
        if !self.0.is_null() {
            // GetSecurityInfo documents LocalFree as the matching allocator.
            unsafe { LocalFree(self.0.cast()) };
        }
    }
}

#[cfg(test)]
mod tests {
    use std::ffi::OsStr;
    use std::fs::OpenOptions;
    use std::os::windows::fs::OpenOptionsExt;

    use super::FailurePoint;
    use super::apply_owner_access;
    use super::create_owner_entry_at;
    use super::default_access_mask;
    use super::fail_once_at;
    use super::owner_access_mask;
    use super::verify_owner_access;
    use crate::policy::LocalOwnerAccessPolicy;

    #[test]
    fn owner_policy_windows_accepts_distinct_exact_masks() {
        let policy = LocalOwnerAccessPolicy::new().with_windows_masks(0x0013_019f, 0x0012_0089);

        assert_eq!(owner_access_mask(&policy, true).expect("directory mask"), 0x0013_019f);
        assert_eq!(owner_access_mask(&policy, false).expect("file mask"), 0x0012_0089);
    }

    #[test]
    fn owner_policy_windows_rejects_missing_or_unsupported_masks() {
        let missing = LocalOwnerAccessPolicy::new();
        assert_eq!(
            owner_access_mask(&missing, false)
                .expect_err("missing mask must fail")
                .kind(),
            std::io::ErrorKind::InvalidInput,
        );

        for unsupported in [0x001f_0200, 0x0200_0000, 0x1000_0000, 0x0200_0000 | 0x0012_0089] {
            let policy = LocalOwnerAccessPolicy::new().with_windows_masks(0x0013_019f, unsupported);
            assert_eq!(
                owner_access_mask(&policy, false)
                    .expect_err("unsupported mask must fail before native mutation")
                    .kind(),
                std::io::ErrorKind::InvalidInput,
            );
        }
    }

    #[test]
    fn owner_policy_windows_keeps_default_access_masks_unchanged() {
        use super::DELETE;
        use super::DIRECTORY_ACCESS;
        use super::FILE_ADD_FILE;
        use super::FILE_ADD_SUBDIRECTORY;
        use super::FILE_APPEND_DATA;
        use super::FILE_DELETE_CHILD;
        use super::FILE_INITIAL_ACCESS;
        use super::FILE_WRITE_ATTRIBUTES;
        use super::FILE_WRITE_DATA;

        assert_eq!(default_access_mask(false, false), FILE_INITIAL_ACCESS);
        assert_eq!(default_access_mask(true, false), DIRECTORY_ACCESS);
        assert_eq!(
            default_access_mask(false, true),
            FILE_INITIAL_ACCESS & !(FILE_WRITE_DATA | FILE_APPEND_DATA),
        );
        assert_eq!(
            default_access_mask(true, true),
            DIRECTORY_ACCESS
                & !(FILE_ADD_FILE | FILE_ADD_SUBDIRECTORY | FILE_DELETE_CHILD | FILE_WRITE_ATTRIBUTES | DELETE),
        );
    }

    fn test_parent() -> (tempfile::TempDir, std::fs::File) {
        let directory = tempfile::tempdir().expect("temporary directory");
        let parent = OpenOptions::new()
            .read(true)
            .custom_flags(super::FILE_FLAG_BACKUP_SEMANTICS)
            .open(directory.path())
            .expect("open temporary directory handle");
        (directory, parent)
    }

    #[test]
    fn owner_policy_windows_create_apply_verify_uses_exact_dacl_mask() {
        let (_directory, parent) = test_parent();
        let policy = LocalOwnerAccessPolicy::new().with_windows_masks(0x0016_0189, 0x0016_0089);
        let file = create_owner_entry_at(&parent, OsStr::new("owner-policy-entry"), false, &policy)
            .expect("create exact-owner file");

        verify_owner_access(&file, false, &policy).expect("verify created owner ACL");
        apply_owner_access(&file, false, &policy).expect("reapply exact owner ACL");
        verify_owner_access(&file, false, &policy).expect("verify applied owner ACL");
    }

    #[test]
    fn owner_policy_windows_directory_create_apply_verify_uses_exact_dacl_mask() {
        let (_directory, parent) = test_parent();
        let policy = LocalOwnerAccessPolicy::new().with_windows_masks(0x0016_0189, 0x0016_0089);
        let directory = create_owner_entry_at(&parent, OsStr::new("owner-policy-directory"), true, &policy)
            .expect("create exact-owner directory");

        verify_owner_access(&directory, true, &policy).expect("verify created directory ACL");
        apply_owner_access(&directory, true, &policy).expect("reapply exact directory ACL");
        verify_owner_access(&directory, true, &policy).expect("verify applied directory ACL");
    }

    #[test]
    fn owner_policy_windows_apply_without_write_dac_fails_without_acl_expansion() {
        let (_directory, parent) = test_parent();
        let policy = LocalOwnerAccessPolicy::new().with_windows_masks(0x0012_0189, 0x0012_0089);
        let file = create_owner_entry_at(&parent, OsStr::new("owner-policy-read-control"), false, &policy)
            .expect("create owner file with read-control access");

        assert!(apply_owner_access(&file, false, &policy).is_err());
        verify_owner_access(&file, false, &policy).expect("failed apply must preserve the exact ACL");
    }

    #[test]
    fn owner_policy_windows_missing_read_control_retains_created_resource() {
        let (_directory, parent) = test_parent();
        let policy = LocalOwnerAccessPolicy::new().with_windows_masks(0x0010_0189, 0x0010_0089);

        let error = create_owner_entry_at(&parent, OsStr::new("owner-policy-no-read-control"), false, &policy)
            .expect_err("readback without READ_CONTROL must fail");
        assert!(
            error.retained.is_some(),
            "created object must retain its recovery owner"
        );
    }

    #[test]
    fn owner_policy_windows_verification_failure_retains_created_resource() {
        let (_directory, parent) = test_parent();
        let policy = LocalOwnerAccessPolicy::new().with_windows_masks(0x0016_0189, 0x0016_0089);
        fail_once_at(FailurePoint::AclReadback);

        let error = create_owner_entry_at(&parent, OsStr::new("owner-policy-recovery"), false, &policy)
            .expect_err("injected verification failure");
        assert!(
            error.retained.is_some(),
            "created object must retain its recovery owner"
        );
    }
}
