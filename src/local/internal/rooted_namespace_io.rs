// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Unix descriptor-relative rooted namespace operations.
// qubit-style: allow source-test-pair
// Private behavior is covered through public integration tests.

use std::ffi::CString;
use std::ffi::OsString;
use std::fs::File;
use std::io::Error;
use std::io::ErrorKind;
use std::io::Result;
use std::os::fd::AsRawFd;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::ffi::OsStringExt;
use std::path::Path;
use std::path::PathBuf;

use rustix::fs::Dir;
use rustix::fs::readlinkat;
use rustix::fs::symlinkat;

use super::path_operations::add_path_context;
use super::rooted_directory_reader::RootedDirectoryReader;
use super::rooted_file_io::open_rooted_parent;
use super::rooted_parent_mode::RootedParentMode;
use crate::LocalRelativePath;

/// A native child name and its no-follow metadata.
pub(crate) type RootedDirectoryEntry = (OsString, libc::stat);

impl RootedDirectoryReader {
    /// Opens a reader over an already-authorized directory descriptor.
    ///
    /// Returns an I/O error when the descriptor cannot be duplicated for
    /// enumeration.
    fn open(directory: File, diagnostic_path: &Path) -> Result<Self> {
        let stream = Dir::read_from(&directory)?;
        Ok(Self {
            directory,
            stream,
            diagnostic_path: diagnostic_path.to_path_buf(),
        })
    }

    /// Reads the next child without following its final symbolic link.
    ///
    /// Returns `Ok(None)` after the directory is exhausted, and returns an I/O
    /// error when enumeration or no-follow metadata inspection fails.
    pub(crate) fn next_entry(&mut self) -> Result<Option<RootedDirectoryEntry>> {
        loop {
            let entry = match self.stream.next() {
                Some(entry) => entry?,
                None => return Ok(None),
            };
            let name = entry.file_name();
            let name = name.to_bytes();
            if name == b"." || name == b".." {
                continue;
            }
            let name = OsString::from_vec(name.to_vec());
            let c_name = CString::new(name.as_bytes()).expect("directory entry names never contain NUL");
            let status = stat_child(&self.directory, &c_name, &self.diagnostic_path)?;
            return Ok(Some((name, status)));
        }
    }
}

/// Opens a lazy reader over the opened root directory.
///
/// Returns an I/O error when the root descriptor cannot be enumerated.
pub(crate) fn open_root_directory_reader(root: &File, diagnostic_root: &Path) -> Result<RootedDirectoryReader> {
    RootedDirectoryReader::open(root.try_clone()?, diagnostic_root)
}

/// Opens a lazy reader over one rooted descendant directory.
///
/// Returns an I/O error when secure traversal or directory opening fails.
pub(crate) fn open_rooted_directory_reader(
    root: &File,
    diagnostic_root: &Path,
    path: &LocalRelativePath,
) -> Result<RootedDirectoryReader> {
    let diagnostic_path = diagnostic_root.join(path.as_path());
    let (parent, name, _) =
        open_rooted_parent(root, &diagnostic_path, path, RootedParentMode::OpenExisting)?.into_parts();
    let directory = match open_directory_component(&parent, &name) {
        Ok(directory) => directory,
        Err(error) => {
            return Err(add_path_context(
                error,
                "open rooted directory for listing",
                &diagnostic_path,
            ));
        }
    };
    RootedDirectoryReader::open(directory, &diagnostic_path)
}

/// Reads immediate entries from the opened root directory.
#[inline(always)]
pub(crate) fn read_root_directory(root: &File, diagnostic_root: &Path) -> Result<Vec<RootedDirectoryEntry>> {
    read_directory_handle(root, diagnostic_root)
}

/// Reads immediate entries from a rooted descendant directory.
pub(crate) fn read_rooted_directory(
    root: &File,
    diagnostic_root: &Path,
    path: &LocalRelativePath,
) -> Result<Vec<RootedDirectoryEntry>> {
    let diagnostic_path = diagnostic_root.join(path.as_path());
    let (parent, name, _) =
        open_rooted_parent(root, &diagnostic_path, path, RootedParentMode::OpenExisting)?.into_parts();
    let directory = open_directory_component(&parent, &name)
        .map_err(|error| add_path_context(error, "open rooted directory for listing", &diagnostic_path))?;
    read_directory_handle(&directory, &diagnostic_path)
}

/// Reads one final symbolic-link target through its opened parent authority.
pub(crate) fn read_rooted_link(root: &File, diagnostic_root: &Path, path: &LocalRelativePath) -> Result<PathBuf> {
    let diagnostic_path = diagnostic_root.join(path.as_path());
    let (parent, name, _) =
        open_rooted_parent(root, &diagnostic_path, path, RootedParentMode::OpenExisting)?.into_parts();
    match readlinkat(&parent, &name, Vec::new()) {
        Ok(target) => Ok(PathBuf::from(OsString::from_vec(target.into_bytes()))),
        Err(error) => Err(add_path_context(
            Error::from(error),
            "read rooted symbolic link",
            &diagnostic_path,
        )),
    }
}

/// Creates one final symbolic link through its opened parent authority.
pub(crate) fn create_rooted_symlink(
    root: &File,
    diagnostic_root: &Path,
    target: &Path,
    path: &LocalRelativePath,
) -> Result<()> {
    let diagnostic_path = diagnostic_root.join(path.as_path());
    let (parent, name, _) =
        open_rooted_parent(root, &diagnostic_path, path, RootedParentMode::OpenExisting)?.into_parts();
    match symlinkat(target, &parent, &name) {
        Ok(()) => Ok(()),
        Err(error) => Err(add_path_context(
            Error::from(error),
            "create rooted symbolic link",
            &diagnostic_path,
        )),
    }
}

/// Creates one rooted directory, optionally creating missing parents.
pub(crate) fn create_rooted_directory(
    root: &File,
    diagnostic_root: &Path,
    path: &LocalRelativePath,
    recursive: bool,
    exists_ok: bool,
) -> Result<()> {
    let diagnostic_path = diagnostic_root.join(path.as_path());
    let mode = if recursive {
        RootedParentMode::CreateMissing
    } else {
        RootedParentMode::OpenExisting
    };
    let (parent, name, _) = open_rooted_parent(root, &diagnostic_path, path, mode)?.into_parts();
    // SAFETY: `parent` and `name` remain live for this non-retaining call.
    let result = unsafe { libc::mkdirat(parent.as_raw_fd(), name.as_ptr(), 0o700) };
    if result == 0 {
        return Ok(());
    }
    let error = Error::last_os_error();
    if error.kind() == ErrorKind::AlreadyExists && exists_ok {
        let status = stat_child(&parent, &name, &diagnostic_path)?;
        if is_directory(status.st_mode as libc::mode_t) {
            return Ok(());
        }
    }
    Err(add_path_context(error, "create rooted directory", &diagnostic_path))
}

/// Removes one rooted entry without following symbolic links.
pub(crate) fn remove_rooted_entry(
    root: &File,
    diagnostic_root: &Path,
    path: &LocalRelativePath,
    recursive: bool,
) -> Result<()> {
    let status = rooted_status(root, diagnostic_root, path)?;
    if !is_directory(status.st_mode as libc::mode_t) || !recursive {
        return unlink_rooted_entry(
            root,
            diagnostic_root,
            path,
            is_directory(status.st_mode as libc::mode_t),
        );
    }

    let mut work = vec![(path.clone(), false)];
    while let Some((current, remove_directory)) = work.pop() {
        if remove_directory {
            unlink_rooted_entry(root, diagnostic_root, &current, true)?;
            continue;
        }
        let status = rooted_status(root, diagnostic_root, &current)?;
        if !is_directory(status.st_mode as libc::mode_t) {
            unlink_rooted_entry(root, diagnostic_root, &current, false)?;
            continue;
        }
        work.push((current.clone(), true));
        for (name, _) in read_rooted_directory(root, diagnostic_root, &current)?
            .into_iter()
            .rev()
        {
            let child = LocalRelativePath::new(current.as_path().join(name))
                .expect("joining validated rooted components stays valid");
            work.push((child, false));
        }
    }
    Ok(())
}

/// Removes one rooted entry whose observed type is already known.
///
/// # Errors
///
/// Returns an I/O error when the parent cannot be opened securely or the
/// entry cannot be removed.
fn unlink_rooted_entry(root: &File, diagnostic_root: &Path, path: &LocalRelativePath, directory: bool) -> Result<()> {
    let diagnostic_path = diagnostic_root.join(path.as_path());
    let (parent, name, _) =
        open_rooted_parent(root, &diagnostic_path, path, RootedParentMode::OpenExisting)?.into_parts();
    let flags = if directory { libc::AT_REMOVEDIR } else { 0 };
    // SAFETY: `parent` and `name` remain live for this non-retaining call.
    let result = unsafe { libc::unlinkat(parent.as_raw_fd(), name.as_ptr(), flags) };
    if result == 0 {
        Ok(())
    } else {
        Err(add_path_context(
            Error::last_os_error(),
            "remove rooted entry",
            &diagnostic_path,
        ))
    }
}

/// Renames one rooted entry without abandoning either parent descriptor.
pub(crate) fn rename_rooted_entry(
    root: &File,
    diagnostic_root: &Path,
    source: &LocalRelativePath,
    destination: &LocalRelativePath,
    overwrite: bool,
) -> Result<()> {
    let source_path = diagnostic_root.join(source.as_path());
    let destination_path = diagnostic_root.join(destination.as_path());
    let (source_parent, source_name, _) =
        open_rooted_parent(root, &source_path, source, RootedParentMode::OpenExisting)?.into_parts();
    let (destination_parent, destination_name, _) =
        open_rooted_parent(root, &destination_path, destination, RootedParentMode::OpenExisting)?.into_parts();
    #[cfg(feature = "internal-test-support")]
    if super::test_support::is_enabled("rooted-rename-indeterminate") {
        return Err(add_path_context(
            crate::local::test_fault_error(),
            "rename rooted entry",
            &destination_path,
        ));
    }
    let result = if overwrite {
        // SAFETY: both parent descriptors and names remain live for this
        // non-retaining call.
        unsafe {
            libc::renameat(
                source_parent.as_raw_fd(),
                source_name.as_ptr(),
                destination_parent.as_raw_fd(),
                destination_name.as_ptr(),
            )
        }
    } else {
        rename_without_replacing(&source_parent, &source_name, &destination_parent, &destination_name)?
    };
    if result == 0 {
        Ok(())
    } else {
        Err(add_path_context(
            Error::last_os_error(),
            "rename rooted entry",
            &destination_path,
        ))
    }
}

/// Applies portable permission bits to one rooted file or directory.
pub(crate) fn set_rooted_permissions(
    root: &File,
    diagnostic_root: &Path,
    path: &LocalRelativePath,
    mode: u32,
) -> Result<()> {
    let diagnostic_path = diagnostic_root.join(path.as_path());
    let (parent, name, _) =
        open_rooted_parent(root, &diagnostic_path, path, RootedParentMode::OpenExisting)?.into_parts();
    let status = stat_child(&parent, &name, &diagnostic_path)?;
    let flags = if is_directory(status.st_mode as libc::mode_t) {
        libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC
    } else {
        libc::O_RDONLY | libc::O_NONBLOCK | libc::O_NOFOLLOW | libc::O_CLOEXEC
    };
    let entry = match super::rooted_file_io::open_file_at(&parent, &name, flags, 0) {
        Ok(entry) => entry,
        Err(error) => {
            return Err(add_path_context(
                error,
                "open rooted entry for permission update",
                &diagnostic_path,
            ));
        }
    };
    let native_mode = libc::mode_t::try_from(mode & 0o7777).expect("portable permission bits fit native mode");
    // SAFETY: `entry` owns a valid descriptor for this non-retaining call.
    let result = unsafe { libc::fchmod(entry.as_raw_fd(), native_mode) };
    if result == 0 {
        Ok(())
    } else {
        Err(add_path_context(
            Error::last_os_error(),
            "set rooted entry permissions",
            &diagnostic_path,
        ))
    }
}

/// Reads a directory through an already-open descriptor.
fn read_directory_handle(directory: &File, diagnostic_path: &Path) -> Result<Vec<RootedDirectoryEntry>> {
    let mut stream = Dir::read_from(directory).map_err(Error::from)?;
    let mut entries = Vec::new();
    for entry in &mut stream {
        let entry = entry.map_err(Error::from)?;
        let name = entry.file_name().to_bytes();
        if name == b"." || name == b".." {
            continue;
        }
        let name = OsString::from_vec(name.to_vec());
        let c_name = CString::new(name.as_bytes()).expect("directory entry names never contain NUL");
        let status = stat_child(directory, &c_name, diagnostic_path)?;
        entries.push((name, status));
    }
    entries.sort_unstable_by(|(left_name, _), (right_name, _)| left_name.cmp(right_name));
    Ok(entries)
}

/// Opens a no-follow child directory from an already-open parent.
#[inline]
fn open_directory_component(parent: &File, name: &CString) -> Result<File> {
    super::rooted_file_io::open_file_at(
        parent,
        name,
        libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        0,
    )
}

/// Reads no-follow metadata for one child of an open directory.
fn stat_child(parent: &File, name: &CString, diagnostic_path: &Path) -> Result<libc::stat> {
    let mut status = std::mem::MaybeUninit::<libc::stat>::uninit();
    // SAFETY: the output storage, descriptor, and name remain valid for this
    // non-retaining call.
    let result = unsafe {
        libc::fstatat(
            parent.as_raw_fd(),
            name.as_ptr(),
            status.as_mut_ptr(),
            libc::AT_SYMLINK_NOFOLLOW,
        )
    };
    if result == -1 {
        return Err(add_path_context(
            Error::last_os_error(),
            "inspect rooted directory entry",
            diagnostic_path,
        ));
    }
    // SAFETY: successful `fstatat` initialized the complete value.
    Ok(unsafe { status.assume_init() })
}

/// Reads no-follow metadata for a rooted path.
#[inline]
fn rooted_status(root: &File, diagnostic_root: &Path, path: &LocalRelativePath) -> Result<libc::stat> {
    super::rooted_file_io::read_rooted_symlink_metadata(root, diagnostic_root, path)
}

/// Returns whether one native mode represents a directory.
#[inline(always)]
const fn is_directory(mode: libc::mode_t) -> bool {
    mode & libc::S_IFMT == libc::S_IFDIR
}

/// Performs an atomic no-replace rename where the platform supports it.
#[cfg(any(target_os = "linux", target_os = "android"))]
#[inline]
fn rename_without_replacing(
    source_parent: &File,
    source_name: &CString,
    destination_parent: &File,
    destination_name: &CString,
) -> Result<libc::c_int> {
    // SAFETY: both parent descriptors and names remain live for this
    // non-retaining call.
    Ok(unsafe {
        libc::renameat2(
            source_parent.as_raw_fd(),
            source_name.as_ptr(),
            destination_parent.as_raw_fd(),
            destination_name.as_ptr(),
            libc::RENAME_NOREPLACE as _,
        )
    })
}

/// Performs an atomic no-replace rename on Apple platforms.
#[cfg(any(target_os = "macos", target_os = "ios"))]
#[inline]
fn rename_without_replacing(
    source_parent: &File,
    source_name: &CString,
    destination_parent: &File,
    destination_name: &CString,
) -> Result<libc::c_int> {
    // SAFETY: both parent descriptors and names remain live for this
    // non-retaining call.
    Ok(unsafe {
        libc::renameatx_np(
            source_parent.as_raw_fd(),
            source_name.as_ptr(),
            destination_parent.as_raw_fd(),
            destination_name.as_ptr(),
            libc::RENAME_EXCL,
        )
    })
}

/// Reports platforms without an atomic descriptor-relative no-replace rename.
#[cfg(not(any(target_os = "linux", target_os = "android", target_os = "macos", target_os = "ios",)))]
#[inline]
fn rename_without_replacing(
    _source_parent: &File,
    _source_name: &CString,
    _destination_parent: &File,
    _destination_name: &CString,
) -> Result<libc::c_int> {
    Err(Error::new(
        ErrorKind::Unsupported,
        "atomic rooted no-replace rename is unsupported on this platform",
    ))
}
