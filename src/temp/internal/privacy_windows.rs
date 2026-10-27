// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Windows private temporary-resource creation and access control.

use std::ffi::OsStr;
use std::fs::File;
use std::io;
use std::mem::size_of;
use std::os::windows::ffi::OsStrExt;
use std::os::windows::io::AsRawHandle;
use std::os::windows::io::FromRawHandle;
use std::ptr;

use windows_sys::Wdk::Foundation::OBJECT_ATTRIBUTES;
use windows_sys::Wdk::Storage::FileSystem::FILE_CREATE;
use windows_sys::Wdk::Storage::FileSystem::FILE_DIRECTORY_FILE;
use windows_sys::Wdk::Storage::FileSystem::FILE_NON_DIRECTORY_FILE;
use windows_sys::Wdk::Storage::FileSystem::NtCreateFile;
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
use windows_sys::Win32::Storage::FileSystem::FILE_LIST_DIRECTORY;
use windows_sys::Win32::Storage::FileSystem::FILE_READ_ATTRIBUTES;
use windows_sys::Win32::Storage::FileSystem::FILE_READ_DATA;
use windows_sys::Win32::Storage::FileSystem::FILE_TRAVERSE;
use windows_sys::Win32::Storage::FileSystem::FILE_WRITE_ATTRIBUTES;
use windows_sys::Win32::Storage::FileSystem::FILE_WRITE_DATA;
use windows_sys::Win32::Storage::FileSystem::READ_CONTROL;
use windows_sys::Win32::Storage::FileSystem::WRITE_DAC;
use windows_sys::Win32::System::IO::IO_STATUS_BLOCK;
use windows_sys::Win32::System::Threading::GetCurrentProcess;
use windows_sys::Win32::System::Threading::OpenProcessToken;

use crate::temp::types::NativePrivateCreateError;
use crate::temp::types::NativePrivateEntry;

const ACL_BUFFER_SIZE: usize = 1024;
const FILE_INITIAL_ACCESS: u32 = FILE_READ_DATA
    | FILE_WRITE_DATA
    | FILE_APPEND_DATA
    | FILE_READ_ATTRIBUTES
    | FILE_WRITE_ATTRIBUTES
    | READ_CONTROL
    | WRITE_DAC
    | DELETE;
const DIRECTORY_ACCESS: u32 = FILE_LIST_DIRECTORY
    | FILE_TRAVERSE
    | FILE_ADD_FILE
    | FILE_ADD_SUBDIRECTORY
    | FILE_DELETE_CHILD
    | FILE_READ_ATTRIBUTES
    | FILE_WRITE_ATTRIBUTES
    | READ_CONTROL
    | WRITE_DAC
    | DELETE;

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
    token: Token,
    information: Vec<usize>,
}

impl CurrentUser {
    fn acquire() -> io::Result<Self> {
        let mut token = ptr::null_mut();
        // GetCurrentProcess is a pseudo-handle; OpenProcessToken returns an owned handle.
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
        Ok(Self { token, information })
    }

    fn sid(&self) -> PSID {
        // GetTokenInformation returns TOKEN_USER at the start of this owned buffer.
        unsafe { (*(self.information.as_ptr().cast::<TOKEN_USER>())).User.Sid }
    }
}

struct SecurityDescriptor {
    descriptor: SECURITY_DESCRIPTOR,
    acl: Vec<usize>,
}

impl SecurityDescriptor {
    fn for_current_user(directory: bool, read_only: bool) -> io::Result<Self> {
        let current_user = CurrentUser::acquire()?;
        let sid_length = unsafe { GetLengthSid(current_user.sid()) } as usize;
        if sid_length == 0 {
            return Err(last_os_error());
        }

        let mut acl = vec![0_usize; ACL_BUFFER_SIZE.div_ceil(size_of::<usize>())];
        let acl_ptr = acl.as_mut_ptr().cast::<ACL>();
        if unsafe {
            InitializeAcl(
                acl_ptr,
                (acl.len() * size_of::<usize>()) as u32,
                ACL_REVISION,
            )
        } == 0
        {
            return Err(last_os_error());
        }
        let access = if directory {
            DIRECTORY_ACCESS
        } else if read_only {
            FILE_INITIAL_ACCESS & !(FILE_WRITE_DATA | FILE_APPEND_DATA)
        } else {
            FILE_INITIAL_ACCESS
        };
        if unsafe { AddAccessAllowedAce(acl_ptr, ACL_REVISION, access, current_user.sid()) } == 0 {
            return Err(last_os_error());
        }

        let mut descriptor = unsafe { std::mem::zeroed::<SECURITY_DESCRIPTOR>() };
        if unsafe {
            InitializeSecurityDescriptor((&mut descriptor as *mut SECURITY_DESCRIPTOR).cast(), 1)
        } == 0
        {
            return Err(last_os_error());
        }
        if unsafe {
            SetSecurityDescriptorDacl(
                (&mut descriptor as *mut SECURITY_DESCRIPTOR).cast(),
                1,
                acl_ptr,
                0,
            )
        } == 0
        {
            return Err(last_os_error());
        }
        // NtCreateFile with this descriptor creates an explicit DACL without inheriting ACEs.
        descriptor.Control |= SE_DACL_PROTECTED;
        Ok(Self { descriptor, acl })
    }

    fn as_ptr(&self) -> PSECURITY_DESCRIPTOR {
        (&self.descriptor as *const SECURITY_DESCRIPTOR)
            .cast_mut()
            .cast()
    }
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

fn create_at(parent: &File, name: &OsStr, directory: bool) -> io::Result<File> {
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
            "invalid entry name",
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
    let mut attributes = OBJECT_ATTRIBUTES {
        Length: size_of::<OBJECT_ATTRIBUTES>() as u32,
        RootDirectory: parent.as_raw_handle().cast(),
        ObjectName: &unicode_name,
        Attributes: 0,
        SecurityDescriptor: ptr::null(),
        SecurityQualityOfService: ptr::null(),
    };
    let descriptor = SecurityDescriptor::for_current_user(directory, false)?;
    attributes.SecurityDescriptor = descriptor.as_ptr().cast();
    let mut handle = ptr::null_mut();
    let mut io_status = unsafe { std::mem::zeroed::<IO_STATUS_BLOCK>() };
    let status = unsafe {
        NtCreateFile(
            &mut handle,
            if directory {
                DIRECTORY_ACCESS
            } else {
                FILE_INITIAL_ACCESS
            },
            &attributes,
            &mut io_status,
            ptr::null(),
            0,
            0x7, // FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE
            FILE_CREATE,
            if directory {
                FILE_DIRECTORY_FILE
            } else {
                FILE_NON_DIRECTORY_FILE
            },
            ptr::null(),
            0,
        )
    };
    if status < 0 {
        return Err(io::Error::from_raw_os_error(status));
    }
    let handle = NativeHandle(handle);
    // Transfer the successfully created native handle into File ownership.
    let file = unsafe { File::from_raw_handle(handle.0.cast()) };
    std::mem::forget(handle);
    Ok(file)
}

/// Creates a file relative to a retained parent handle with a protected current-user DACL.
pub(crate) fn create_private_file_at(
    parent: &File,
    name: &OsStr,
) -> Result<File, NativePrivateCreateError> {
    create_private_entry_at(parent, name, false)
}

/// Creates a directory relative to a retained parent handle with a protected current-user DACL.
pub(crate) fn create_private_directory_at(
    parent: &File,
    name: &OsStr,
) -> Result<File, NativePrivateCreateError> {
    create_private_entry_at(parent, name, true)
}

fn create_private_entry_at(
    parent: &File,
    name: &OsStr,
    directory: bool,
) -> Result<File, NativePrivateCreateError> {
    let retained_parent = parent
        .try_clone()
        .map_err(|source| NativePrivateCreateError {
            source,
            retained: None,
        })?;
    let retained_name = name.to_os_string();
    let file = create_at(parent, name, directory).map_err(|source| NativePrivateCreateError {
        source,
        retained: None,
    })?;
    if let Err(source) = verify_current_user_access(&file, directory, false) {
        return Err(NativePrivateCreateError {
            source,
            retained: Some(NativePrivateEntry {
                parent: retained_parent,
                name: retained_name,
                file: Some(file),
                identity: None,
            }),
        });
    }
    Ok(file)
}

/// Applies the current user's access policy to an already-open entry.
pub(crate) fn apply_current_user_access(
    file: &File,
    directory: bool,
    read_only: bool,
) -> io::Result<()> {
    let descriptor = SecurityDescriptor::for_current_user(directory, read_only)?;
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
    if read_only && !directory {
        // Attribute changes are performed through the retained handle, not a path.
        let mut information = unsafe {
            std::mem::zeroed::<windows_sys::Win32::Storage::FileSystem::FILE_BASIC_INFO>()
        };
        if unsafe {
            windows_sys::Win32::Storage::FileSystem::GetFileInformationByHandleEx(
                file.as_raw_handle().cast(),
                windows_sys::Win32::Storage::FileSystem::FileBasicInfo,
                (&mut information as *mut windows_sys::Win32::Storage::FileSystem::FILE_BASIC_INFO)
                    .cast(),
                size_of::<windows_sys::Win32::Storage::FileSystem::FILE_BASIC_INFO>() as u32,
            )
        } == 0
        {
            return Err(last_os_error());
        }
        information.FileAttributes =
            windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_READONLY;
        if unsafe {
            windows_sys::Win32::Storage::FileSystem::SetFileInformationByHandle(
                file.as_raw_handle().cast(),
                windows_sys::Win32::Storage::FileSystem::FileBasicInfo,
                (&information as *const windows_sys::Win32::Storage::FileSystem::FILE_BASIC_INFO)
                    .cast(),
                size_of::<windows_sys::Win32::Storage::FileSystem::FILE_BASIC_INFO>() as u32,
            )
        } == 0
        {
            return Err(last_os_error());
        }
    }
    verify_current_user_access(file, directory, read_only)
}

/// Verifies owner, protected DACL, current-user ACE and required read-only attribute.
pub(crate) fn verify_current_user_access(
    file: &File,
    directory: bool,
    read_only: bool,
) -> io::Result<()> {
    let current_user = CurrentUser::acquire()?;
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
    if owner.is_null()
        || unsafe { windows_sys::Win32::Security::EqualSid(owner, current_user.sid()) } == 0
    {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "owner SID is not current user",
        ));
    }
    if unsafe { GetSecurityDescriptorControl(descriptor_ptr, &mut control, &mut revision) } == 0
        || control & SE_DACL_PROTECTED == 0
    {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "DACL is not protected",
        ));
    }
    if unsafe {
        GetSecurityDescriptorDacl(
            descriptor_ptr,
            &mut dacl_present,
            &mut dacl,
            &mut dacl_defaulted,
        )
    } == 0
        || dacl_present == 0
        || dacl.is_null()
    {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "missing DACL",
        ));
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
    let expected_access = if directory {
        DIRECTORY_ACCESS
    } else if read_only {
        FILE_INITIAL_ACCESS & !(FILE_WRITE_DATA | FILE_APPEND_DATA)
    } else {
        FILE_INITIAL_ACCESS
    };
    if unsafe { windows_sys::Win32::Security::EqualSid(ace_sid, current_user.sid()) } == 0
        || allowed.Mask != expected_access
    {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "DACL does not match current-user policy",
        ));
    }
    if read_only && !directory {
        let metadata = file.metadata()?;
        if metadata.permissions().readonly() == false {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                "file is not read-only",
            ));
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
