// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Windows private temporary-resource creation and access control.

mod current_user;
mod local_security_descriptor;
mod native_handle;
mod security_descriptor;
mod token;

#[cfg(test)]
use std::cell::Cell;
use std::ffi::OsStr;
use std::ffi::OsString;
use std::fs::File;
use std::fs::OpenOptions;
use std::io;
use std::mem::forget;
use std::mem::size_of;
use std::mem::zeroed;
use std::os::windows::ffi::OsStrExt;
use std::os::windows::fs::MetadataExt;
use std::os::windows::fs::OpenOptionsExt;
use std::os::windows::io::AsRawHandle;
use std::os::windows::io::FromRawHandle;
use std::path::Component;
use std::path::Path;
use std::ptr;
use std::sync::Arc;

use windows_sys::Wdk::Foundation::OBJECT_ATTRIBUTES;
use windows_sys::Wdk::Storage::FileSystem::FILE_CREATE;
use windows_sys::Wdk::Storage::FileSystem::FILE_DIRECTORY_FILE;
use windows_sys::Wdk::Storage::FileSystem::FILE_NON_DIRECTORY_FILE;
use windows_sys::Wdk::Storage::FileSystem::FILE_OPEN;
use windows_sys::Wdk::Storage::FileSystem::FILE_OPEN_REPARSE_POINT;
use windows_sys::Wdk::Storage::FileSystem::FILE_SYNCHRONOUS_IO_NONALERT;
use windows_sys::Wdk::Storage::FileSystem::NtCreateFile;
use windows_sys::Wdk::Storage::FileSystem::RtlNtStatusToDosErrorNoTeb;
use windows_sys::Win32::Foundation::GetLastError;
use windows_sys::Win32::Foundation::UNICODE_STRING;
use windows_sys::Win32::Security::ACCESS_ALLOWED_ACE;
use windows_sys::Win32::Security::ACE_HEADER;
use windows_sys::Win32::Security::ACL;
use windows_sys::Win32::Security::Authorization::GetSecurityInfo;
use windows_sys::Win32::Security::Authorization::SE_FILE_OBJECT;
use windows_sys::Win32::Security::Authorization::SetSecurityInfo;
use windows_sys::Win32::Security::DACL_SECURITY_INFORMATION;
use windows_sys::Win32::Security::EqualSid;
use windows_sys::Win32::Security::GetAce;
use windows_sys::Win32::Security::GetSecurityDescriptorControl;
use windows_sys::Win32::Security::GetSecurityDescriptorDacl;
use windows_sys::Win32::Security::OWNER_SECURITY_INFORMATION;
use windows_sys::Win32::Security::PROTECTED_DACL_SECURITY_INFORMATION;
use windows_sys::Win32::Security::PSECURITY_DESCRIPTOR;
use windows_sys::Win32::Security::PSID;
use windows_sys::Win32::Security::SE_DACL_PROTECTED;
use windows_sys::Win32::Storage::FileSystem::DELETE;
use windows_sys::Win32::Storage::FileSystem::FILE_ADD_FILE;
use windows_sys::Win32::Storage::FileSystem::FILE_ADD_SUBDIRECTORY;
use windows_sys::Win32::Storage::FileSystem::FILE_APPEND_DATA;
use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_NORMAL;
use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_READONLY;
use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_REPARSE_POINT;
use windows_sys::Win32::Storage::FileSystem::FILE_BASIC_INFO;
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
use windows_sys::Win32::Storage::FileSystem::FileBasicInfo;
use windows_sys::Win32::Storage::FileSystem::GetFileInformationByHandleEx;
use windows_sys::Win32::Storage::FileSystem::READ_CONTROL;
use windows_sys::Win32::Storage::FileSystem::SYNCHRONIZE;
use windows_sys::Win32::Storage::FileSystem::SetFileInformationByHandle;
use windows_sys::Win32::Storage::FileSystem::WRITE_DAC;
use windows_sys::Win32::System::IO::IO_STATUS_BLOCK;

use self::current_user::CurrentUser;
use self::local_security_descriptor::LocalSecurityDescriptor;
use self::native_handle::NativeHandle;
use self::security_descriptor::SecurityDescriptor;
use crate::observation::LocalObservationRequirement;
use crate::observation::query::query_observation;
use crate::policy::LocalOwnerAccessPolicy;
use crate::temp::types::NativePrivateCreateError;
use crate::temp::types::NativePrivateEntry;

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

/// Injects a scheduled native-operation failure in test builds.
macro_rules! check_failure {
    ($point:ident) => {{
        #[cfg(test)]
        fail_if(FailurePoint::$point)?;
    }};
}

/// Returns this module's default DACL mask for a file or directory.
///
/// # Returns
///
/// A writable request returns the corresponding full initial mask. For a
/// read-only file, only `FILE_WRITE_DATA` and `FILE_APPEND_DATA` are removed.
/// For a read-only directory, only `FILE_ADD_FILE`, `FILE_ADD_SUBDIRECTORY`,
/// `FILE_DELETE_CHILD`, `FILE_WRITE_ATTRIBUTES`, and `DELETE` are removed;
/// other bits in the initial masks remain set.
///
/// # Parameters
///
/// * `directory` selects file or directory access bits.
/// * `read_only` removes the documented write bits when true.
#[must_use]
#[inline]
pub(super) fn default_access_mask(directory: bool, read_only: bool) -> u32 {
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

/// Selects a configured Windows owner-policy mask after validating both masks.
///
/// Returns `InvalidInput` if Windows masks are absent or either the directory
/// or file mask contains any bit outside `OWNER_ACCESS_MASKS`. Otherwise
/// returns the directory mask when `directory` is true and the file mask when
/// it is false.
///
/// # Parameters
///
/// * `policy` supplies configured Windows masks.
/// * `directory` selects which validated mask to return.
///
/// # Returns
///
/// The validated directory or file access mask selected by `directory`.
///
/// # Errors
///
/// Returns `InvalidInput` if the policy has no Windows masks or includes
/// unsupported bits.
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

/// Converts the last Win32 error into an owned I/O error.
///
/// # Returns
///
/// The last system error for the current thread, wrapped as `io::Error`.
#[must_use]
fn last_os_error() -> io::Error {
    io::Error::from_raw_os_error(unsafe { GetLastError() } as i32)
}

/// Converts an NTSTATUS failure into its corresponding standard I/O error.
///
/// # Parameters
///
/// * `status` is the failing NTSTATUS returned by a native operation.
///
/// # Returns
///
/// The mapped Windows error wrapped as `io::Error`.
#[must_use]
fn nt_status_error(status: i32) -> io::Error {
    // SAFETY: RtlNtStatusToDosErrorNoTeb accepts any NTSTATUS value and does
    // not retain pointers.
    let error_code = unsafe { RtlNtStatusToDosErrorNoTeb(status) };
    io::Error::from_raw_os_error(error_code as i32)
}

/// Creates a child using the module's default access masks.
///
/// # Parameters
///
/// * `parent` is the retained directory handle for the new child.
/// * `name` is one child name, not a path.
/// * `directory` selects a file or directory child.
///
/// # Returns
///
/// An open handle to the newly created file or directory.
///
/// # Errors
///
/// Returns an error if native access, validation, or verification fails.
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

/// Creates a child with separate DACL and returned-handle access masks.
///
/// # Parameters
///
/// * `parent` is the retained directory handle for the new child.
/// * `name` is one child name, not a path.
/// * `directory` selects a file or directory child.
/// * `access_mask` is installed as the child DACL mask.
/// * `handle_access` requests rights for the returned handle.
///
/// # Returns
///
/// An open handle to the newly created file or directory.
///
/// # Errors
///
/// Returns an error if native access, validation, or verification fails.
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
    let mut io_status = unsafe { zeroed::<IO_STATUS_BLOCK>() };
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
    forget(handle);
    Ok(file)
}

/// Opens one existing child by native directory handle with rights to update
/// its DACL.
///
/// # Parameters
///
/// * `parent` is the directory handle containing the child.
/// * `name` is one child name, not a path.
/// * `directory` selects the expected child type.
///
/// # Returns
///
/// An open handle to the validated existing child.
///
/// # Errors
///
/// Returns an error if native access, validation, or verification fails.
pub(crate) fn open_existing_entry_at(parent: &File, name: &OsStr, directory: bool) -> io::Result<File> {
    open_existing_entry_at_with_sync(parent, name, directory, false)
}

/// Opens an existing child with write access when synchronizing file contents.
///
/// # Parameters
///
/// * `parent` is the directory handle containing the child.
/// * `name` is one child name, not a path.
/// * `directory` selects the expected child type.
///
/// # Returns
///
/// An open handle to the validated child with file-write access requested for
/// synchronization.
///
/// # Errors
///
/// Returns an error if native access, validation, or verification fails.
pub(crate) fn open_existing_entry_at_for_sync(parent: &File, name: &OsStr, directory: bool) -> io::Result<File> {
    open_existing_entry_at_with_sync(parent, name, directory, true)
}

/// Opens and validates one child, optionally requesting file-write access.
///
/// # Parameters
///
/// * `parent` is the directory handle containing the child.
/// * `name` is one child name, not a path.
/// * `directory` selects the expected child type.
/// * `for_sync` requests file write access when the child is a file.
///
/// # Returns
///
/// An open handle to the validated existing child.
///
/// # Errors
///
/// Returns an error if native access, validation, or verification fails.
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
    let mut io_status = unsafe { zeroed::<IO_STATUS_BLOCK>() };
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
    forget(handle);
    verify_open_entry_type(&file, directory)?;
    Ok(file)
}

/// Rejects reparse points and entries whose native type differs from the
/// request.
///
/// # Parameters
///
/// * `file` is the open handle returned by the native open operation.
/// * `directory` selects the expected file or directory type.
///
/// # Returns
///
/// Returns `()` when metadata confirms a non-reparse entry of the expected
/// kind.
///
/// # Errors
///
/// Returns the metadata I/O error or `InvalidInput` for a reparse point or
/// wrong entry kind.
fn verify_open_entry_type(file: &File, directory: bool) -> io::Result<()> {
    let metadata = file.metadata()?;
    if metadata.file_attributes() & FILE_ATTRIBUTE_REPARSE_POINT != 0
        || (directory && !metadata.is_dir())
        || (!directory && !metadata.is_file())
    {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "entry is a reparse point or has an unexpected type",
        ));
    }
    Ok(())
}

/// Opens a Host file through its retained parent with DACL and attribute
/// rights.
///
/// # Parameters
///
/// * `path` is the host file path whose parent and entry are opened separately.
///
/// # Returns
///
/// An open handle to the validated host file.
///
/// # Errors
///
/// Returns an error if native access, validation, or verification fails.
pub(crate) fn open_existing_host_file(path: &Path) -> io::Result<File> {
    let parent_path = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "file path has no parent"))?;
    let name = path
        .file_name()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "file path has no entry name"))?;
    let parent = OpenOptions::new()
        .read(true)
        .custom_flags(FILE_FLAG_BACKUP_SEMANTICS)
        .open(parent_path)?;
    open_existing_entry_at(&parent, name, false)
}

/// Opens an existing relative path one component at a time with no-follow
/// semantics.
///
/// # Parameters
///
/// * `parent` is the starting directory handle.
/// * `relative` contains only normal path components.
/// * `directory` selects the expected type of the final component.
///
/// # Returns
///
/// An open handle to the final validated path component.
///
/// # Errors
///
/// Returns an error if native access, validation, or verification fails.
pub(crate) fn open_existing_relative(parent: &File, relative: &Path, directory: bool) -> io::Result<File> {
    let mut current = parent.try_clone()?;
    let mut components = relative.components().peekable();
    while let Some(component) = components.next() {
        let Component::Normal(name) = component else {
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
///
/// # Parameters
///
/// * `parent` is the retained parent directory handle.
/// * `name` is one child name, not a path.
///
/// # Returns
///
/// The newly created private file handle.
///
/// # Errors
///
/// Returns an error if native access, validation, or verification fails.
pub(crate) fn create_private_file_at(parent: &File, name: &OsStr) -> Result<File, NativePrivateCreateError> {
    create_private_entry_at(parent, name, false)
}

/// Creates a directory relative to a retained parent handle with a protected
/// current-user DACL.
///
/// # Parameters
///
/// * `parent` is the retained parent directory handle.
/// * `name` is one child name, not a path.
///
/// # Returns
///
/// The newly created private directory handle.
///
/// # Errors
///
/// Returns an error if native access, validation, or verification fails.
pub(crate) fn create_private_directory_at(parent: &File, name: &OsStr) -> Result<File, NativePrivateCreateError> {
    create_private_entry_at(parent, name, true)
}

/// Creates an owner-policy entry and retains it for recovery if verification
/// fails.
///
/// # Parameters
///
/// * `parent` is the retained parent directory handle.
/// * `name` is one child name, not a path.
/// * `directory` selects a file or directory child.
/// * `policy` supplies the exact supported Windows access masks.
///
/// # Returns
///
/// The created file or directory after exact owner-policy verification.
///
/// # Errors
///
/// Returns the creation or verification error and retains a created entry when
/// recovery is needed.
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
        return Err(retain_created_entry_after_verification_failure(
            retained_parent,
            retained_name,
            file,
            directory,
            source,
        ));
    }
    Ok(file)
}

/// Creates and verifies an entry using the default current-user access policy.
///
/// # Parameters
///
/// * `parent` is the retained parent directory handle.
/// * `name` is one child name, not a path.
/// * `directory` selects a file or directory child.
///
/// # Returns
///
/// The created file or directory after current-user access verification.
///
/// # Errors
///
/// Returns the creation or verification error and retains a created entry when
/// recovery is needed.
fn create_private_entry_at(parent: &File, name: &OsStr, directory: bool) -> Result<File, NativePrivateCreateError> {
    let retained_parent = parent
        .try_clone()
        .map_err(|source| NativePrivateCreateError { source, retained: None })?;
    let retained_name = name.to_os_string();
    let file =
        create_at(parent, name, directory).map_err(|source| NativePrivateCreateError { source, retained: None })?;
    if let Err(source) = verify_current_user_access(&file, directory, false) {
        return Err(retain_created_entry_after_verification_failure(
            retained_parent,
            retained_name,
            file,
            directory,
            source,
        ));
    }
    Ok(file)
}

/// Retains a newly created object and its recovery identity after verification
/// fails.
///
/// # Parameters
///
/// * `parent` is the already-cloned directory handle used for recovery.
/// * `name` is the created child name.
/// * `file` is the created object whose verification failed.
/// * `directory` records the created object's expected kind.
/// * `source` is the verification error preserved for the caller.
#[must_use]
fn retain_created_entry_after_verification_failure(
    parent: File,
    name: OsString,
    file: File,
    directory: bool,
    source: io::Error,
) -> NativePrivateCreateError {
    let identity = query_observation(&file, LocalObservationRequirement::Optional)
        .ok()
        .and_then(|observation| observation.identity().cloned());
    NativePrivateCreateError {
        source,
        retained: Some(NativePrivateEntry {
            parent: Arc::new(parent),
            name,
            file: Some(Arc::new(file)),
            identity,
            directory,
        }),
    }
}

/// Applies the current user's access policy to an already-open entry.
///
/// For files, `read_only` controls the DACL and whether this function sets the
/// `FILE_ATTRIBUTE_READONLY` attribute. Passing `false` does not clear a
/// read-only attribute that was already present.
///
/// # Parameters
///
/// * `file` is the open entry whose DACL is updated.
/// * `directory` selects the default file or directory mask.
/// * `read_only` selects the read-only mask and file attribute behavior.
///
/// # Returns
///
/// Returns `()` after applying and verifying the current-user access policy.
///
/// # Errors
///
/// Returns an error if native access, validation, or verification fails.
pub(crate) fn apply_current_user_access(file: &File, directory: bool, read_only: bool) -> io::Result<()> {
    let descriptor = SecurityDescriptor::for_current_user(directory, read_only)?;
    check_failure!(AccessApplyBefore);
    let status = unsafe {
        SetSecurityInfo(
            file.as_raw_handle().cast(),
            SE_FILE_OBJECT,
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
        let mut information = unsafe { zeroed::<FILE_BASIC_INFO>() };
        if unsafe {
            GetFileInformationByHandleEx(
                file.as_raw_handle().cast(),
                FileBasicInfo,
                (&mut information as *mut FILE_BASIC_INFO).cast(),
                size_of::<FILE_BASIC_INFO>() as u32,
            )
        } == 0
        {
            return Err(last_os_error());
        }
        information.FileAttributes = (information.FileAttributes & !FILE_ATTRIBUTE_NORMAL) | FILE_ATTRIBUTE_READONLY;
        if unsafe {
            SetFileInformationByHandle(
                file.as_raw_handle().cast(),
                FileBasicInfo,
                (&information as *const FILE_BASIC_INFO).cast(),
                size_of::<FILE_BASIC_INFO>() as u32,
            )
        } == 0
        {
            return Err(last_os_error());
        }
    }
    verify_current_user_access(file, directory, read_only)
}

/// Applies and verifies the configured owner access policy on an open entry.
///
/// # Parameters
///
/// * `file` is the open entry whose DACL is updated.
/// * `directory` selects the policy directory or file mask.
/// * `policy` supplies the exact supported Windows access masks.
///
/// # Returns
///
/// Returns `()` after applying and verifying the configured owner access
/// policy.
///
/// # Errors
///
/// Returns an error if native access, validation, or verification fails.
pub(crate) fn apply_owner_access(file: &File, directory: bool, policy: &LocalOwnerAccessPolicy) -> io::Result<()> {
    let mask = owner_access_mask(policy, directory)?;
    let descriptor = SecurityDescriptor::for_current_user_with_mask(mask)?;
    check_failure!(AccessApplyBefore);
    let status = unsafe {
        SetSecurityInfo(
            file.as_raw_handle().cast(),
            SE_FILE_OBJECT,
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
///
/// # Parameters
///
/// * `file` is the open entry to inspect.
/// * `directory` selects the expected file or directory access mask.
/// * `read_only` requires the read-only file attribute for files.
///
/// # Returns
///
/// Returns `()` when the owner, protected DACL, exact mask, and requested
/// read-only state pass verification.
///
/// # Errors
///
/// Returns an error if native access, validation, or verification fails.
pub(crate) fn verify_current_user_access(file: &File, directory: bool, read_only: bool) -> io::Result<()> {
    verify_access_mask(file, directory, default_access_mask(directory, read_only), read_only)
}

/// Verifies that an entry has the exact configured owner access mask.
///
/// # Parameters
///
/// * `file` is the open entry to inspect.
/// * `directory` selects the policy directory or file mask.
/// * `policy` supplies the expected supported Windows access mask.
///
/// # Returns
///
/// Returns `()` when the owner and protected DACL match the configured access
/// mask.
///
/// # Errors
///
/// Returns an error if native access, validation, or verification fails.
pub(crate) fn verify_owner_access(file: &File, directory: bool, policy: &LocalOwnerAccessPolicy) -> io::Result<()> {
    let mask = owner_access_mask(policy, directory)?;
    verify_access_mask(file, directory, mask, false)
}

/// Checks the owner, protected DACL, sole ACE, exact mask, and optional file
/// flag.
///
/// # Parameters
///
/// * `file` is the open entry to inspect.
/// * `directory` selects the expected file or directory type.
/// * `expected_access` is the exact mask required in the sole allow ACE.
/// * `read_only` requires the read-only file attribute for files.
///
/// # Returns
///
/// Returns `()` when all requested ownership, DACL, ACE, mask, and read-only
/// checks pass.
///
/// # Errors
///
/// Returns an error if native access, validation, or verification fails.
fn verify_access_mask(file: &File, directory: bool, expected_access: u32, read_only: bool) -> io::Result<()> {
    check_failure!(AclReadback);
    let current_user = CurrentUser::acquire()?;
    let current_sid = current_user.sid()?;
    let mut owner: PSID = ptr::null_mut();
    let mut dacl: *mut ACL = ptr::null_mut();
    let mut descriptor_ptr: PSECURITY_DESCRIPTOR = ptr::null_mut();
    let needed = unsafe {
        GetSecurityInfo(
            file.as_raw_handle().cast(),
            SE_FILE_OBJECT,
            OWNER_SECURITY_INFORMATION | DACL_SECURITY_INFORMATION,
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
    if owner.is_null() || unsafe { EqualSid(owner, current_sid) } == 0 {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "owner SID is not current user",
        ));
    }
    verify_protected_dacl(descriptor_ptr, &mut dacl)?;
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
    let ace_header = unsafe { &*ace.cast::<ACE_HEADER>() };
    if ace_header.AceType != 0 || ace_header.AceFlags != 0 {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "unexpected ACE type or flags",
        ));
    }
    let allowed = unsafe { &*ace.cast::<ACCESS_ALLOWED_ACE>() };
    let ace_sid = ptr::addr_of!(allowed.SidStart).cast_mut().cast();
    if unsafe { EqualSid(ace_sid, current_sid) } == 0 || allowed.Mask != expected_access {
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

/// Confirms that a security descriptor has a protected DACL and retrieves its
/// pointer.
///
/// # Parameters
///
/// * `descriptor` is owned by the caller and remains alive during this check.
/// * `dacl` receives the pointer returned from the descriptor.
///
/// # Returns
///
/// Returns `()` when the descriptor contains a present, non-null protected
/// DACL.
///
/// # Errors
///
/// Returns `PermissionDenied` when the DACL is unprotected, missing, or cannot
/// be read.
fn verify_protected_dacl(descriptor: PSECURITY_DESCRIPTOR, dacl: &mut *mut ACL) -> io::Result<()> {
    let mut control = 0_u16;
    let mut revision = 0_u32;
    if unsafe { GetSecurityDescriptorControl(descriptor, &mut control, &mut revision) } == 0
        || control & SE_DACL_PROTECTED == 0
    {
        return Err(io::Error::new(io::ErrorKind::PermissionDenied, "DACL is not protected"));
    }

    let mut dacl_present = 0;
    let mut dacl_defaulted = 0;
    if unsafe { GetSecurityDescriptorDacl(descriptor, &mut dacl_present, dacl, &mut dacl_defaulted) } == 0
        || dacl_present == 0
        || (*dacl).is_null()
    {
        return Err(io::Error::new(io::ErrorKind::PermissionDenied, "missing DACL"));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::io::ErrorKind;

    use super::default_access_mask;
    use super::owner_access_mask;
    use crate::policy::LocalOwnerAccessPolicy;

    #[test]
    fn test_owner_policy_windows_accepts_distinct_exact_masks() {
        let policy = LocalOwnerAccessPolicy::new().with_windows_masks(0x0013_019f, 0x0012_0089);

        assert_eq!(owner_access_mask(&policy, true).expect("directory mask"), 0x0013_019f);
        assert_eq!(owner_access_mask(&policy, false).expect("file mask"), 0x0012_0089);
    }

    #[test]
    fn test_owner_policy_windows_rejects_missing_or_unsupported_masks() {
        let missing = LocalOwnerAccessPolicy::new();
        assert_eq!(
            owner_access_mask(&missing, false)
                .expect_err("missing mask must fail")
                .kind(),
            ErrorKind::InvalidInput,
        );

        for unsupported in [0x001f_0200, 0x0200_0000, 0x1000_0000, 0x0200_0000 | 0x0012_0089] {
            let policy = LocalOwnerAccessPolicy::new().with_windows_masks(0x0013_019f, unsupported);
            assert_eq!(
                owner_access_mask(&policy, false)
                    .expect_err("unsupported mask must fail before native mutation")
                    .kind(),
                ErrorKind::InvalidInput,
            );
        }
    }

    #[test]
    fn test_owner_policy_windows_keeps_default_access_masks_unchanged() {
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
}
