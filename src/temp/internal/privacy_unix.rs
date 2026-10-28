// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
#[cfg(test)]
use std::cell::Cell;
use std::ffi::CString;
use std::ffi::OsStr;
use std::fs::File;
use std::io;
use std::os::fd::AsRawFd;
use std::os::fd::FromRawFd;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::MetadataExt;

#[cfg(test)]
thread_local! {
    static ACCESS_FAILURE_AFTER: Cell<Option<usize>> = const { Cell::new(None) };
}

#[cfg(test)]
pub(crate) fn fail_access_after(successful_steps: usize) {
    ACCESS_FAILURE_AFTER.with(|slot| slot.set(Some(successful_steps)));
}

#[cfg(test)]
fn fail_access_if_scheduled() -> io::Result<()> {
    let fail = ACCESS_FAILURE_AFTER.with(|slot| match slot.get() {
        Some(0) => {
            slot.set(None);
            true
        }
        Some(remaining) => {
            slot.set(Some(remaining - 1));
            false
        }
        None => false,
    });
    if fail {
        Err(io::Error::other("injected access policy failure"))
    } else {
        Ok(())
    }
}

use crate::observation::LocalFileIdentity;
use crate::policy::LocalOwnerAccessPolicy;
use crate::temp::types::NativePrivateCreateError;
use crate::temp::types::NativePrivateEntry;

fn owner_mode(policy: &LocalOwnerAccessPolicy, directory: bool) -> io::Result<u32> {
    let (directory_mode, file_mode) = policy.unix_modes().ok_or_else(|| {
        io::Error::new(io::ErrorKind::InvalidInput, "Unix owner access policy is not configured")
    })?;
    if directory_mode & !0o700 != 0 || file_mode & !0o700 != 0 {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "Unix owner modes may contain only 0o700 permission bits"));
    }
    Ok(if directory { directory_mode } else { file_mode })
}

pub(crate) fn verify_owner_access(file: &File, directory: bool, policy: &LocalOwnerAccessPolicy) -> io::Result<()> {
    let expected = owner_mode(policy, directory)?;
    ensure_acl_backend_supported()?;
    verify_owner_and_type(file, directory)?;
    let metadata = file.metadata()?;
    if metadata.mode() & 0o7777 != expected {
        return Err(io::Error::new(io::ErrorKind::PermissionDenied, "entry mode does not exactly match owner policy"));
    }
    verify_no_extra_acl(file, directory)
}

pub(crate) fn apply_owner_access(file: &File, directory: bool, policy: &LocalOwnerAccessPolicy) -> io::Result<()> {
    let mode = owner_mode(policy, directory)?;
    ensure_acl_backend_supported()?;
    verify_owner_and_type(file, directory)?;
    verify_no_extra_acl(file, directory)?;
    #[cfg(test)]
    fail_access_if_scheduled()?;
    if unsafe { libc::fchmod(file.as_raw_fd(), mode) } < 0 {
        return Err(io::Error::last_os_error());
    }
    verify_owner_access(file, directory, policy)
}

pub(crate) fn create_owner_entry_at(
    parent: &File,
    name: &OsStr,
    directory: bool,
    policy: &LocalOwnerAccessPolicy,
) -> Result<File, NativePrivateCreateError> {
    let mode = match owner_mode(policy, directory) {
        Ok(mode) => mode,
        Err(source) => return Err(pre_create_error(source)),
    };
    if let Err(source) = ensure_acl_backend_supported() {
        return Err(pre_create_error(source));
    }
    let name = match component_name(name) {
        Ok(name) => name,
        Err(source) => return Err(pre_create_error(source)),
    };
    let retained_parent = match parent.try_clone() {
        Ok(parent) => parent,
        Err(source) => return Err(pre_create_error(source)),
    };
    if directory {
        if unsafe { libc::mkdirat(parent.as_raw_fd(), name.as_ptr(), mode as libc::mode_t) } < 0 {
            return Err(pre_create_error(io::Error::last_os_error()));
        }
    } else {
        let descriptor = unsafe {
            libc::openat(parent.as_raw_fd(), name.as_ptr(), libc::O_RDWR | libc::O_CREAT | libc::O_EXCL | libc::O_NOFOLLOW | libc::O_CLOEXEC, mode as libc::mode_t)
        };
        if descriptor < 0 {
            return Err(pre_create_error(io::Error::last_os_error()));
        }
        // SAFETY: openat returned a fresh descriptor owned by this function.
        let file = unsafe { File::from_raw_fd(descriptor) };
        return finish_owner_creation(parent, retained_parent, name, file, false, policy);
    }
    let descriptor = unsafe { libc::openat(parent.as_raw_fd(), name.as_ptr(), libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC) };
    if descriptor < 0 {
        let source = io::Error::last_os_error();
        let identity = identity_for_name(parent, OsStr::from_bytes(name.as_bytes())).ok();
        return Err(create_error(
            true,
            source,
            retained_parent,
            OsStr::from_bytes(name.as_bytes()),
            None,
            identity,
        ));
    }
    // SAFETY: openat returned a fresh descriptor owned by this function.
    let file = unsafe { File::from_raw_fd(descriptor) };
    finish_owner_creation(parent, retained_parent, name, file, true, policy)
}

fn finish_owner_creation(
    parent: &File,
    retained_parent: File,
    name: CString,
    file: File,
    directory: bool,
    policy: &LocalOwnerAccessPolicy,
) -> Result<File, NativePrivateCreateError> {
    let identity = identity_for_file(&file).ok();
    let result = (|| {
        let mode = owner_mode(policy, directory)?;
        if unsafe { libc::fchmod(file.as_raw_fd(), mode) } < 0 {
            return Err(io::Error::last_os_error());
        }
        if directory {
            let found = identity_for_name(parent, OsStr::from_bytes(name.as_bytes()))?;
            if identity.as_ref() != Some(&found) {
                return Err(io::Error::new(io::ErrorKind::PermissionDenied, "directory entry changed after handle acquisition"));
            }
        }
        verify_owner_access(&file, directory, policy)
    })();
    match result {
        Ok(()) => Ok(file),
        Err(source) => Err(create_error(directory, source, retained_parent, OsStr::from_bytes(name.as_bytes()), Some(file), identity)),
    }
}

fn verify_no_extra_acl(file: &File, directory: bool) -> io::Result<()> {
    #[cfg(target_os = "linux")]
    {
        use std::os::fd::AsRawFd;
        use std::ffi::CString;
        let names: &[&[u8]] = if directory {
            &[b"system.posix_acl_access\0", b"system.posix_acl_default\0"]
        } else {
            &[b"system.posix_acl_access\0"]
        };
        for bytes in names {
            let name = CString::new(&bytes[..bytes.len() - 1]).expect("static xattr name");
            let result = unsafe { libc::fgetxattr(file.as_raw_fd(), name.as_ptr(), std::ptr::null_mut(), 0) };
            if result >= 0 {
                return Err(io::Error::new(io::ErrorKind::PermissionDenied, "entry has an extended POSIX ACL"));
            }
            let error = io::Error::last_os_error();
            if error.raw_os_error() != Some(libc::ENODATA) {
                return Err(error);
            }
        }
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (file, directory);
        return Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "exact Unix owner ACL verification is not implemented on this platform",
        ));
    }
    Ok(())
}

fn ensure_acl_backend_supported() -> io::Result<()> {
    #[cfg(not(target_os = "linux"))]
    return Err(io::Error::new(
        io::ErrorKind::Unsupported,
        "exact Unix owner ACL verification is not implemented on this platform",
    ));
    #[cfg(target_os = "linux")]
    Ok(())
}

pub(crate) fn create_private_file_at(parent: &File, name: &OsStr) -> Result<File, NativePrivateCreateError> {
    create_private_file_at_impl(parent, name, File::try_clone)
}

#[cfg(test)]
pub(crate) fn create_private_file_at_with_clone_hook<F>(
    parent: &File,
    name: &OsStr,
    clone_parent: F,
) -> Result<File, NativePrivateCreateError>
where
    F: FnOnce(&File) -> io::Result<File>,
{
    create_private_file_at_impl(parent, name, clone_parent)
}

fn create_private_file_at_impl<F>(
    parent: &File,
    name: &OsStr,
    clone_parent: F,
) -> Result<File, NativePrivateCreateError>
where
    F: FnOnce(&File) -> io::Result<File>,
{
    let name = match component_name(name) {
        Ok(name) => name,
        Err(source) => return Err(pre_create_error(source)),
    };
    let retained_parent = clone_parent(parent).map_err(pre_create_error)?;
    let descriptor = unsafe {
        libc::openat(
            parent.as_raw_fd(),
            name.as_ptr(),
            libc::O_RDWR | libc::O_CREAT | libc::O_EXCL | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            0o600,
        )
    };
    if descriptor < 0 {
        return Err(pre_create_error(io::Error::last_os_error()));
    }
    // SAFETY: openat returned a fresh descriptor owned by this function.
    let file = unsafe { File::from_raw_fd(descriptor) };
    let metadata = match file.metadata() {
        Ok(metadata) => metadata,
        Err(source) => {
            let identity = identity_for_file(&file).ok();
            return Err(create_error(
                false,
                source,
                retained_parent,
                OsStr::from_bytes(name.as_bytes()),
                Some(file),
                identity,
            ));
        }
    };
    let identity = identity_for_file(&file).ok();
    let mode = metadata.mode() & 0o777;
    if !metadata.file_type().is_file() || metadata.uid() != unsafe { libc::geteuid() } {
        return Err(create_error(
            false,
            io::Error::new(
                io::ErrorKind::PermissionDenied,
                "created entry is not an owned regular file",
            ),
            retained_parent,
            OsStr::from_bytes(name.as_bytes()),
            Some(file),
            identity,
        ));
    }
    if mode & 0o077 != 0 || mode & 0o600 != 0o600 {
        return Err(create_error(
            false,
            io::Error::new(
                io::ErrorKind::PermissionDenied,
                "umask prevented required private file mode 0600",
            ),
            retained_parent,
            OsStr::from_bytes(name.as_bytes()),
            Some(file),
            identity,
        ));
    }
    Ok(file)
}

pub(crate) fn create_private_directory_at(parent: &File, name: &OsStr) -> Result<File, NativePrivateCreateError> {
    create_private_directory_at_impl(parent, name, File::try_clone, || {})
}

#[cfg(test)]
pub(crate) fn create_private_directory_at_with_clone_hook<F>(
    parent: &File,
    name: &OsStr,
    clone_parent: F,
) -> Result<File, NativePrivateCreateError>
where
    F: FnOnce(&File) -> io::Result<File>,
{
    create_private_directory_at_impl(parent, name, clone_parent, || {})
}

#[cfg(test)]
pub(crate) fn create_private_directory_at_with_hook<F>(
    parent: &File,
    name: &OsStr,
    after_open: F,
) -> Result<File, NativePrivateCreateError>
where
    F: FnOnce(),
{
    create_private_directory_at_impl(parent, name, File::try_clone, after_open)
}

fn create_private_directory_at_impl<C, H>(
    parent: &File,
    name: &OsStr,
    clone_parent: C,
    after_open: H,
) -> Result<File, NativePrivateCreateError>
where
    C: FnOnce(&File) -> io::Result<File>,
    H: FnOnce(),
{
    let name = match component_name(name) {
        Ok(name) => name,
        Err(source) => return Err(pre_create_error(source)),
    };
    let retained_parent = clone_parent(parent).map_err(pre_create_error)?;
    let result = unsafe { libc::mkdirat(parent.as_raw_fd(), name.as_ptr(), 0o700) };
    if result < 0 {
        return Err(pre_create_error(io::Error::last_os_error()));
    }
    let descriptor = unsafe {
        libc::openat(
            parent.as_raw_fd(),
            name.as_ptr(),
            libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        )
    };
    if descriptor < 0 {
        let source = io::Error::last_os_error();
        return Err(create_error(
            true,
            source,
            retained_parent,
            OsStr::from_bytes(name.as_bytes()),
            None,
            None,
        ));
    }
    // SAFETY: openat returned a fresh descriptor owned by this function.
    let directory = unsafe { File::from_raw_fd(descriptor) };
    after_open();
    let metadata = match directory.metadata() {
        Ok(metadata) => metadata,
        Err(source) => {
            let identity = identity_for_file(&directory).ok();
            return Err(create_error(
                true,
                source,
                retained_parent,
                OsStr::from_bytes(name.as_bytes()),
                Some(directory),
                identity,
            ));
        }
    };
    let identity = identity_for_file(&directory).ok();
    let name_identity = match identity_for_name(parent, OsStr::from_bytes(name.as_bytes())) {
        Ok(identity) => identity,
        Err(source) => {
            return Err(create_error(
                true,
                source,
                retained_parent,
                OsStr::from_bytes(name.as_bytes()),
                Some(directory),
                identity,
            ));
        }
    };
    if identity.as_ref() != Some(&name_identity) {
        return Err(create_error(
            true,
            io::Error::new(
                io::ErrorKind::PermissionDenied,
                "directory entry changed after handle acquisition",
            ),
            retained_parent,
            OsStr::from_bytes(name.as_bytes()),
            Some(directory),
            identity,
        ));
    }
    let mode = metadata.mode() & 0o777;
    if !metadata.is_dir() || metadata.uid() != unsafe { libc::geteuid() } {
        return Err(create_error(
            true,
            io::Error::new(
                io::ErrorKind::PermissionDenied,
                "created entry is not an owned directory",
            ),
            retained_parent,
            OsStr::from_bytes(name.as_bytes()),
            Some(directory),
            identity,
        ));
    }
    if mode & 0o077 != 0 || mode & 0o700 != 0o700 {
        return Err(create_error(
            true,
            io::Error::new(
                io::ErrorKind::PermissionDenied,
                "umask prevented required private directory mode 0700",
            ),
            retained_parent,
            OsStr::from_bytes(name.as_bytes()),
            Some(directory),
            identity,
        ));
    }
    Ok(directory)
}

pub(crate) fn apply_current_user_access(file: &File, directory: bool, read_only: bool) -> io::Result<()> {
    verify_owner_and_type(file, directory)?;
    let mode = match (directory, read_only) {
        (false, false) => 0o600,
        (false, true) => 0o400,
        (true, false) => 0o700,
        (true, true) => 0o500,
    };
    #[cfg(test)]
    fail_access_if_scheduled()?;
    if unsafe { libc::fchmod(file.as_raw_fd(), mode) } < 0 {
        return Err(io::Error::last_os_error());
    }
    verify_current_user_access(file, directory, read_only)
}

pub(crate) fn verify_current_user_access(file: &File, directory: bool, read_only: bool) -> io::Result<()> {
    verify_owner_and_type(file, directory)?;
    let metadata = file.metadata()?;
    let expected = match (directory, read_only) {
        (false, false) => 0o600,
        (false, true) => 0o400,
        (true, false) => 0o700,
        (true, true) => 0o500,
    };
    if metadata.mode() & 0o777 != expected {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "handle access mode does not match required mode",
        ));
    }
    Ok(())
}

#[cfg(test)]
mod owner_policy_unix_tests {
    use super::{apply_owner_access, create_owner_entry_at, verify_owner_access};
    use crate::policy::LocalOwnerAccessPolicy;
    use std::fs::File;
    use std::os::unix::fs::MetadataExt;

    #[test]
    fn owner_policy_unix_creates_exact_modes_and_applies_read_only_policy() {
        let temp = tempfile::tempdir().expect("temporary directory");
        let parent = File::open(temp.path()).expect("open parent directory");
        let editable = LocalOwnerAccessPolicy::new().with_unix_modes(0o700, 0o600);
        let file = create_owner_entry_at(&parent, std::ffi::OsStr::new("owner-file"), false, &editable)
            .expect("create owner file");
        assert_eq!(file.metadata().expect("file metadata").mode() & 0o7777, 0o600);
        apply_owner_access(
            &file,
            false,
            &LocalOwnerAccessPolicy::new().with_unix_modes(0o500, 0o400),
        )
        .expect("apply read-only access");
        verify_owner_access(
            &file,
            false,
            &LocalOwnerAccessPolicy::new().with_unix_modes(0o500, 0o400),
        )
        .expect("verify read-only access");
        assert_eq!(file.metadata().expect("file metadata").mode() & 0o7777, 0o400);
        apply_owner_access(&file, false, &editable).expect("restore editable access");
        assert_eq!(file.metadata().expect("file metadata").mode() & 0o7777, 0o600);

        let editable_directory = create_owner_entry_at(
            &parent,
            std::ffi::OsStr::new("editable-owner-directory"),
            true,
            &editable,
        )
        .expect("create editable owner directory");
        assert_eq!(editable_directory.metadata().expect("directory metadata").mode() & 0o7777, 0o700);

        let readonly = LocalOwnerAccessPolicy::new().with_unix_modes(0o500, 0o400);
        let readonly_file = create_owner_entry_at(
            &parent,
            std::ffi::OsStr::new("readonly-owner-file"),
            false,
            &readonly,
        )
        .expect("create read-only owner file");
        assert_eq!(readonly_file.metadata().expect("file metadata").mode() & 0o7777, 0o400);
        let directory = create_owner_entry_at(
            &parent,
            std::ffi::OsStr::new("owner-directory"),
            true,
            &readonly,
        )
        .expect("create read-only owner directory");
        assert_eq!(directory.metadata().expect("directory metadata").mode() & 0o7777, 0o500);
    }

    #[test]
    fn owner_policy_unix_rejects_unconfigured_and_invalid_modes_before_mutation() {
        let temp = tempfile::tempdir().expect("temporary directory");
        let parent = File::open(temp.path()).expect("open parent directory");
        for policy in [
            LocalOwnerAccessPolicy::new(),
            LocalOwnerAccessPolicy::new().with_unix_modes(0o770, 0o600),
            LocalOwnerAccessPolicy::new().with_unix_modes(0o4700, 0o600),
        ] {
            let error = create_owner_entry_at(&parent, std::ffi::OsStr::new("invalid"), false, &policy)
                .expect_err("invalid policy must fail");
            assert_eq!(error.source.kind(), std::io::ErrorKind::InvalidInput);
            assert!(!temp.path().join("invalid").exists());
        }
    }

    #[cfg(target_os = "linux")]
    #[test]
    fn owner_policy_unix_rejects_extended_access_acl() {
        use std::ffi::CString;
        use std::os::fd::AsRawFd;

        let temp = tempfile::tempdir().expect("temporary directory");
        let parent = File::open(temp.path()).expect("open parent directory");
        let policy = LocalOwnerAccessPolicy::new().with_unix_modes(0o700, 0o600);
        let file = create_owner_entry_at(&parent, std::ffi::OsStr::new("acl-file"), false, &policy)
            .expect("create owner file");
        let acl_name = CString::new("system.posix_acl_access").expect("static xattr name");
        let acl = [
            0x02_u8, 0x00, 0x00, 0x00,
            0x01, 0x00, 0x07, 0x00, 0xff, 0xff, 0xff, 0xff,
            0x02, 0x00, 0x04, 0x00, 0xfe, 0xff, 0x00, 0x00,
            0x04, 0x00, 0x00, 0x00, 0xff, 0xff, 0xff, 0xff,
            0x10, 0x00, 0x00, 0x00, 0xff, 0xff, 0xff, 0xff,
            0x20, 0x00, 0x00, 0x00, 0xff, 0xff, 0xff, 0xff,
        ];
        let set_result = unsafe {
            libc::fsetxattr(
                file.as_raw_fd(),
                acl_name.as_ptr(),
                acl.as_ptr().cast(),
                acl.len(),
                0,
            )
        };
        if set_result < 0 {
            let error = std::io::Error::last_os_error();
            panic!("Linux test filesystem must support installing the POSIX ACL fixture: {error}");
        }
        let chmod_result = unsafe { libc::fchmod(file.as_raw_fd(), 0o600) };
        assert_eq!(chmod_result, 0, "preserve exact mode alongside ACL fixture");
        let error = verify_owner_access(&file, false, &policy).expect_err("extended ACL is rejected");
        assert_eq!(error.kind(), std::io::ErrorKind::PermissionDenied);
        let error = apply_owner_access(
            &file,
            false,
            &LocalOwnerAccessPolicy::new().with_unix_modes(0o500, 0o400),
        )
        .expect_err("extended ACL is rejected before applying a new mode");
        assert_eq!(error.kind(), std::io::ErrorKind::PermissionDenied);
        assert_eq!(file.metadata().expect("file metadata").mode() & 0o7777, 0o600);
    }
}

fn verify_owner_and_type(file: &File, directory: bool) -> io::Result<()> {
    let metadata = file.metadata()?;
    let correct_type = if directory {
        metadata.is_dir()
    } else {
        metadata.is_file()
    };
    if !correct_type || metadata.uid() != unsafe { libc::geteuid() } {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "handle is not an owned entry of the expected type",
        ));
    }
    Ok(())
}

fn component_name(name: &OsStr) -> io::Result<CString> {
    let bytes = name.as_bytes();
    if bytes.is_empty() || bytes == b"." || bytes == b".." || bytes.contains(&b'/') {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "name must be one native path component",
        ));
    }
    CString::new(bytes).map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "name contains a nul byte"))
}

fn identity_for_file(file: &File) -> io::Result<LocalFileIdentity> {
    let metadata = file.metadata()?;
    Ok(LocalFileIdentity::unix(metadata.dev(), metadata.ino()))
}

fn identity_for_name(parent: &File, name: &OsStr) -> io::Result<LocalFileIdentity> {
    let name = component_name(name)?;
    let mut status = std::mem::MaybeUninit::<libc::stat>::uninit();
    let result = unsafe {
        libc::fstatat(
            parent.as_raw_fd(),
            name.as_ptr(),
            status.as_mut_ptr(),
            libc::AT_SYMLINK_NOFOLLOW,
        )
    };
    if result < 0 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: fstatat initialized status on success.
    let status = unsafe { status.assume_init() };
    #[allow(clippy::unnecessary_cast)]
    let device = status.st_dev as u64;
    #[allow(clippy::unnecessary_cast)]
    let inode = status.st_ino as u64;
    Ok(LocalFileIdentity::unix(device, inode))
}

fn create_error(
    directory: bool,
    source: io::Error,
    parent: File,
    name: &OsStr,
    file: Option<File>,
    identity: Option<LocalFileIdentity>,
) -> NativePrivateCreateError {
    NativePrivateCreateError {
        source,
        retained: Some(NativePrivateEntry {
            parent,
            name: name.to_os_string(),
            file,
            identity,
            directory,
        }),
    }
}

fn pre_create_error(source: io::Error) -> NativePrivateCreateError {
    NativePrivateCreateError { source, retained: None }
}
