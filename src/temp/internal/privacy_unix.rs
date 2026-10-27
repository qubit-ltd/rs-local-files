use std::ffi::{CString, OsStr};
use std::fs::File;
use std::io;
use std::os::fd::{AsRawFd, FromRawFd};
use std::os::unix::ffi::OsStrExt;
use std::os::unix::fs::MetadataExt;

#[cfg(test)]
use std::cell::Cell;

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
use crate::temp::types::{NativePrivateCreateError, NativePrivateEntry};

pub(crate) fn create_private_file_at(
    parent: &File,
    name: &OsStr,
) -> Result<File, NativePrivateCreateError> {
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

pub(crate) fn create_private_directory_at(
    parent: &File,
    name: &OsStr,
) -> Result<File, NativePrivateCreateError> {
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

pub(crate) fn apply_current_user_access(
    file: &File,
    directory: bool,
    read_only: bool,
) -> io::Result<()> {
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

pub(crate) fn verify_current_user_access(
    file: &File,
    directory: bool,
    read_only: bool,
) -> io::Result<()> {
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
    CString::new(bytes)
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "name contains a nul byte"))
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
    Ok(LocalFileIdentity::unix(
        status.st_dev as u64,
        status.st_ino as u64,
    ))
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
    NativePrivateCreateError {
        source,
        retained: None,
    }
}
