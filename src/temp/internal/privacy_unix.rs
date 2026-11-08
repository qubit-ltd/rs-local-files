// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
#[cfg(test)]
use std::cell::Cell;
#[cfg(test)]
use std::cell::RefCell;
use std::ffi::CString;
use std::ffi::OsStr;
use std::fs::File;
use std::io;
use std::os::fd::AsRawFd;
use std::os::fd::FromRawFd;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::MetadataExt;

use crate::observation::LocalFileIdentity;
use crate::policy::LocalOwnerAccessPolicy;
use crate::temp::types::NativePrivateCreateError;
use crate::temp::types::NativePrivateEntry;

#[cfg(test)]
thread_local! {
    static ACCESS_FAILURE_AFTER: Cell<Option<usize>> = const { Cell::new(None) };
    static OWNER_OPEN_FAILURE_HOOK: RefCell<Option<OwnerOpenFailureHook>> = const { RefCell::new(None) };
}

#[cfg(test)]
type OwnerOpenFailureHook = Box<dyn FnOnce(&File, &OsStr) -> io::Result<()>>;

#[cfg(all(test, target_os = "linux"))]
pub(crate) fn install_owner_open_failure_hook(hook: impl FnOnce(&File, &OsStr) -> io::Result<()> + 'static) {
    OWNER_OPEN_FAILURE_HOOK.with(|slot| {
        assert!(slot.borrow().is_none(), "owner open hook already installed");
        *slot.borrow_mut() = Some(Box::new(hook));
    });
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

/// Selects the configured owner-only mode for a file or directory.
///
/// # Parameters
///
/// `policy` supplies both modes; `directory` selects the directory or file
/// mode.
///
/// # Returns
///
/// The configured mode for the selected entry kind.
///
/// # Errors
///
/// Returns `InvalidInput` when Unix modes are absent or contain bits outside
/// `0o700`.
fn owner_mode(policy: &LocalOwnerAccessPolicy, directory: bool) -> io::Result<u32> {
    let (directory_mode, file_mode) = policy.unix_modes().ok_or_else(|| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "Unix owner access policy is not configured",
        )
    })?;
    if directory_mode & !0o700 != 0 || file_mode & !0o700 != 0 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "Unix owner modes may contain only 0o700 permission bits",
        ));
    }
    Ok(if directory { directory_mode } else { file_mode })
}

/// Converts a policy mode to the platform's native mode type.
///
/// # Parameters
///
/// `mode` is the owner permission mode to convert.
///
/// # Returns
///
/// The mode represented as `libc::mode_t`.
///
/// # Errors
///
/// Returns `InvalidInput` when the mode cannot be represented by
/// `libc::mode_t`.
fn native_mode(mode: u32) -> io::Result<libc::mode_t> {
    libc::mode_t::try_from(mode).map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "owner mode does not fit the native mode type",
        )
    })
}

/// Converts a policy mode to the integer type required by `openat`.
///
/// # Parameters
///
/// `mode` is the owner permission mode to convert.
///
/// # Returns
///
/// The mode represented as `libc::c_uint`.
///
/// # Errors
///
/// Returns `InvalidInput` when the mode cannot be represented by
/// `libc::c_uint`.
fn openat_mode(mode: u32) -> io::Result<libc::c_uint> {
    libc::c_uint::try_from(mode).map_err(|_| {
        io::Error::new(
            io::ErrorKind::InvalidInput,
            "owner mode does not fit the openat variadic argument type",
        )
    })
}

/// Verifies ownership, exact mode, and absence of additional ACL entries.
///
/// # Parameters
///
/// `file` is the opened entry; `directory` selects its expected kind; `policy`
/// supplies exact modes.
///
/// # Returns
///
/// Succeeds when the entry satisfies the exact owner policy.
///
/// # Errors
///
/// Returns an I/O error when ownership, type, exact mode, or ACL verification
/// fails.
pub(crate) fn verify_owner_access(file: &File, directory: bool, policy: &LocalOwnerAccessPolicy) -> io::Result<()> {
    let expected = owner_mode(policy, directory)?;
    ensure_acl_backend_supported()?;
    verify_owner_and_type(file, directory)?;
    let metadata = file.metadata()?;
    if metadata.mode() & 0o7777 != expected {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            "entry mode does not exactly match owner policy",
        ));
    }
    verify_no_extra_acl(file, directory)
}

/// Applies the configured owner mode after validating the entry and ACL state.
///
/// # Parameters
///
/// `file` is the opened entry; `directory` selects its expected kind; `policy`
/// supplies the target mode.
///
/// # Returns
///
/// Succeeds after the requested mode has been applied and verified.
///
/// # Errors
///
/// Returns an I/O error when validation, mode application, or post-application
/// verification fails.
pub(crate) fn apply_owner_access(file: &File, directory: bool, policy: &LocalOwnerAccessPolicy) -> io::Result<()> {
    let mode = native_mode(owner_mode(policy, directory)?)?;
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

/// Creates a file or directory beneath `parent` using the configured owner
/// mode.
///
/// # Parameters
///
/// `parent` supplies directory authority; `name` is one native path component;
/// `directory` selects the entry kind; `policy` supplies its owner mode.
///
/// # Returns
///
/// The opened entry on success.
///
/// # Errors
///
/// Returns the creation or policy error, retaining recovery context when an
/// entry may have been created.
pub(crate) fn create_owner_entry_at(
    parent: &File,
    name: &OsStr,
    directory: bool,
    policy: &LocalOwnerAccessPolicy,
) -> Result<File, NativePrivateCreateError> {
    create_owner_entry_at_with_hook(parent, name, directory, policy, || Ok(()))
}

/// Creates an owner entry while allowing the test build to inject an open
/// failure.
///
/// # Parameters
///
/// `parent` supplies directory authority; `name` is one native path component;
/// `directory` selects the entry kind; `policy` supplies its owner mode; `hook`
/// runs after directory creation and before opening it.
///
/// # Returns
///
/// The opened entry on success.
///
/// # Errors
///
/// Returns the creation or policy error, retaining recovery context when an
/// entry may have been created.
fn create_owner_entry_at_with_hook(
    parent: &File,
    name: &OsStr,
    directory: bool,
    policy: &LocalOwnerAccessPolicy,
    hook: impl FnOnce() -> io::Result<()>,
) -> Result<File, NativePrivateCreateError> {
    let mode = match owner_mode(policy, directory) {
        Ok(mode) => mode,
        Err(source) => return Err(pre_create_error(source)),
    };
    let native_mode = native_mode(mode).map_err(pre_create_error)?;
    let openat_mode = openat_mode(mode).map_err(pre_create_error)?;
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
        if unsafe { libc::mkdirat(parent.as_raw_fd(), name.as_ptr(), native_mode) } < 0 {
            return Err(pre_create_error(io::Error::last_os_error()));
        }
    } else {
        let descriptor = unsafe {
            libc::openat(
                parent.as_raw_fd(),
                name.as_ptr(),
                libc::O_RDWR | libc::O_CREAT | libc::O_EXCL | libc::O_NOFOLLOW | libc::O_CLOEXEC,
                openat_mode,
            )
        };
        if descriptor < 0 {
            return Err(pre_create_error(io::Error::last_os_error()));
        }
        // SAFETY: openat returned a fresh descriptor owned by this function.
        let file = unsafe { File::from_raw_fd(descriptor) };
        return finish_owner_creation(parent, retained_parent, name, file, false, policy);
    }
    #[cfg(test)]
    let hook_result = OWNER_OPEN_FAILURE_HOOK.with(|slot| {
        slot.borrow_mut()
            .take()
            .map_or_else(hook, |hook| hook(parent, OsStr::from_bytes(name.as_bytes())))
    });
    #[cfg(not(test))]
    let hook_result = hook();
    if let Err(source) = hook_result {
        return Err(create_error(
            true,
            source,
            retained_parent,
            OsStr::from_bytes(name.as_bytes()),
            None,
            None,
        ));
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
    let file = unsafe { File::from_raw_fd(descriptor) };
    finish_owner_creation(parent, retained_parent, name, file, true, policy)
}

/// Applies and verifies policy on a newly opened owner entry, retaining
/// recovery data on failure.
///
/// # Parameters
///
/// `parent` and `name` identify the entry; `retained_parent` preserves recovery
/// authority; `file` is the opened handle; `directory` selects the expected
/// entry kind; `policy` supplies its owner mode.
///
/// # Returns
///
/// The verified opened entry.
///
/// # Errors
///
/// Returns the policy error and retains available recovery context.
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
        let mode = native_mode(owner_mode(policy, directory)?)?;
        if unsafe { libc::fchmod(file.as_raw_fd(), mode) } < 0 {
            return Err(io::Error::last_os_error());
        }
        if directory {
            let found = identity_for_name(parent, OsStr::from_bytes(name.as_bytes()))?;
            if identity.as_ref() != Some(&found) {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "directory entry changed after handle acquisition",
                ));
            }
        }
        verify_owner_access(&file, directory, policy)
    })();
    match result {
        Ok(()) => Ok(file),
        Err(source) => Err(create_error(
            directory,
            source,
            retained_parent,
            OsStr::from_bytes(name.as_bytes()),
            Some(file),
            identity,
        )),
    }
}

/// Rejects access or default POSIX ACL entries beyond the mode bits.
///
/// # Parameters
///
/// `file` is the opened entry to inspect; `directory` determines whether its
/// default ACL is also checked.
///
/// # Returns
///
/// Succeeds when no additional ACL entries are present.
///
/// # Errors
///
/// Returns an I/O error when an extra ACL is present or ACL inspection is
/// unavailable.
fn verify_no_extra_acl(file: &File, directory: bool) -> io::Result<()> {
    #[cfg(target_os = "linux")]
    {
        let names: &[&[u8]] = if directory {
            &[b"system.posix_acl_access\0", b"system.posix_acl_default\0"]
        } else {
            &[b"system.posix_acl_access\0"]
        };
        for bytes in names {
            let name = CString::new(&bytes[..bytes.len() - 1]).expect("static xattr name");
            let result = unsafe { libc::fgetxattr(file.as_raw_fd(), name.as_ptr(), std::ptr::null_mut(), 0) };
            if result >= 0 {
                return Err(io::Error::new(
                    io::ErrorKind::PermissionDenied,
                    "entry has an extended POSIX ACL",
                ));
            }
            let error = io::Error::last_os_error();
            if error.raw_os_error() != Some(libc::ENODATA) {
                return Err(error);
            }
        }
        Ok(())
    }
    #[cfg(not(target_os = "linux"))]
    {
        let _ = (file, directory);
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "exact Unix owner ACL verification is not implemented on this platform",
        ))
    }
}

/// Checks whether this platform can verify the exact ACL policy.
///
/// # Returns
///
/// Succeeds when this platform has the required ACL verification backend.
///
/// # Errors
///
/// Returns `Unsupported` when exact ACL verification is unavailable.
fn ensure_acl_backend_supported() -> io::Result<()> {
    #[cfg(not(target_os = "linux"))]
    {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "exact Unix owner ACL verification is not implemented on this platform",
        ))
    }
    #[cfg(target_os = "linux")]
    {
        Ok(())
    }
}

/// Creates an exclusively named private regular file beneath an open directory.
///
/// # Parameters
///
/// `parent` supplies directory authority and `name` is one native path
/// component.
///
/// # Returns
///
/// The newly opened private regular file.
///
/// # Errors
///
/// Returns the creation error, retaining recovery context when creation may
/// have succeeded.
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

/// Implements private file creation with an injectable parent-handle clone
/// operation.
///
/// # Type Parameters
///
/// `F` is a one-shot operation that clones or otherwise retains the parent
/// handle.
///
/// # Parameters
///
/// `parent` supplies directory authority; `name` is one native path component;
/// `clone_parent` prepares a retained parent handle before mutation.
///
/// # Returns
///
/// The newly opened private regular file.
///
/// # Errors
///
/// Returns the creation error, retaining recovery context when creation may
/// have succeeded.
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
    #[cfg(test)]
    let metadata_result = crate::tests::native_io_fault_tests::metadata(&file);
    #[cfg(not(test))]
    let metadata_result = file.metadata();
    let metadata = match metadata_result {
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

/// Creates an exclusively named private directory beneath an open directory.
///
/// # Parameters
///
/// `parent` supplies directory authority and `name` is one native path
/// component.
///
/// # Returns
///
/// The newly opened private directory.
///
/// # Errors
///
/// Returns the creation error, retaining recovery context when creation may
/// have succeeded.
pub(crate) fn create_private_directory_at(parent: &File, name: &OsStr) -> Result<File, NativePrivateCreateError> {
    create_private_directory_at_impl(parent, name, File::try_clone, || Ok(()), || {})
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
    create_private_directory_at_impl(parent, name, clone_parent, || Ok(()), || {})
}

#[cfg(test)]
pub(crate) fn create_private_directory_at_with_open_failure_hook<F>(
    parent: &File,
    name: &OsStr,
    before_open: F,
) -> Result<File, NativePrivateCreateError>
where
    F: FnOnce() -> io::Result<()>,
{
    create_private_directory_at_impl(parent, name, File::try_clone, before_open, || {})
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
    create_private_directory_at_impl(parent, name, File::try_clone, || Ok(()), after_open)
}

/// Implements private directory creation with injectable operations for tests.
///
/// # Type Parameters
///
/// `C` prepares a retained parent handle; `O` injects a fallible operation
/// before opening; `H` injects a callback after opening.
///
/// # Parameters
///
/// `parent` supplies directory authority; `name` is one native path component;
/// `clone_parent`, `before_open`, and `after_open` provide the corresponding
/// test operations.
///
/// # Returns
///
/// The newly opened private directory.
///
/// # Errors
///
/// Returns the creation error, retaining recovery context when creation may
/// have succeeded.
fn create_private_directory_at_impl<C, O, H>(
    parent: &File,
    name: &OsStr,
    clone_parent: C,
    before_open: O,
    after_open: H,
) -> Result<File, NativePrivateCreateError>
where
    C: FnOnce(&File) -> io::Result<File>,
    O: FnOnce() -> io::Result<()>,
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
    if let Err(source) = before_open() {
        return Err(create_error(
            true,
            source,
            retained_parent,
            OsStr::from_bytes(name.as_bytes()),
            None,
            None,
        ));
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
    #[cfg(test)]
    let metadata_result = crate::tests::native_io_fault_tests::metadata(&directory);
    #[cfg(not(test))]
    let metadata_result = directory.metadata();
    let metadata = match metadata_result {
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

/// Applies the standard writable or read-only mode for the current user.
///
/// # Parameters
///
/// `file` is the opened entry; `directory` selects its expected kind;
/// `read_only` selects owner read-only or writable access.
///
/// # Returns
///
/// Succeeds after the requested mode has been applied and verified.
///
/// # Errors
///
/// Returns an I/O error when ownership, type, mode application, or verification
/// fails.
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

/// Verifies ownership, entry type, and the standard current-user access mode.
///
/// # Parameters
///
/// `file` is the opened entry; `directory` selects its expected kind;
/// `read_only` selects the expected owner mode.
///
/// # Returns
///
/// Succeeds when the entry has the requested current-user mode.
///
/// # Errors
///
/// Returns an I/O error when ownership, type, or mode does not match the
/// requested access.
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

/// Verifies that `file` refers to an entry owned by the current user of the
/// expected type.
///
/// # Parameters
///
/// `file` is the opened entry; `directory` selects the expected entry kind.
///
/// # Returns
///
/// Succeeds when metadata confirms the expected owner and entry kind.
///
/// # Errors
///
/// Returns an I/O error when metadata cannot be read or ownership and type do
/// not match.
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

/// Converts one valid native path component to a NUL-terminated string.
///
/// # Parameters
///
/// `name` is the native component to validate and convert.
///
/// # Returns
///
/// The NUL-terminated component.
///
/// # Errors
///
/// Returns `InvalidInput` when the component is empty, dot, dot-dot, contains a
/// separator, or contains NUL.
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

/// Reads the stable device and inode identity from an open file handle.
///
/// # Parameters
///
/// `file` is the open handle whose identity is read.
///
/// # Returns
///
/// The device and inode identity.
///
/// # Errors
///
/// Returns an I/O error if handle metadata cannot be read.
fn identity_for_file(file: &File) -> io::Result<LocalFileIdentity> {
    let metadata = file.metadata()?;
    Ok(LocalFileIdentity::unix(metadata.dev(), metadata.ino()))
}

/// Reads the no-follow identity of a named entry beneath `parent`.
///
/// # Parameters
///
/// `parent` supplies directory authority and `name` is one native path
/// component.
///
/// # Returns
///
/// The named entry's device and inode identity without following a symbolic
/// link.
///
/// # Errors
///
/// Returns an I/O error if the name is invalid or the entry cannot be
/// inspected.
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

/// Builds a creation error that retains the entry context available for
/// recovery.
///
/// # Parameters
///
/// `directory`, `parent`, and `name` describe the created entry; `source` is
/// the failure; `file` and `identity` carry any proven handle and identity.
///
/// # Returns
///
/// A creation error containing the supplied failure and retained recovery
/// context.
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
            parent: std::sync::Arc::new(parent),
            name: name.to_os_string(),
            file: file.map(std::sync::Arc::new),
            identity,
            directory,
        }),
    }
}

/// Builds an error for a failure that occurred before an entry was created.
///
/// # Parameters
///
/// `source` is the failure that prevented entry creation.
///
/// # Returns
///
/// A creation error without retained entry context.
fn pre_create_error(source: io::Error) -> NativePrivateCreateError {
    NativePrivateCreateError { source, retained: None }
}

#[cfg(test)]
mod helper_validation_tests {
    use std::ffi::OsStr;

    use super::component_name;
    use super::native_mode;
    use super::openat_mode;
    use super::owner_mode;
    use crate::policy::LocalOwnerAccessPolicy;

    #[test]
    fn test_unix_private_entry_helpers_validate_names_and_modes() {
        let policy = LocalOwnerAccessPolicy::new().with_unix_modes(0o700, 0o400);
        assert_eq!(owner_mode(&policy, true).expect("directory mode"), 0o700);
        assert_eq!(owner_mode(&policy, false).expect("file mode"), 0o400);
        let unconfigured = LocalOwnerAccessPolicy::new();
        assert!(owner_mode(&unconfigured, true).is_err());

        assert_eq!(native_mode(0o600).expect("native mode"), 0o600);
        assert_eq!(openat_mode(0o600).expect("openat mode"), 0o600);
        assert_eq!(component_name(OsStr::new("entry")).expect("component"), c"entry");
        for invalid in [
            OsStr::new(""),
            OsStr::new("."),
            OsStr::new(".."),
            OsStr::new("a/b"),
            OsStr::new("a\0b"),
        ] {
            assert!(component_name(invalid).is_err(), "invalid component {invalid:?}");
        }
    }
}
