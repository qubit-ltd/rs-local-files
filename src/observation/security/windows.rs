// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::fmt;
use std::fs::File;
use std::io;
use std::mem::size_of;
use std::os::windows::io::AsRawHandle;
use std::os::windows::io::FromRawHandle;
use std::os::windows::io::OwnedHandle;
use std::ptr;

use windows_sys::Win32::Security::DACL_SECURITY_INFORMATION;
use windows_sys::Win32::Security::GetTokenInformation;
use windows_sys::Win32::Security::OWNER_SECURITY_INFORMATION;
use windows_sys::Win32::Security::TOKEN_QUERY;
use windows_sys::Win32::Security::TOKEN_USER;
use windows_sys::Win32::Security::TokenUser;
use windows_sys::Win32::Storage::FileSystem::FILE_ID_INFO;
use windows_sys::Win32::Storage::FileSystem::FILE_STANDARD_INFO;
use windows_sys::Win32::Storage::FileSystem::FileIdInfo;
use windows_sys::Win32::Storage::FileSystem::FileStandardInfo;
use windows_sys::Win32::Storage::FileSystem::GetFileInformationByHandleEx;
use windows_sys::Win32::Storage::FileSystem::GetVolumeInformationByHandleW;
use windows_sys::Win32::System::Threading::GetCurrentProcess;
use windows_sys::Win32::System::Threading::GetCurrentThread;
use windows_sys::Win32::System::Threading::OpenProcessToken;
use windows_sys::Win32::System::Threading::OpenThreadToken;

use crate::LocalResult;
use crate::error::LocalFileError;
use crate::error::LocalFileErrorKind;
use crate::error::LocalFileOperation;
use crate::error::LocalResourceKind;
use crate::error::LocalResourceLimitError;
use crate::observation::LocalAccessObservation;
use crate::observation::LocalFileIdentity;
use crate::observation::LocalFileSystemLocality;
use crate::observation::LocalFileSystemObservation;
use crate::observation::LocalObservationRequirement;
use crate::observation::LocalSecurityObservation;
use crate::observation::LocalSecurityObservationOptions;
use crate::observation::LocalWindowsAccessObservation;
use crate::observation::LocalWindowsFileSystemObservation;

const MAX_SECURITY_DESCRIPTOR_BYTES: usize = 1024 * 1024;
const MAX_OWNER_DESCRIPTOR_BYTES: usize = 4096;
// SECURITY_DESCRIPTOR_RELATIVE is Revision (u8), Sbz1 (u8), Control (u16),
// and four offsets (u32 each): 1 + 1 + 2 + 4 * 4 = 20 bytes. The header is
// already aligned for the 4-byte alignment required by the following ACL.
const RELATIVE_DESCRIPTOR_HEADER_BYTES: usize = 20;
const SECURITY_QUERY_ATTEMPTS: usize = 8;
const STATUS_BUFFER_TOO_SMALL: i32 = 0xC000_0023u32 as i32;
const STATUS_BUFFER_OVERFLOW: i32 = 0x8000_0005u32 as i32;
const SE_DACL_PRESENT: u16 = 0x0004;
const SE_DACL_PROTECTED: u16 = 0x1000;

pub(crate) fn query_security(
    file: &File,
    options: &LocalSecurityObservationOptions,
) -> LocalResult<LocalSecurityObservation> {
    let handle = file.as_raw_handle();
    let before = query_identity(handle)?;

    let filesystem = if options.filesystem() == LocalObservationRequirement::Required {
        Some(required_query(query_filesystem(handle))?)
    } else {
        optional_query(query_filesystem(handle))?
    };

    let access = if options.access() == LocalObservationRequirement::Required {
        Some(required_query(query_access(handle, options))?)
    } else {
        optional_query(query_access(handle, options))?
    };

    let link_count = if options.link_count() == LocalObservationRequirement::Required {
        Some(u64::from(
            required_query(query_file_info::<FILE_STANDARD_INFO>(handle, FileStandardInfo))?.NumberOfLinks,
        ))
    } else {
        optional_query(query_file_info::<FILE_STANDARD_INFO>(handle, FileStandardInfo))?
            .map(|standard| u64::from(standard.NumberOfLinks))
    };
    let after = query_identity(handle)?;
    if before != after {
        return Err(LocalFileError::new(
            LocalFileErrorKind::ChangedDuringRead,
            LocalFileOperation::Metadata,
        ));
    }
    Ok(LocalSecurityObservation::new(
        Some(before),
        filesystem,
        access.map(|value| LocalAccessObservation::new(None, Some(value))),
        link_count,
    ))
}

fn query_identity(handle: windows_sys::Win32::Foundation::HANDLE) -> LocalResult<LocalFileIdentity> {
    let id = query_file_info::<FILE_ID_INFO>(handle, FileIdInfo)?;
    Ok(LocalFileIdentity::windows(id.VolumeSerialNumber, id.FileId.Identifier))
}

fn query_filesystem(handle: windows_sys::Win32::Foundation::HANDLE) -> LocalResult<LocalFileSystemObservation> {
    let mut name = [0u16; 64];
    let mut attributes = 0u32;
    // SAFETY: `handle` is borrowed from `file`; both output buffers have the
    // exact writable lengths passed to the OS. The filesystem name is bounded.
    let ok = unsafe {
        GetVolumeInformationByHandleW(
            handle,
            ptr::null_mut(),
            0,
            ptr::null_mut(),
            ptr::null_mut(),
            &mut attributes,
            name.as_mut_ptr(),
            name.len() as u32,
        )
    };
    if ok == 0 {
        return Err(native_io_error(io::Error::last_os_error()));
    }
    let end = name.iter().position(|unit| *unit == 0).unwrap_or(name.len());
    let filesystem_name = String::from_utf16(&name[..end]).map_err(|_| data_corruption())?;
    Ok(LocalFileSystemObservation::new(
        LocalFileSystemLocality::Unknown,
        None,
        Some(LocalWindowsFileSystemObservation::new(filesystem_name, attributes)),
    ))
}

fn query_access(
    handle: windows_sys::Win32::Foundation::HANDLE,
    options: &LocalSecurityObservationOptions,
) -> LocalResult<LocalWindowsAccessObservation> {
    let current_sid = current_user_sid()?;
    let owner_descriptor = query_security_descriptor(
        handle,
        OWNER_SECURITY_INFORMATION,
        Some(MAX_OWNER_DESCRIPTOR_BYTES),
        None,
    )?;
    let owner_sid = decode_owner_sid(&owner_descriptor).ok_or_else(data_corruption)?;
    let acl_limit = options.max_acl_bytes();
    let descriptor = query_security_descriptor(handle, DACL_SECURITY_INFORMATION, acl_limit, acl_limit)?;
    check_acl_budget(&descriptor, options)?;
    let decoded =
        decode_dacl_descriptor(&descriptor, None, acl_limit, &owner_sid, &current_sid).ok_or_else(data_corruption)?;
    Ok(decoded)
}

fn query_security_descriptor(
    handle: windows_sys::Win32::Foundation::HANDLE,
    information: u32,
    max_bytes: Option<usize>,
    acl_budget: Option<usize>,
) -> LocalResult<Vec<u8>> {
    query_security_descriptor_with(
        information,
        max_bytes,
        acl_budget,
        |information, output, capacity, needed| {
            // SAFETY: the caller supplies either a null pointer with zero capacity
            // for the probe or a writable allocation of exactly this capacity.
            unsafe {
                windows_sys::Wdk::Storage::FileSystem::NtQuerySecurityObject(
                    handle,
                    information,
                    output,
                    capacity,
                    needed,
                )
            }
        },
    )
}

fn query_security_descriptor_with(
    information: u32,
    max_bytes: Option<usize>,
    acl_budget: Option<usize>,
    mut query: impl FnMut(u32, *mut core::ffi::c_void, u32, *mut u32) -> i32,
) -> LocalResult<Vec<u8>> {
    let mut needed = 0u32;
    let status = query(information, ptr::null_mut(), 0, &mut needed);
    if status != STATUS_BUFFER_TOO_SMALL && status != STATUS_BUFFER_OVERFLOW {
        return Err(ntstatus_error(status));
    }
    for _ in 0..SECURITY_QUERY_ATTEMPTS {
        let length = usize::try_from(needed).map_err(|_| data_corruption())?;
        if length > MAX_SECURITY_DESCRIPTOR_BYTES {
            return Err(resource_limit(
                LocalResourceKind::AclBytes,
                MAX_SECURITY_DESCRIPTOR_BYTES,
                length,
            ));
        }
        let allowed = match (max_bytes, acl_budget) {
            (Some(limit), Some(_)) => RELATIVE_DESCRIPTOR_HEADER_BYTES
                .checked_add(limit)
                .ok_or_else(|| resource_limit(LocalResourceKind::AclBytes, limit, usize::MAX))?
                .min(MAX_SECURITY_DESCRIPTOR_BYTES),
            (Some(limit), None) => limit.min(MAX_SECURITY_DESCRIPTOR_BYTES),
            (None, _) => MAX_SECURITY_DESCRIPTOR_BYTES,
        };
        if length > allowed {
            if let Some(limit) = acl_budget {
                let requested = length.saturating_sub(RELATIVE_DESCRIPTOR_HEADER_BYTES);
                return Err(resource_limit(LocalResourceKind::AclBytes, limit, requested));
            }
            return Err(resource_limit(LocalResourceKind::AclBytes, allowed, length));
        }
        if length == 0 {
            return Err(data_corruption());
        }
        let mut buffer = Vec::new();
        buffer
            .try_reserve_exact(length)
            .map_err(|_| resource_limit(LocalResourceKind::AclBytes, length, length))?;
        buffer.resize(length, 0);
        let mut actual = 0u32;
        // SAFETY: `buffer` is writable for `length` bytes, and the API receives
        // that exact bounded size. `needed` remains an output scalar.
        let status = query(
            information,
            buffer.as_mut_ptr().cast(),
            u32::try_from(length).map_err(|_| data_corruption())?,
            &mut actual,
        );
        if status >= 0 {
            let actual = usize::try_from(actual).map_err(|_| data_corruption())?;
            if actual > buffer.len() {
                return Err(data_corruption());
            }
            buffer.truncate(actual);
            return Ok(buffer);
        }
        if status != STATUS_BUFFER_TOO_SMALL && status != STATUS_BUFFER_OVERFLOW {
            return Err(ntstatus_error(status));
        }
        needed = actual.max(needed);
    }
    Err(
        LocalFileError::new(LocalFileErrorKind::ChangedDuringRead, LocalFileOperation::Metadata)
            .with_reason("security descriptor length changed during bounded query retries"),
    )
}

fn current_user_sid() -> LocalResult<String> {
    let mut token = ptr::null_mut();
    // Prefer the impersonation token when this thread has one, so the observed
    // principal matches the effective caller. Fall back only for ERROR_NO_TOKEN.
    // SAFETY: the pseudo thread handle is valid for this thread; `token` points
    // to writable storage for a returned token handle.
    if unsafe { OpenThreadToken(GetCurrentThread(), TOKEN_QUERY, 1, &mut token) } == 0 {
        let error = io::Error::last_os_error();
        if error.raw_os_error() != Some(windows_sys::Win32::Foundation::ERROR_NO_TOKEN as i32) {
            return Err(native_io_error(error));
        }
        // SAFETY: the pseudo process handle is valid for this process and
        // `token` points to writable storage for the resulting token handle.
        if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) } == 0 {
            return Err(native_io_error(io::Error::last_os_error()));
        }
    }
    // SAFETY: successful OpenProcessToken returned an owned HANDLE.
    let token = unsafe { OwnedHandle::from_raw_handle(token.cast()) };
    let mut needed = 0u32;
    // SAFETY: this zero-length call only probes the required TOKEN_USER size.
    unsafe { GetTokenInformation(token.as_raw_handle().cast(), TokenUser, ptr::null_mut(), 0, &mut needed) };
    if needed == 0 || needed as usize > 4096 {
        return Err(data_corruption());
    }
    let mut buffer = vec![0u8; needed as usize];
    // SAFETY: buffer has the exact reported bounded size and is writable.
    if unsafe {
        GetTokenInformation(
            token.as_raw_handle().cast(),
            TokenUser,
            buffer.as_mut_ptr().cast(),
            needed,
            &mut needed,
        )
    } == 0
    {
        return Err(native_io_error(io::Error::last_os_error()));
    }
    if needed as usize > buffer.len() || buffer.len() < size_of::<TOKEN_USER>() {
        return Err(data_corruption());
    }
    // SAFETY: the successful native call filled a buffer at least as large as
    // TOKEN_USER; read_unaligned avoids imposing alignment on Vec<u8>.
    let user = unsafe { ptr::read_unaligned(buffer.as_ptr().cast::<TOKEN_USER>()) };
    let base = buffer.as_ptr() as usize;
    let sid_ptr = user.User.Sid as usize;
    let offset = sid_ptr.checked_sub(base).ok_or_else(data_corruption)?;
    if offset >= buffer.len() {
        return Err(data_corruption());
    }
    sid_string(&buffer[offset..]).ok_or_else(data_corruption)
}

fn query_file_info<T: Default>(
    handle: windows_sys::Win32::Foundation::HANDLE,
    class: windows_sys::Win32::Storage::FileSystem::FILE_INFO_BY_HANDLE_CLASS,
) -> LocalResult<T> {
    let mut output = T::default();
    // SAFETY: output is a correctly sized writable value for the selected
    // information class and the handle is borrowed from the retained File.
    if unsafe { GetFileInformationByHandleEx(handle, class, ptr::from_mut(&mut output).cast(), size_of::<T>() as u32) }
        == 0
    {
        return Err(native_io_error(io::Error::last_os_error()));
    }
    Ok(output)
}

#[cfg(test)]
fn decode_security_descriptor(
    bytes: &[u8],
    max_entries: Option<usize>,
    max_acl_bytes: Option<usize>,
    current_sid: &str,
) -> Option<LocalWindowsAccessObservation> {
    if bytes.len() < 20 || bytes[0] != 1 {
        return None;
    }
    let control = u16::from_le_bytes([bytes[2], bytes[3]]);
    const SE_SELF_RELATIVE: u16 = 0x8000;
    if control & SE_SELF_RELATIVE == 0 {
        return None;
    }
    let owner_offset = usize::try_from(u32::from_le_bytes(bytes[4..8].try_into().ok()?)).ok()?;
    let dacl_offset = usize::try_from(u32::from_le_bytes(bytes[16..20].try_into().ok()?)).ok()?;
    if owner_offset < 20 || owner_offset % 4 != 0 {
        return None;
    }
    let owner_end = sid_end(bytes, owner_offset)?;
    let owner_sid = sid_string(&bytes[owner_offset..owner_end])?;
    let dacl_present = control & SE_DACL_PRESENT != 0;
    let dacl_is_null = dacl_present && dacl_offset == 0;
    let aces = if !dacl_present {
        if dacl_offset != 0 {
            return None;
        }
        Vec::new()
    } else if dacl_is_null {
        Vec::new()
    } else {
        if dacl_offset < 20 || dacl_offset % 4 != 0 {
            return None;
        }
        let header_end = dacl_offset.checked_add(8)?;
        if header_end > bytes.len() {
            return None;
        }
        let acl_size = usize::from(u16::from_le_bytes([bytes[dacl_offset + 2], bytes[dacl_offset + 3]]));
        let ace_count = usize::from(u16::from_le_bytes([bytes[dacl_offset + 4], bytes[dacl_offset + 5]]));
        if max_acl_bytes.is_some_and(|limit| acl_size > limit) {
            return None;
        }
        if max_entries.is_some_and(|limit| ace_count > limit) {
            return None;
        }
        let acl_end = dacl_offset.checked_add(acl_size)?;
        if acl_size < 8
            || !matches!(bytes[dacl_offset], 2 | 4)
            || acl_end > bytes.len()
            || ranges_overlap(owner_offset..owner_end, dacl_offset..acl_end)
        {
            return None;
        }
        decode_acl(
            &bytes[dacl_offset..acl_end],
            max_entries,
            max_acl_bytes,
            Some(current_sid),
        )?
    };
    Some(LocalWindowsAccessObservation::new(
        owner_sid.clone(),
        owner_sid == current_sid,
        dacl_present,
        dacl_is_null,
        control & SE_DACL_PROTECTED != 0,
        true,
        aces,
    ))
}

fn decode_owner_sid(bytes: &[u8]) -> Option<String> {
    if bytes.len() < RELATIVE_DESCRIPTOR_HEADER_BYTES || bytes[0] != 1 {
        return None;
    }
    let control = u16::from_le_bytes([bytes[2], bytes[3]]);
    if control & 0x8000 == 0 {
        return None;
    }
    let offset = usize::try_from(u32::from_le_bytes(bytes[4..8].try_into().ok()?)).ok()?;
    if offset < RELATIVE_DESCRIPTOR_HEADER_BYTES || offset % 4 != 0 {
        return None;
    }
    let end = sid_end(bytes, offset)?;
    sid_string(&bytes[offset..end])
}

fn decode_dacl_descriptor(
    bytes: &[u8],
    max_entries: Option<usize>,
    max_acl_bytes: Option<usize>,
    owner_sid: &str,
    current_sid: &str,
) -> Option<LocalWindowsAccessObservation> {
    if bytes.len() < RELATIVE_DESCRIPTOR_HEADER_BYTES || bytes[0] != 1 {
        return None;
    }
    let control = u16::from_le_bytes([bytes[2], bytes[3]]);
    if control & 0x8000 == 0 {
        return None;
    }
    let dacl_offset = usize::try_from(u32::from_le_bytes(bytes[16..20].try_into().ok()?)).ok()?;
    let dacl_present = control & SE_DACL_PRESENT != 0;
    let dacl_is_null = dacl_present && dacl_offset == 0;
    let aces = if !dacl_present {
        if dacl_offset != 0 {
            return None;
        }
        Vec::new()
    } else if dacl_is_null {
        Vec::new()
    } else {
        if dacl_offset < RELATIVE_DESCRIPTOR_HEADER_BYTES || dacl_offset % 4 != 0 {
            return None;
        }
        let header_end = dacl_offset.checked_add(8)?;
        if header_end > bytes.len() {
            return None;
        }
        let acl_size = usize::from(u16::from_le_bytes([bytes[dacl_offset + 2], bytes[dacl_offset + 3]]));
        let ace_count = usize::from(u16::from_le_bytes([bytes[dacl_offset + 4], bytes[dacl_offset + 5]]));
        if max_acl_bytes.is_some_and(|limit| acl_size > limit) || max_entries.is_some_and(|limit| ace_count > limit) {
            return None;
        }
        let acl_end = dacl_offset.checked_add(acl_size)?;
        if acl_size < 8 || !matches!(bytes[dacl_offset], 2 | 4) || acl_end > bytes.len() {
            return None;
        }
        decode_acl(
            &bytes[dacl_offset..acl_end],
            max_entries,
            max_acl_bytes,
            Some(current_sid),
        )?
    };
    Some(LocalWindowsAccessObservation::new(
        owner_sid.to_owned(),
        owner_sid == current_sid,
        dacl_present,
        dacl_is_null,
        control & SE_DACL_PROTECTED != 0,
        true,
        aces,
    ))
}

fn sid_end(bytes: &[u8], offset: usize) -> Option<usize> {
    let header_end = offset.checked_add(8)?;
    if header_end > bytes.len() || bytes[offset] != 1 || bytes[offset + 1] > 15 {
        return None;
    }
    let sid_size = 8usize.checked_add(usize::from(bytes[offset + 1]).checked_mul(4)?)?;
    let end = offset.checked_add(sid_size)?;
    (end <= bytes.len()).then_some(end)
}

#[cfg(test)]
fn ranges_overlap(left: std::ops::Range<usize>, right: std::ops::Range<usize>) -> bool {
    left.start < right.end && right.start < left.end
}

fn check_acl_budget(bytes: &[u8], options: &LocalSecurityObservationOptions) -> LocalResult<()> {
    if bytes.len() < 20 {
        return Err(data_corruption());
    }
    let control = u16::from_le_bytes([bytes[2], bytes[3]]);
    if control & SE_DACL_PRESENT == 0 {
        return Ok(());
    }
    let offset = usize::try_from(u32::from_le_bytes(
        bytes[16..20].try_into().map_err(|_| data_corruption())?,
    ))
    .map_err(|_| data_corruption())?;
    if offset == 0 {
        return Ok(());
    }
    if offset < 20 || offset % 4 != 0 {
        return Err(data_corruption());
    }
    let header_end = offset.checked_add(8).ok_or_else(data_corruption)?;
    if header_end > bytes.len() {
        return Err(data_corruption());
    }
    let acl_bytes = usize::from(u16::from_le_bytes([bytes[offset + 2], bytes[offset + 3]]));
    let entries = usize::from(u16::from_le_bytes([bytes[offset + 4], bytes[offset + 5]]));
    if acl_bytes < 8 || offset.checked_add(acl_bytes).is_none_or(|end| end > bytes.len()) {
        return Err(data_corruption());
    }
    if let Some(limit) = options.max_acl_bytes()
        && acl_bytes > limit
    {
        return Err(resource_limit(LocalResourceKind::AclBytes, limit, acl_bytes));
    }
    if let Some(limit) = options.max_acl_entries()
        && entries > limit
    {
        return Err(resource_limit(LocalResourceKind::AclEntries, limit, entries));
    }
    Ok(())
}

fn explicitly_unsupported(error: &LocalFileError) -> bool {
    error.kind() == LocalFileErrorKind::Unsupported
}

fn optional_query<T>(result: LocalResult<T>) -> LocalResult<Option<T>> {
    match result {
        Ok(value) => Ok(Some(value)),
        Err(error) if explicitly_unsupported(&error) => Ok(None),
        Err(error) => Err(error),
    }
}

fn required_query<T>(result: LocalResult<T>) -> LocalResult<T> {
    result.map_err(requirement_for_unsupported)
}

fn requirement_for_unsupported(error: LocalFileError) -> LocalFileError {
    if explicitly_unsupported(&error) {
        LocalFileError::new(LocalFileErrorKind::RequirementNotMet, LocalFileOperation::Metadata)
    } else {
        error
    }
}

fn data_corruption() -> LocalFileError {
    LocalFileError::new(LocalFileErrorKind::DataCorruption, LocalFileOperation::Metadata)
}

fn resource_limit(resource: LocalResourceKind, limit: usize, requested: usize) -> LocalFileError {
    LocalFileError::from_resource_limit(
        LocalFileOperation::Metadata,
        None,
        LocalResourceLimitError::new(resource, limit, 0, requested),
    )
}

fn native_io_error(error: io::Error) -> LocalFileError {
    let unsupported_code = matches!(
        error.raw_os_error(),
        Some(code)
            if code == windows_sys::Win32::Foundation::ERROR_INVALID_FUNCTION as i32
                || code == windows_sys::Win32::Foundation::ERROR_NOT_SUPPORTED as i32
                || code == windows_sys::Win32::Foundation::ERROR_CALL_NOT_IMPLEMENTED as i32
    );
    let converted = LocalFileError::from_io(LocalFileOperation::Metadata, None, None, error);
    if unsupported_code {
        converted.with_kind(LocalFileErrorKind::Unsupported)
    } else {
        converted
    }
}

fn ntstatus_error(status: i32) -> LocalFileError {
    const STATUS_ACCESS_DENIED: i32 = 0xC000_0022u32 as i32;
    const STATUS_NOT_SUPPORTED: i32 = 0xC000_00BBu32 as i32;
    let kind = if status == STATUS_ACCESS_DENIED {
        LocalFileErrorKind::PermissionDenied
    } else if status == STATUS_NOT_SUPPORTED {
        LocalFileErrorKind::Unsupported
    } else {
        LocalFileErrorKind::Io
    };
    LocalFileError::from_io(
        LocalFileOperation::Metadata,
        None,
        None,
        io::Error::new(io::ErrorKind::Other, NtStatusError(status)),
    )
    .with_kind(kind)
}

#[derive(Debug)]
struct NtStatusError(i32);

impl fmt::Display for NtStatusError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "NtQuerySecurityObject returned NTSTATUS {:#010x}",
            self.0 as u32
        )
    }
}

impl std::error::Error for NtStatusError {}

#[cfg(test)]
mod tests {
    use std::io;

    use super::decode_acl;
    use super::decode_security_descriptor;
    use super::native_io_error;
    use super::optional_query;
    use super::required_query;
    use super::sid_string;
    use crate::error::LocalFileErrorKind;
    use crate::error::LocalResourceKind;
    use crate::observation::LocalSecurityObservationOptions;

    #[path = "acl_budget_tests.rs"]
    mod acl_budget_tests;

    #[test]
    fn security_windows_sid_decode_validates_length_and_formats_authority() {
        let sid = [1, 2, 0, 0, 0, 0, 0, 5, 32, 0, 0, 0, 33, 2, 0, 0];
        assert_eq!(sid_string(&sid), Some("S-1-5-32-545".to_owned()));
        assert_eq!(sid_string(&sid[..15]), None);
    }

    #[test]
    fn security_windows_acl_decode_preserves_unknown_ace_and_rejects_bad_bounds() {
        let acl = [2, 0, 16, 0, 1, 0, 0, 0, 0x7f, 0, 8, 0, 0, 0, 0, 0];
        let entries = decode_acl(&acl, None, None, None).expect("valid ACL");
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].ace_type(), 0x7f);
        assert!(!entries[0].is_decoded());
        assert!(decode_acl(&acl[..15], None, None, None).is_none());
        assert!(decode_acl(&acl, None, Some(15), None).is_none());
    }

    #[test]
    fn security_windows_acl_decodes_basic_ace_mask_and_current_trustee() {
        let acl = [
            2, 0, 32, 0, 1, 0, 0, 0, // ACL header
            0, 0, 24, 0, // ACCESS_ALLOWED_ACE header
            0x78, 0x56, 0x34, 0x12, // access mask
            1, 2, 0, 0, 0, 0, 0, 5, 32, 0, 0, 0, 33, 2, 0, 0, // SID
        ];
        let entries = decode_acl(&acl, None, None, Some("S-1-5-32-545")).expect("valid basic ACE");
        assert_eq!(entries.len(), 1);
        assert!(entries[0].is_decoded());
        assert_eq!(entries[0].access_mask(), Some(0x1234_5678));
        assert_eq!(entries[0].trustee_sid(), Some("S-1-5-32-545"));
        assert_eq!(entries[0].trustee_is_current_user(), Some(true));
    }

    #[test]
    fn security_windows_descriptor_distinguishes_absent_null_and_empty_dacl() {
        let owner = [1, 2, 0, 0, 0, 0, 0, 5, 32, 0, 0, 0, 33, 2, 0, 0];
        let mut descriptor = vec![0; 44];
        descriptor[0] = 1;
        descriptor[2..4].copy_from_slice(&0x8004u16.to_le_bytes());
        descriptor[4..8].copy_from_slice(&20u32.to_le_bytes());
        descriptor[16..20].copy_from_slice(&36u32.to_le_bytes());
        descriptor[20..36].copy_from_slice(&owner);
        descriptor[36] = 2;
        descriptor[38..40].copy_from_slice(&8u16.to_le_bytes());
        descriptor[40..42].copy_from_slice(&0u16.to_le_bytes());

        let empty =
            decode_security_descriptor(&descriptor, None, None, "S-1-5-32-545").expect("empty DACL should decode");
        assert!(empty.dacl_present());
        assert!(!empty.dacl_is_null());
        assert!(empty.aces().is_empty());

        descriptor[2..4].copy_from_slice(&0x8004u16.to_le_bytes());
        descriptor[16..20].copy_from_slice(&0u32.to_le_bytes());
        let null =
            decode_security_descriptor(&descriptor[..36], None, None, "S-1-5-32-545").expect("null DACL should decode");
        assert!(null.dacl_present());
        assert!(null.dacl_is_null());

        descriptor[2..4].copy_from_slice(&0x8000u16.to_le_bytes());
        let absent = decode_security_descriptor(&descriptor[..36], None, None, "S-1-5-32-545")
            .expect("absent DACL should decode");
        assert!(!absent.dacl_present());
        assert!(!absent.dacl_is_null());

        let mut misaligned = descriptor.clone();
        misaligned[2..4].copy_from_slice(&0x8004u16.to_le_bytes());
        misaligned[16..20].copy_from_slice(&37u32.to_le_bytes());
        assert!(decode_security_descriptor(&misaligned, None, None, "S-1-5-32-545").is_none());

        let mut truncated = descriptor.clone();
        truncated[2..4].copy_from_slice(&0x8004u16.to_le_bytes());
        truncated[16..20].copy_from_slice(&36u32.to_le_bytes());
        assert!(decode_security_descriptor(&truncated[..43], None, None, "S-1-5-32-545").is_none());
        assert!(decode_security_descriptor(&truncated[..30], None, None, "S-1-5-32-545").is_none());

        let mut overlapping = vec![0; 56];
        overlapping[..20].copy_from_slice(&descriptor[..20]);
        overlapping[2..4].copy_from_slice(&0x8004u16.to_le_bytes());
        overlapping[16..20].copy_from_slice(&24u32.to_le_bytes());
        overlapping[20..36].copy_from_slice(&owner);
        overlapping[24] = 2;
        overlapping[26..28].copy_from_slice(&32u16.to_le_bytes());
        overlapping[28..30].copy_from_slice(&0u16.to_le_bytes());
        assert!(decode_security_descriptor(&overlapping, None, None, "S-1-5-32-545").is_none());
    }

    #[test]
    fn security_windows_acl_limits_report_the_dimension_before_decode() {
        let descriptor = descriptor_with_one_unknown_ace();
        let bytes_limited = super::check_acl_budget(
            &descriptor,
            &LocalSecurityObservationOptions::new().with_max_acl_bytes(15),
        )
        .expect_err("ACL byte budget should reject the descriptor");
        assert_eq!(bytes_limited.kind(), LocalFileErrorKind::ResourceLimit);
        assert_eq!(
            bytes_limited.resource_limit_error().map(|limit| limit.resource()),
            Some(LocalResourceKind::AclBytes),
        );

        let entries_limited = super::check_acl_budget(
            &descriptor,
            &LocalSecurityObservationOptions::new().with_max_acl_entries(0),
        )
        .expect_err("ACL entry budget should reject the descriptor");
        assert_eq!(entries_limited.kind(), LocalFileErrorKind::ResourceLimit);
        assert_eq!(
            entries_limited.resource_limit_error().map(|limit| limit.resource()),
            Some(LocalResourceKind::AclEntries),
        );
    }

    #[test]
    fn security_windows_optional_suppresses_only_unsupported_and_required_rejects_it() {
        for code in [
            windows_sys::Win32::Foundation::ERROR_INVALID_FUNCTION,
            windows_sys::Win32::Foundation::ERROR_NOT_SUPPORTED,
            windows_sys::Win32::Foundation::ERROR_CALL_NOT_IMPLEMENTED,
        ] {
            let unsupported = native_io_error(io::Error::from_raw_os_error(code as i32));
            assert_eq!(unsupported.kind(), LocalFileErrorKind::Unsupported);
            assert!(
                optional_query::<()>(Err(unsupported))
                    .expect("optional unsupported")
                    .is_none()
            );

            let unsupported = native_io_error(io::Error::from_raw_os_error(code as i32));
            assert_eq!(
                required_query::<()>(Err(unsupported))
                    .expect_err("required unsupported")
                    .kind(),
                LocalFileErrorKind::RequirementNotMet,
            );
        }
        let denied = native_io_error(io::Error::from(io::ErrorKind::PermissionDenied));
        assert_eq!(denied.kind(), LocalFileErrorKind::PermissionDenied);
        assert_eq!(
            optional_query::<()>(Err(denied))
                .expect_err("optional permission error must propagate")
                .kind(),
            LocalFileErrorKind::PermissionDenied,
        );
    }

    fn descriptor_with_one_unknown_ace() -> Vec<u8> {
        let mut descriptor = vec![0; 52];
        descriptor[0] = 1;
        descriptor[2..4].copy_from_slice(&0x8004u16.to_le_bytes());
        descriptor[4..8].copy_from_slice(&20u32.to_le_bytes());
        descriptor[16..20].copy_from_slice(&36u32.to_le_bytes());
        descriptor[20..36].copy_from_slice(&[1, 2, 0, 0, 0, 0, 0, 5, 32, 0, 0, 0, 33, 2, 0, 0]);
        descriptor[36] = 2;
        descriptor[38..40].copy_from_slice(&16u16.to_le_bytes());
        descriptor[40..42].copy_from_slice(&1u16.to_le_bytes());
        descriptor[44] = 0x7f;
        descriptor[46..48].copy_from_slice(&8u16.to_le_bytes());
        descriptor
    }
}

fn sid_string(bytes: &[u8]) -> Option<String> {
    if bytes.len() < 8 || bytes[0] != 1 {
        return None;
    }
    let subauthority_count = usize::from(bytes[1]);
    let sid_size = 8usize.checked_add(subauthority_count.checked_mul(4)?)?;
    if subauthority_count > 15 || sid_size > bytes.len() {
        return None;
    }
    let authority = bytes[2..8]
        .iter()
        .fold(0u64, |value, byte| (value << 8) | u64::from(*byte));
    let mut result = format!("S-{}-{}", bytes[0], authority);
    for chunk in bytes[8..sid_size].chunks_exact(4) {
        result.push('-');
        result.push_str(&u32::from_le_bytes(chunk.try_into().ok()?).to_string());
    }
    Some(result)
}

fn decode_acl(
    bytes: &[u8],
    max_entries: Option<usize>,
    max_bytes: Option<usize>,
    current_sid: Option<&str>,
) -> Option<Vec<crate::observation::LocalWindowsAceObservation>> {
    if bytes.len() < 8 {
        return None;
    }
    let acl_size = usize::from(u16::from_le_bytes([bytes[2], bytes[3]]));
    let count = usize::from(u16::from_le_bytes([bytes[4], bytes[5]]));
    if acl_size < 8
        || !matches!(bytes[0], 2 | 4)
        || acl_size > bytes.len()
        || max_entries.is_some_and(|limit| count > limit)
        || max_bytes.is_some_and(|limit| acl_size > limit)
    {
        return None;
    }
    let mut offset = 8usize;
    let mut output = Vec::with_capacity(count);
    for _ in 0..count {
        let header_end = offset.checked_add(4)?;
        if header_end > acl_size {
            return None;
        }
        let ace_type = bytes[offset];
        let flags = bytes[offset + 1];
        let ace_size = usize::from(u16::from_le_bytes([bytes[offset + 2], bytes[offset + 3]]));
        let ace_end = offset.checked_add(ace_size)?;
        if ace_size < 4 || ace_size % 4 != 0 || ace_end > acl_size {
            return None;
        }
        // Retain unfamiliar ACEs exactly as ordered native facts. Known basic
        // allow/deny/audit/alarm ACEs have a mask followed by a SID.
        let decoded = matches!(ace_type, 0..=3);
        let (mask, trustee) = if decoded {
            if ace_size < 16 {
                return None;
            }
            let mask = u32::from_le_bytes(bytes[offset + 4..offset + 8].try_into().ok()?);
            let sid_bytes = &bytes[offset + 8..ace_end];
            if sid_end(sid_bytes, 0)? != sid_bytes.len() {
                return None;
            }
            let sid = sid_string(sid_bytes)?;
            (Some(mask), Some(sid))
        } else {
            (None, None)
        };
        let trustee_is_current_user = trustee.as_deref().zip(current_sid).map(|(sid, current)| sid == current);
        output.push(crate::observation::LocalWindowsAceObservation::new(
            ace_type,
            flags,
            u16::try_from(ace_size).ok()?,
            mask,
            trustee,
            trustee_is_current_user,
            decoded,
        ));
        offset = ace_end;
    }
    if offset > acl_size { None } else { Some(output) }
}
