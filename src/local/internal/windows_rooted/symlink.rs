// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Handle-relative Windows symbolic-link operations.

use std::ffi::OsString;
use std::fs::File;
use std::io::Error;
use std::io::ErrorKind;
use std::io::Result;
use std::mem::size_of;
use std::os::windows::ffi::OsStrExt;
use std::os::windows::ffi::OsStringExt;
use std::os::windows::io::AsRawHandle;
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
use windows_sys::Win32::Storage::FileSystem::DELETE;
use windows_sys::Win32::Storage::FileSystem::FILE_ATTRIBUTE_DIRECTORY;
use windows_sys::Win32::Storage::FileSystem::FILE_ID_INFO;
use windows_sys::Win32::Storage::FileSystem::FILE_READ_ATTRIBUTES;
use windows_sys::Win32::Storage::FileSystem::FileIdInfo;
use windows_sys::Win32::Storage::FileSystem::GetFileInformationByHandleEx;
use windows_sys::Win32::Storage::FileSystem::MAXIMUM_REPARSE_DATA_BUFFER_SIZE;
use windows_sys::Win32::Storage::FileSystem::SYNCHRONIZE;
use windows_sys::Win32::System::IO::DeviceIoControl;
use windows_sys::Win32::System::Ioctl::FSCTL_GET_REPARSE_POINT;
use windows_sys::Win32::System::Ioctl::FSCTL_SET_REPARSE_POINT;

use super::handle::handle_attributes;
use super::handle::nt_open_at;
use super::handle::open_parent;
use super::namespace_mutation::delete_open_entry;
use crate::local::LocalRelativePath;
use crate::local::internal::RootedSymlinkCreateError;
use crate::local::internal::RootedSymlinkCreateFailureState;

/// Windows tag identifying a symbolic-link reparse buffer.
const IO_REPARSE_TAG_SYMLINK: u32 = 0xA000_000C;
/// Flag identifying a relative symbolic-link substitute name.
const SYMLINK_FLAG_RELATIVE: u32 = 1;
/// Bytes before the variable path buffer in a symbolic-link reparse record.
const SYMLINK_PATH_BUFFER_OFFSET: usize = 20;
/// Bytes in the symbolic-link-specific section before its path buffer.
const SYMLINK_REPARSE_FIXED_DATA_LENGTH: usize = 12;

/// Reads one final symbolic-link target through a no-follow handle.
///
/// # Errors
///
/// Returns an I/O error when parent traversal, final-handle opening, native
/// reparse retrieval, or buffer validation fails.
pub(crate) fn read_rooted_link(root: &File, _diagnostic_root: &Path, path: &LocalRelativePath) -> Result<PathBuf> {
    let link = open_link(root, path, GENERIC_READ | FILE_READ_ATTRIBUTES | SYNCHRONIZE)?;
    let buffer = read_reparse_buffer(&link)?;
    parse_symbolic_link_target(&buffer)
}

/// Creates one final symbolic link relative to its already-opened parent.
///
/// The destination placeholder and reparse record are both addressed through
/// handles, so renaming or replacing the diagnostic Host path cannot redirect
/// the operation.
///
/// # Errors
///
/// Returns an I/O error when secure traversal, placeholder creation, reparse
/// installation, or rollback fails.
pub(crate) fn create_rooted_symlink(
    root: &File,
    _diagnostic_root: &Path,
    target: &Path,
    path: &LocalRelativePath,
    targets_directory: bool,
) -> std::result::Result<(), RootedSymlinkCreateError> {
    let unchanged = |primary| RootedSymlinkCreateError::new(RootedSymlinkCreateFailureState::Unchanged, primary, None);
    let buffer = build_symbolic_link_buffer(target).map_err(unchanged)?;
    let (parent, name) = open_parent(root, path).map_err(unchanged)?;
    let options = if targets_directory {
        FILE_DIRECTORY_FILE
    } else {
        FILE_NON_DIRECTORY_FILE
    };
    let link = nt_open_at(
        &parent,
        &name,
        GENERIC_WRITE | DELETE | FILE_READ_ATTRIBUTES | SYNCHRONIZE,
        FILE_CREATE,
        options,
    )
    .map_err(unchanged)?;
    if let Err(source_error) = set_reparse_buffer(&link, &buffer) {
        return match delete_open_entry(&link) {
            Ok(()) => Err(unchanged(source_error)),
            Err(cleanup_error) => {
                let state = rollback_failure_state(&parent, &name, &link);
                Err(RootedSymlinkCreateError::new(state, source_error, Some(cleanup_error)))
            }
        };
    }
    Ok(())
}

/// Determines whether a failed rollback left the original placeholder named.
///
/// Reopening by the retained parent handle and comparing stable file identity
/// avoids treating the mere presence of a native error code as proof that the
/// destination still names the placeholder.
fn rollback_failure_state(
    parent: &File,
    name: &std::ffi::OsStr,
    placeholder: &File,
) -> RootedSymlinkCreateFailureState {
    let reopened = match nt_open_at(parent, name, FILE_READ_ATTRIBUTES | SYNCHRONIZE, FILE_OPEN, 0) {
        Ok(reopened) => reopened,
        Err(error) if error.kind() == ErrorKind::NotFound => {
            return RootedSymlinkCreateFailureState::Unchanged;
        }
        Err(_) => return RootedSymlinkCreateFailureState::Indeterminate,
    };
    match (handle_identity(placeholder), handle_identity(&reopened)) {
        (Ok(expected), Ok(observed)) if expected == observed => RootedSymlinkCreateFailureState::PartiallyPublished,
        _ => RootedSymlinkCreateFailureState::Indeterminate,
    }
}

/// Reads the stable volume and file identifier of an opened entry.
fn handle_identity(file: &File) -> Result<(u64, [u8; 16])> {
    let mut identity = FILE_ID_INFO::default();
    // SAFETY: `file` owns a live handle and `identity` is a correctly sized
    // writable buffer for FileIdInfo.
    let result = unsafe {
        GetFileInformationByHandleEx(
            file.as_raw_handle(),
            FileIdInfo,
            (&raw mut identity).cast(),
            size_of::<FILE_ID_INFO>() as u32,
        )
    };
    if result == 0 {
        return Err(Error::last_os_error());
    }
    Ok((identity.VolumeSerialNumber, identity.FileId.Identifier))
}

/// Reports whether a symbolic-link entry carries the directory attribute.
///
/// This opens only the final reparse point and never queries its target.
///
/// # Errors
///
/// Returns an I/O error when the final entry cannot be opened, is not a
/// symbolic link, or its attributes cannot be inspected.
pub(crate) fn rooted_link_targets_directory(root: &File, path: &LocalRelativePath) -> Result<bool> {
    let link = open_link(root, path, FILE_READ_ATTRIBUTES | SYNCHRONIZE)?;
    let attributes = handle_attributes(&link)?;
    if attributes.ReparseTag != IO_REPARSE_TAG_SYMLINK {
        return Err(Error::new(
            ErrorKind::Unsupported,
            "rooted entry is not a symbolic link",
        ));
    }
    Ok(attributes.FileAttributes & FILE_ATTRIBUTE_DIRECTORY != 0)
}

/// Opens the final link without following it.
fn open_link(root: &File, path: &LocalRelativePath, access: u32) -> Result<File> {
    let (parent, name) = open_parent(root, path)?;
    nt_open_at(&parent, &name, access, FILE_OPEN, 0)
}

/// Retrieves the opaque reparse record for an already-opened link handle.
fn read_reparse_buffer(link: &File) -> Result<Vec<u8>> {
    let mut storage = vec![0_usize; (MAXIMUM_REPARSE_DATA_BUFFER_SIZE as usize).div_ceil(size_of::<usize>())];
    let mut returned = 0_u32;
    // SAFETY: `link` remains open, the output allocation is writable for the
    // advertised capacity, and this synchronous call retains no pointers.
    let result = unsafe {
        DeviceIoControl(
            link.as_raw_handle(),
            FSCTL_GET_REPARSE_POINT,
            null(),
            0,
            storage.as_mut_ptr().cast(),
            MAXIMUM_REPARSE_DATA_BUFFER_SIZE,
            &raw mut returned,
            null_mut(),
        )
    };
    if result == 0 {
        return Err(Error::last_os_error());
    }
    let returned = returned as usize;
    if returned > MAXIMUM_REPARSE_DATA_BUFFER_SIZE as usize {
        return Err(corrupt_reparse("reparse query returned an oversized buffer"));
    }
    // SAFETY: `storage` contains `returned` initialized bytes written by the
    // successful synchronous query.
    Ok(unsafe { std::slice::from_raw_parts(storage.as_ptr().cast::<u8>(), returned) }.to_vec())
}

/// Installs a complete symbolic-link reparse record on a placeholder handle.
fn set_reparse_buffer(link: &File, buffer: &[u8]) -> Result<()> {
    let mut returned = 0_u32;
    let buffer_length = u32::try_from(buffer.len())
        .map_err(|_| Error::new(ErrorKind::InvalidInput, "symbolic-link reparse buffer is too large"))?;
    // SAFETY: `link` remains open, `buffer` contains a complete immutable
    // reparse record, and this synchronous call retains no pointers.
    let result = unsafe {
        DeviceIoControl(
            link.as_raw_handle(),
            FSCTL_SET_REPARSE_POINT,
            buffer.as_ptr().cast(),
            buffer_length,
            null_mut(),
            0,
            &raw mut returned,
            null_mut(),
        )
    };
    if result == 0 {
        Err(Error::last_os_error())
    } else {
        Ok(())
    }
}

/// Builds the native reparse record that preserves one user-visible target.
fn build_symbolic_link_buffer(target: &Path) -> Result<Vec<u8>> {
    let print_name: Vec<u16> = target.as_os_str().encode_wide().collect();
    if print_name.contains(&0) {
        return Err(Error::new(
            ErrorKind::InvalidInput,
            "symbolic-link target contains an interior NUL",
        ));
    }
    let (substitute_name, flags) = substitute_name(target, &print_name);
    let substitute_bytes = checked_utf16_byte_length(substitute_name.len())?;
    let print_bytes = checked_utf16_byte_length(print_name.len())?;
    let path_bytes = usize::from(substitute_bytes)
        .checked_add(usize::from(print_bytes))
        .ok_or_else(|| Error::new(ErrorKind::InvalidInput, "symbolic-link target is too long"))?;
    let data_length = SYMLINK_REPARSE_FIXED_DATA_LENGTH
        .checked_add(path_bytes)
        .and_then(|length| u16::try_from(length).ok())
        .ok_or_else(|| Error::new(ErrorKind::InvalidInput, "symbolic-link reparse data is too long"))?;
    let total_length = 8_usize
        .checked_add(usize::from(data_length))
        .ok_or_else(|| Error::new(ErrorKind::InvalidInput, "symbolic-link reparse buffer overflowed"))?;
    if total_length > MAXIMUM_REPARSE_DATA_BUFFER_SIZE as usize {
        return Err(Error::new(
            ErrorKind::InvalidInput,
            "symbolic-link reparse buffer exceeds the native limit",
        ));
    }

    let mut buffer = vec![0_u8; total_length];
    write_u32(&mut buffer, 0, IO_REPARSE_TAG_SYMLINK);
    write_u16(&mut buffer, 4, data_length);
    write_u16(&mut buffer, 8, 0);
    write_u16(&mut buffer, 10, substitute_bytes);
    write_u16(&mut buffer, 12, substitute_bytes);
    write_u16(&mut buffer, 14, print_bytes);
    write_u32(&mut buffer, 16, flags);
    write_wide(&mut buffer, SYMLINK_PATH_BUFFER_OFFSET, &substitute_name);
    write_wide(
        &mut buffer,
        SYMLINK_PATH_BUFFER_OFFSET + usize::from(substitute_bytes),
        &print_name,
    );
    Ok(buffer)
}

/// Parses and validates the user-visible target from a native reparse record.
fn parse_symbolic_link_target(buffer: &[u8]) -> Result<PathBuf> {
    if buffer.len() < SYMLINK_PATH_BUFFER_OFFSET {
        return Err(corrupt_reparse("symbolic-link reparse buffer is truncated"));
    }
    if read_u32(buffer, 0)? != IO_REPARSE_TAG_SYMLINK {
        return Err(Error::new(
            ErrorKind::Unsupported,
            "reparse point is not a symbolic link",
        ));
    }
    let data_length = usize::from(read_u16(buffer, 4)?);
    let record_length = 8_usize
        .checked_add(data_length)
        .filter(|length| *length <= buffer.len())
        .ok_or_else(|| corrupt_reparse("symbolic-link reparse data length is invalid"))?;
    let substitute_offset = usize::from(read_u16(buffer, 8)?);
    let substitute_length = usize::from(read_u16(buffer, 10)?);
    let print_offset = usize::from(read_u16(buffer, 12)?);
    let print_length = usize::from(read_u16(buffer, 14)?);
    let (offset, length, strip_native_prefix) = if print_length == 0 {
        (substitute_offset, substitute_length, true)
    } else {
        (print_offset, print_length, false)
    };
    let units = read_wide_range(buffer, record_length, offset, length)?;
    let units = if strip_native_prefix {
        normalize_substitute_name(&units)
    } else {
        units
    };
    Ok(PathBuf::from(OsString::from_wide(&units)))
}

/// Produces the kernel substitute name and relative-link flag.
fn substitute_name(target: &Path, print_name: &[u16]) -> (Vec<u16>, u32) {
    const NT_PREFIX: &[u16] = &[b'\\' as u16, b'?' as u16, b'?' as u16, b'\\' as u16];
    const NT_UNC_PREFIX: &[u16] = &[
        b'\\' as u16,
        b'?' as u16,
        b'?' as u16,
        b'\\' as u16,
        b'U' as u16,
        b'N' as u16,
        b'C' as u16,
        b'\\' as u16,
    ];
    const WIN32_VERBATIM_PREFIX: &[u16] = &[b'\\' as u16, b'\\' as u16, b'?' as u16, b'\\' as u16];
    const WIN32_VERBATIM_UNC_PREFIX: &[u16] = &[
        b'\\' as u16,
        b'\\' as u16,
        b'?' as u16,
        b'\\' as u16,
        b'U' as u16,
        b'N' as u16,
        b'C' as u16,
        b'\\' as u16,
    ];
    if !target.is_absolute() {
        return (print_name.to_vec(), SYMLINK_FLAG_RELATIVE);
    }
    let mut substitute = Vec::with_capacity(print_name.len() + NT_UNC_PREFIX.len());
    if let Some(rest) = print_name.strip_prefix(WIN32_VERBATIM_UNC_PREFIX) {
        substitute.extend_from_slice(NT_UNC_PREFIX);
        substitute.extend_from_slice(rest);
    } else if let Some(rest) = print_name.strip_prefix(WIN32_VERBATIM_PREFIX) {
        substitute.extend_from_slice(NT_PREFIX);
        substitute.extend_from_slice(rest);
    } else if print_name.starts_with(&[b'\\' as u16, b'\\' as u16]) {
        substitute.extend_from_slice(NT_UNC_PREFIX);
        substitute.extend_from_slice(&print_name[2..]);
    } else {
        substitute.extend_from_slice(NT_PREFIX);
        substitute.extend_from_slice(print_name);
    }
    (substitute, 0)
}

/// Removes a Win32 NT namespace prefix from a substitute-name fallback.
fn normalize_substitute_name(units: &[u16]) -> Vec<u16> {
    const NT_PREFIX: &[u16] = &[b'\\' as u16, b'?' as u16, b'?' as u16, b'\\' as u16];
    const NT_UNC_PREFIX: &[u16] = &[
        b'\\' as u16,
        b'?' as u16,
        b'?' as u16,
        b'\\' as u16,
        b'U' as u16,
        b'N' as u16,
        b'C' as u16,
        b'\\' as u16,
    ];
    if let Some(rest) = units.strip_prefix(NT_UNC_PREFIX) {
        [b'\\' as u16, b'\\' as u16]
            .into_iter()
            .chain(rest.iter().copied())
            .collect()
    } else if let Some(rest) = units.strip_prefix(NT_PREFIX) {
        rest.to_vec()
    } else {
        units.to_vec()
    }
}

/// Converts a UTF-16 unit count into a native 16-bit byte length.
fn checked_utf16_byte_length(units: usize) -> Result<u16> {
    units
        .checked_mul(size_of::<u16>())
        .and_then(|length| u16::try_from(length).ok())
        .ok_or_else(|| Error::new(ErrorKind::InvalidInput, "symbolic-link target is too long"))
}

/// Reads one little-endian 16-bit value from a validated native buffer.
fn read_u16(buffer: &[u8], offset: usize) -> Result<u16> {
    let bytes = buffer
        .get(offset..offset + 2)
        .ok_or_else(|| corrupt_reparse("symbolic-link reparse field is truncated"))?;
    Ok(u16::from_le_bytes([bytes[0], bytes[1]]))
}

/// Reads one little-endian 32-bit value from a validated native buffer.
fn read_u32(buffer: &[u8], offset: usize) -> Result<u32> {
    let bytes = buffer
        .get(offset..offset + 4)
        .ok_or_else(|| corrupt_reparse("symbolic-link reparse field is truncated"))?;
    Ok(u32::from_le_bytes([bytes[0], bytes[1], bytes[2], bytes[3]]))
}

/// Reads one UTF-16 slice addressed relative to the reparse path buffer.
fn read_wide_range(buffer: &[u8], record_length: usize, offset: usize, length: usize) -> Result<Vec<u16>> {
    if !offset.is_multiple_of(size_of::<u16>()) || !length.is_multiple_of(size_of::<u16>()) {
        return Err(corrupt_reparse("symbolic-link reparse name is not UTF-16 aligned"));
    }
    let start = SYMLINK_PATH_BUFFER_OFFSET
        .checked_add(offset)
        .ok_or_else(|| corrupt_reparse("symbolic-link reparse name offset overflowed"))?;
    let end = start
        .checked_add(length)
        .filter(|end| *end <= record_length)
        .ok_or_else(|| corrupt_reparse("symbolic-link reparse name exceeds its record"))?;
    Ok(buffer[start..end]
        .chunks_exact(size_of::<u16>())
        .map(|bytes| u16::from_le_bytes([bytes[0], bytes[1]]))
        .collect())
}

/// Builds an invalid-data error for a malformed native reparse record.
fn corrupt_reparse(message: &'static str) -> Error {
    Error::new(ErrorKind::InvalidData, message)
}

/// Writes one little-endian 16-bit field into a sized builder buffer.
fn write_u16(buffer: &mut [u8], offset: usize, value: u16) {
    buffer[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
}

/// Writes one little-endian 32-bit field into a sized builder buffer.
fn write_u32(buffer: &mut [u8], offset: usize, value: u32) {
    buffer[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}

/// Writes UTF-16 units into a sized builder buffer.
fn write_wide(buffer: &mut [u8], offset: usize, units: &[u16]) {
    for (index, unit) in units.iter().enumerate() {
        let start = offset + index * size_of::<u16>();
        write_u16(buffer, start, *unit);
    }
}

#[cfg(test)]
mod tests {
    use std::io::ErrorKind;
    use std::path::Path;

    use super::build_symbolic_link_buffer;
    use super::parse_symbolic_link_target;

    /// Verifies relative dangling targets survive native reparse encoding.
    #[test]
    fn test_relative_symbolic_link_reparse_buffer_round_trips() {
        let target = Path::new(r"..\missing\target");
        let buffer = build_symbolic_link_buffer(target).expect("relative reparse buffer should encode");

        assert_eq!(target, parse_symbolic_link_target(&buffer).unwrap());
    }

    /// Verifies absolute targets retain their Win32 spelling for callers.
    #[test]
    fn test_absolute_symbolic_link_reparse_buffer_round_trips() {
        let target = Path::new(r"C:\outside\target");
        let buffer = build_symbolic_link_buffer(target).expect("absolute reparse buffer should encode");

        assert_eq!(target, parse_symbolic_link_target(&buffer).unwrap());
    }

    /// Verifies verbatim absolute targets retain their user-visible spelling.
    #[test]
    fn test_verbatim_symbolic_link_reparse_buffer_round_trips() {
        for target in [
            Path::new(r"\\?\C:\outside\target"),
            Path::new(r"\\?\UNC\server\share\target"),
        ] {
            let buffer = build_symbolic_link_buffer(target).expect("verbatim reparse buffer should encode");

            assert_eq!(target, parse_symbolic_link_target(&buffer).unwrap());
        }
    }

    /// Verifies malformed native records are classified as data corruption.
    #[test]
    fn test_truncated_symbolic_link_reparse_buffer_is_invalid_data() {
        let error = parse_symbolic_link_target(&[0; 8]).expect_err("a truncated record must fail");

        assert_eq!(ErrorKind::InvalidData, error.kind());
    }

    /// Verifies unsupported reparse tags do not masquerade as corrupt links.
    #[test]
    fn test_unknown_reparse_tag_is_unsupported() {
        let error = parse_symbolic_link_target(&[0; 20]).expect_err("an unknown tag must fail");

        assert_eq!(ErrorKind::Unsupported, error.kind());
    }
}
