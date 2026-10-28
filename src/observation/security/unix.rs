// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::fs::File;
use std::io;
use std::os::fd::AsRawFd;

use crate::LocalResult;
use crate::error::LocalFileError;
use crate::error::LocalFileErrorKind;
use crate::error::LocalFileOperation;
use crate::error::LocalResourceKind;
use crate::error::LocalResourceLimitError;
use crate::observation::LocalAccessObservation;
#[cfg(target_os = "linux")]
use crate::observation::LocalAclPresence;
use crate::observation::LocalFileIdentity;
#[cfg(any(target_os = "linux", target_os = "macos"))]
use crate::observation::LocalFileSystemLocality;
use crate::observation::LocalFileSystemObservation;
use crate::observation::LocalObservationRequirement;
use crate::observation::LocalSecurityObservation;
use crate::observation::LocalSecurityObservationOptions;
#[cfg(target_os = "linux")]
use crate::observation::LocalUnixAccessObservation;
#[cfg(any(target_os = "linux", target_os = "macos"))]
use crate::observation::LocalUnixFileSystemObservation;
#[cfg(any(target_os = "linux", target_os = "macos"))]
use crate::observation::LocalUnixPlatform;

const ACL_RETRIES: usize = 8;
const LINUX_ACL_VERSION: u32 = 2;
const LINUX_ACL_ENTRY_SIZE: usize = 8;

pub(crate) fn query_security(
    file: &File,
    options: &LocalSecurityObservationOptions,
) -> LocalResult<LocalSecurityObservation> {
    let before = stat(file)?;
    #[allow(clippy::useless_conversion)]
    let device = u64::try_from(before.st_dev).map_err(|_| invalid_native_metadata())?;
    #[allow(clippy::useless_conversion)]
    let inode = u64::try_from(before.st_ino).map_err(|_| invalid_native_metadata())?;
    let identity = LocalFileIdentity::unix(device, inode);
    #[allow(clippy::useless_conversion)]
    let link_count = u64::try_from(before.st_nlink)
        .map(Some)
        .map_err(|_| invalid_native_metadata())?;
    let filesystem = query_filesystem(file, options.filesystem())?;
    let access = query_access(file, &before, options)?;
    let after = stat(file)?;
    if before.st_dev != after.st_dev || before.st_ino != after.st_ino {
        return Err(LocalFileError::new(
            LocalFileErrorKind::ChangedDuringRead,
            LocalFileOperation::Metadata,
        ));
    }
    Ok(LocalSecurityObservation::new(
        Some(identity),
        filesystem,
        access,
        link_count,
    ))
}

fn stat(file: &File) -> LocalResult<libc::stat> {
    // SAFETY: `stat` is initialized by fstat on success and `file` owns a valid
    // descriptor.
    let mut stat = unsafe { std::mem::zeroed::<libc::stat>() };
    // SAFETY: the descriptor is borrowed for the duration of this call and the
    // output is writable.
    if unsafe { libc::fstat(file.as_raw_fd(), &mut stat) } == -1 {
        return Err(io_error(io::Error::last_os_error()));
    }
    Ok(stat)
}

fn requested_value<T>(
    requirement: LocalObservationRequirement,
    query: impl FnOnce() -> io::Result<T>,
) -> LocalResult<Option<T>> {
    match query() {
        Ok(value) => Ok(Some(value)),
        Err(error) if explicitly_unsupported(&error) => {
            if requirement == LocalObservationRequirement::Required {
                Err(LocalFileError::new(
                    LocalFileErrorKind::RequirementNotMet,
                    LocalFileOperation::Metadata,
                ))
            } else {
                Ok(None)
            }
        }
        Err(error) => Err(io_error(error)),
    }
}

fn query_filesystem(
    file: &File,
    requirement: LocalObservationRequirement,
) -> LocalResult<Option<LocalFileSystemObservation>> {
    requested_value(requirement, || filesystem_facts(file))
}

#[cfg(target_os = "linux")]
fn filesystem_facts(file: &File) -> io::Result<LocalFileSystemObservation> {
    // SAFETY: `statfs` is initialized by fstatfs on success and the descriptor is
    // valid.
    let mut stats = unsafe { std::mem::zeroed::<libc::statfs>() };
    // SAFETY: the descriptor and writable output are valid for this call.
    if unsafe { libc::fstatfs(file.as_raw_fd(), &mut stats) } == -1 {
        return Err(io::Error::last_os_error());
    }
    let native_type = libc::c_ulong::from_ne_bytes(stats.f_type.to_ne_bytes());
    let locality = match u32::try_from(native_type).ok() {
        Some(fs_type) => match fs_type {
            0x6969 | 0x517B | 0xFF53 | 0xFE534D42 => LocalFileSystemLocality::Remote,
            0xEF53 | 0x58465342 | 0x01021994 | 0x794C7630 | 0x9123683E => LocalFileSystemLocality::Local,
            _ => LocalFileSystemLocality::Unknown,
        },
        None => LocalFileSystemLocality::Unknown,
    };
    // `c_ulong` is 32-bit on 32-bit Linux and 64-bit on 64-bit Linux.
    #[allow(clippy::useless_conversion)]
    let type_id = u64::from(native_type);
    Ok(LocalFileSystemObservation::new(
        locality,
        Some(LocalUnixFileSystemObservation::new(
            LocalUnixPlatform::Linux,
            type_id,
            None,
        )),
        None,
    ))
}

#[cfg(target_os = "macos")]
fn filesystem_facts(file: &File) -> io::Result<LocalFileSystemObservation> {
    // SAFETY: `statfs` is initialized by fstatfs on success and the descriptor is
    // valid.
    let mut stats = unsafe { std::mem::zeroed::<libc::statfs>() };
    // SAFETY: the descriptor and writable output are valid for this call.
    if unsafe { libc::fstatfs(file.as_raw_fd(), &mut stats) } == -1 {
        return Err(io::Error::last_os_error());
    }
    let type_name = unsafe { std::ffi::CStr::from_ptr(stats.f_fstypename.as_ptr()) }
        .to_string_lossy()
        .into_owned();
    let locality = match type_name.as_str() {
        "nfs" | "smbfs" | "webdav" => LocalFileSystemLocality::Remote,
        "apfs" | "hfs" | "ufs" => LocalFileSystemLocality::Local,
        _ => LocalFileSystemLocality::Unknown,
    };
    Ok(LocalFileSystemObservation::new(
        locality,
        Some(LocalUnixFileSystemObservation::new(
            LocalUnixPlatform::Darwin,
            u64::from(stats.f_type),
            Some(type_name),
        )),
        None,
    ))
}

#[cfg(all(unix, not(any(target_os = "linux", target_os = "macos"))))]
fn filesystem_facts(_file: &File) -> io::Result<LocalFileSystemObservation> {
    Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "filesystem facts unavailable",
    ))
}

#[cfg(target_os = "linux")]
fn query_access(
    file: &File,
    stat: &libc::stat,
    options: &LocalSecurityObservationOptions,
) -> LocalResult<Option<LocalAccessObservation>> {
    requested_value(options.access(), || query_linux_access(file, stat, options))
        .map(|value| value.map(|access| LocalAccessObservation::new(Some(access), None)))
}

#[cfg(not(target_os = "linux"))]
fn query_access(
    _file: &File,
    _stat: &libc::stat,
    options: &LocalSecurityObservationOptions,
) -> LocalResult<Option<LocalAccessObservation>> {
    match options.access() {
        LocalObservationRequirement::Optional => Ok(None),
        LocalObservationRequirement::Required => Err(LocalFileError::new(
            LocalFileErrorKind::RequirementNotMet,
            LocalFileOperation::Metadata,
        )
        .with_reason("complete Unix ACL facts cannot be queried within the configured bounds")),
    }
}

#[cfg(target_os = "linux")]
fn query_linux_access(
    file: &File,
    stat: &libc::stat,
    options: &LocalSecurityObservationOptions,
) -> io::Result<LocalUnixAccessObservation> {
    let access_acl = query_linux_acl(file, b"system.posix_acl_access\0", options)?;
    let default_acl = query_linux_acl(file, b"system.posix_acl_default\0", options)?;
    // SAFETY: geteuid has no preconditions.
    let owner_is_current_user = unsafe { libc::geteuid() } == stat.st_uid;
    #[allow(clippy::useless_conversion)]
    let mode = u32::try_from(stat.st_mode & 0o7777)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "invalid native permission mode"))?;
    Ok(LocalUnixAccessObservation::new(
        mode,
        owner_is_current_user,
        access_acl,
        default_acl,
    ))
}

#[cfg(target_os = "linux")]
fn query_linux_acl(
    file: &File,
    name: &'static [u8],
    options: &LocalSecurityObservationOptions,
) -> io::Result<LocalAclPresence> {
    let mut buffer = Vec::new();
    for _ in 0..ACL_RETRIES {
        // SAFETY: xattr name is NUL-terminated; null output with size zero requests its
        // length.
        let length = unsafe { libc::fgetxattr(file.as_raw_fd(), name.as_ptr().cast(), std::ptr::null_mut(), 0) };
        if length == -1 {
            let error = io::Error::last_os_error();
            if error.raw_os_error() == Some(libc::ENODATA) {
                return Ok(LocalAclPresence::Absent);
            }
            if explicitly_unsupported(&error) {
                return Ok(LocalAclPresence::NotSupported);
            }
            return Err(error);
        }
        let length = usize::try_from(length).map_err(|_| invalid_acl())?;
        check_acl_length(length, options)?;
        buffer.clear();
        buffer
            .try_reserve_exact(length)
            .map_err(|_| io::Error::new(io::ErrorKind::OutOfMemory, "ACL allocation failed"))?;
        buffer.resize(length, 0);
        // SAFETY: the buffer has `length` writable bytes and the descriptor/name remain
        // valid.
        let read = unsafe {
            libc::fgetxattr(
                file.as_raw_fd(),
                name.as_ptr().cast(),
                buffer.as_mut_ptr().cast(),
                buffer.len(),
            )
        };
        if read == -1 {
            let error = io::Error::last_os_error();
            if acl_read_requires_retry(&error) {
                continue;
            }
            return Err(error);
        }
        if usize::try_from(read).ok() != Some(length) {
            continue;
        }
        if buffer.is_empty() {
            return Err(invalid_acl());
        }
        validate_linux_acl(&buffer, options.max_acl_entries())?;
        return Ok(LocalAclPresence::Present);
    }
    Err(io::Error::other("ACL changed during bounded read retries"))
}

#[cfg(target_os = "linux")]
fn validate_linux_acl(bytes: &[u8], max_entries: Option<usize>) -> io::Result<()> {
    if bytes.len() < 4 || u32::from_le_bytes(bytes[0..4].try_into().map_err(|_| invalid_acl())?) != LINUX_ACL_VERSION {
        return Err(invalid_acl());
    }
    let entries_bytes = bytes.len() - 4;
    if !entries_bytes.is_multiple_of(LINUX_ACL_ENTRY_SIZE) {
        return Err(invalid_acl());
    }
    let entries = entries_bytes / LINUX_ACL_ENTRY_SIZE;
    if let Some(max) = max_entries
        && entries > max
    {
        return Err(io::Error::other(
            LocalResourceLimitError::new(LocalResourceKind::AclEntries, max, max, entries).with_observed(entries),
        ));
    }
    let mut user_obj = 0;
    let mut group_obj = 0;
    let mut other = 0;
    let mut mask = 0;
    let mut named_entries = 0;
    for entry in bytes[4..].chunks_exact(LINUX_ACL_ENTRY_SIZE) {
        let tag = u16::from_le_bytes(entry[0..2].try_into().map_err(|_| invalid_acl())?);
        let perm = u16::from_le_bytes(entry[2..4].try_into().map_err(|_| invalid_acl())?);
        let id = u32::from_le_bytes(entry[4..8].try_into().map_err(|_| invalid_acl())?);
        if !matches!(tag, 0x01 | 0x02 | 0x04 | 0x08 | 0x10 | 0x20) || perm & !0x7 != 0 {
            return Err(invalid_acl());
        }
        match tag {
            0x01 => {
                user_obj += 1;
                if id != u32::MAX {
                    return Err(invalid_acl());
                }
            }
            0x02 | 0x08 => {
                named_entries += 1;
                if id == u32::MAX {
                    return Err(invalid_acl());
                }
            }
            0x04 => {
                group_obj += 1;
                if id != u32::MAX {
                    return Err(invalid_acl());
                }
            }
            0x10 => {
                mask += 1;
                if id != u32::MAX {
                    return Err(invalid_acl());
                }
            }
            0x20 => {
                other += 1;
                if id != u32::MAX {
                    return Err(invalid_acl());
                }
            }
            _ => unreachable!(),
        }
    }
    if user_obj != 1 || group_obj != 1 || other != 1 || mask > 1 || (named_entries > 0 && mask != 1) {
        return Err(invalid_acl());
    }
    Ok(())
}

fn io_error(error: io::Error) -> LocalFileError {
    LocalFileError::from_io(LocalFileOperation::Metadata, None, None, error)
}

fn invalid_acl() -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, "malformed POSIX ACL")
}

fn invalid_native_metadata() -> LocalFileError {
    LocalFileError::new(LocalFileErrorKind::DataCorruption, LocalFileOperation::Metadata)
}

fn explicitly_unsupported(error: &io::Error) -> bool {
    error
        .raw_os_error()
        .is_some_and(|code| code == libc::ENOTSUP || code == libc::EOPNOTSUPP || code == libc::ENOSYS)
}

#[cfg(any(target_os = "linux", test))]
fn acl_read_requires_retry(error: &io::Error) -> bool {
    let Some(code) = error.raw_os_error() else {
        return false;
    };
    if code == libc::ERANGE {
        return true;
    }
    #[cfg(target_os = "linux")]
    if code == libc::ENODATA {
        return true;
    }
    false
}

fn check_acl_size_io(length: usize, options: &LocalSecurityObservationOptions) -> io::Result<()> {
    if let Some(max) = options.max_acl_bytes()
        && length > max
    {
        return Err(io::Error::other(
            LocalResourceLimitError::new(LocalResourceKind::AclBytes, max, max, length).with_observed(length),
        ));
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn check_acl_length(length: usize, options: &LocalSecurityObservationOptions) -> io::Result<()> {
    check_acl_size_io(length, options)?;
    if length < 4 || !(length - 4).is_multiple_of(LINUX_ACL_ENTRY_SIZE) {
        return Err(invalid_acl());
    }
    let entries = (length - 4) / LINUX_ACL_ENTRY_SIZE;
    if let Some(max) = options.max_acl_entries()
        && entries > max
    {
        return Err(io::Error::other(
            LocalResourceLimitError::new(LocalResourceKind::AclEntries, max, max, entries).with_observed(entries),
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use std::fs;
    use std::os::unix::fs::MetadataExt;

    use super::File;
    use super::LocalFileErrorKind;
    use super::LocalFileIdentity;
    use super::LocalObservationRequirement;
    use super::LocalResourceKind;
    use super::LocalResult;
    use super::LocalSecurityObservationOptions;
    use super::acl_read_requires_retry;
    use super::check_acl_length;
    use super::io;
    use super::io_error;
    use super::query_security;
    use super::requested_value;
    use super::validate_linux_acl;

    #[test]
    fn security_unix_observes_retained_file_after_path_replacement() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let path = directory.path().join("object");
        fs::write(&path, b"original").expect("create original");
        let retained = File::open(&path).expect("open original");
        let expected = retained.metadata().expect("metadata").ino();
        fs::remove_file(&path).expect("remove name");
        fs::write(&path, b"replacement").expect("create replacement");

        let observation = query_security(
            &retained,
            &LocalSecurityObservationOptions::new().with_access(LocalObservationRequirement::Required),
        )
        .expect("observe retained object");
        assert_eq!(
            observation.identity().expect("identity").clone(),
            LocalFileIdentity::unix(retained.metadata().unwrap().dev(), expected)
        );
        assert_eq!(observation.link_count(), Some(0));
    }

    #[test]
    fn security_unix_reports_same_handle_link_count() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let path = directory.path().join("object");
        let alias = directory.path().join("alias");
        fs::write(&path, b"same inode").expect("create file");
        let retained = File::open(&path).expect("open file");
        let options = LocalSecurityObservationOptions::new().with_link_count(LocalObservationRequirement::Required);
        assert_eq!(query_security(&retained, &options).unwrap().link_count(), Some(1));
        fs::hard_link(&path, alias).expect("create hard link");
        assert_eq!(query_security(&retained, &options).unwrap().link_count(), Some(2));
    }

    #[test]
    fn security_unix_preserves_optional_permission_errors() {
        let denied = io::Error::from_raw_os_error(libc::EACCES);
        let result: LocalResult<Option<u8>> = requested_value(LocalObservationRequirement::Optional, || Err(denied));
        assert_eq!(
            result.expect_err("permission denial must propagate").kind(),
            LocalFileErrorKind::PermissionDenied
        );
    }

    #[test]
    fn security_unix_retries_acl_length_change_errors() {
        assert!(acl_read_requires_retry(&io::Error::from_raw_os_error(libc::ERANGE)));
        #[cfg(target_os = "linux")]
        assert!(acl_read_requires_retry(&io::Error::from_raw_os_error(libc::ENODATA)));
        assert!(!acl_read_requires_retry(&io::Error::from_raw_os_error(libc::EACCES)));
    }

    #[test]
    fn security_unix_zero_acl_byte_limit_rejects_nonempty_acl() {
        let acl = [
            2, 0, 0, 0, 1, 0, 7, 0, 0xff, 0xff, 0xff, 0xff, 4, 0, 5, 0, 0xff, 0xff, 0xff, 0xff, 0x20, 0, 0, 0, 0xff,
            0xff, 0xff, 0xff,
        ];
        assert!(validate_linux_acl(&acl, None).is_ok());
        assert!(validate_linux_acl(&acl, Some(0)).is_err());
        let options = LocalSecurityObservationOptions::new().with_max_acl_bytes(0);
        let result = io_error(check_acl_length(acl.len(), &options).unwrap_err());
        assert_eq!(result.kind(), LocalFileErrorKind::ResourceLimit);
        let limit = result.resource_limit_error().expect("structured ACL byte limit");
        assert_eq!(limit.resource(), LocalResourceKind::AclBytes);
        assert_eq!(limit.observed(), Some(acl.len()));

        let options = LocalSecurityObservationOptions::new().with_max_acl_entries(0);
        let result = io_error(check_acl_length(acl.len(), &options).unwrap_err());
        let limit = result.resource_limit_error().expect("structured ACL entry limit");
        assert_eq!(limit.resource(), LocalResourceKind::AclEntries);
        assert_eq!(limit.observed(), Some(3));
    }
}
