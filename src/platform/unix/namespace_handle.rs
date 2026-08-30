// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Unix directory-descriptor namespace authority.

use std::ffi::CStr;
use std::ffi::CString;
use std::ffi::OsStr;
use std::ffi::OsString;
use std::fs::File;
use std::fs::OpenOptions;
use std::io;
use std::io::ErrorKind;
use std::mem::MaybeUninit;
use std::ops::BitAnd;
use std::os::fd::AsRawFd;
use std::os::fd::FromRawFd;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::ffi::OsStringExt;
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;
use std::time::Duration;
use std::time::SystemTime;
use std::time::UNIX_EPOCH;

use super::DirectoryCursor;
use super::EntryIdentity;
use super::OpenedFile;
use super::StagedFile;
use super::filesystem_probe;
use crate::LocalFileError;
use crate::LocalFileErrorKind;
use crate::LocalFileKind;
use crate::LocalFileMetadata;
use crate::LocalFileOperation;
use crate::LocalFileSystemLimits;
use crate::LocalFileSystemSpace;
use crate::LocalResult;
use crate::RelativePath;

/// An opened Unix directory descriptor authorizing relative operations.
#[derive(Debug)]
#[must_use]
pub(crate) struct NamespaceHandle {
    /// Directory descriptor used as the sole namespace authority.
    pub(super) descriptor: File,
}

impl NamespaceHandle {
    /// Opens a root directory without following its final component.
    ///
    /// # Parameters
    ///
    /// - `path`: Native directory path to anchor.
    ///
    /// # Errors
    ///
    /// Returns an open-root error when the path is missing, linked, not a
    /// directory, or cannot be opened by the process.
    pub(crate) fn open_root(path: &Path) -> LocalResult<Self> {
        let descriptor = OpenOptions::new()
            .read(true)
            .custom_flags(libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC)
            .open(path)
            .map_err(|error| io_error(LocalFileOperation::OpenRoot, path, None, error))?;
        Ok(Self { descriptor })
    }

    /// Duplicates this authority without resolving its original path again.
    ///
    /// # Errors
    ///
    /// Returns an open-root error when the descriptor cannot be duplicated.
    #[allow(dead_code)]
    pub(crate) fn clone_handle(&self) -> LocalResult<Self> {
        self.descriptor
            .try_clone()
            .map(|descriptor| Self { descriptor })
            .map_err(|error| io_error(LocalFileOperation::OpenRoot, Path::new(""), None, error))
    }

    /// Reads no-follow metadata for a validated relative path.
    ///
    /// The empty path returns metadata for the opened authority itself.
    ///
    /// # Errors
    ///
    /// Returns a metadata error when the entry cannot be inspected through
    /// this authority.
    pub(crate) fn metadata(&self, path: &RelativePath) -> LocalResult<LocalFileMetadata> {
        if path.as_path().as_os_str().is_empty() {
            return self
                .descriptor
                .metadata()
                .map(|metadata| LocalFileMetadata::from_native(&metadata))
                .map_err(|error| io_error(LocalFileOperation::Metadata, path.as_path(), None, error));
        }
        status_at(self, path, LocalFileOperation::Metadata).map(|status| metadata_from_stat(&status))
    }

    /// Reads the target stored in a final symbolic-link entry.
    ///
    /// # Errors
    ///
    /// Returns a bind-path error when parent traversal or `readlinkat` fails.
    pub(crate) fn read_link(&self, path: &RelativePath) -> LocalResult<std::path::PathBuf> {
        let (parent, name) = open_parent(self, path, LocalFileOperation::BindPath)?;
        let mut buffer = vec![0_u8; 256];
        loop {
            // SAFETY: `parent` and `name` remain live, while `buffer` exposes
            // writable storage of exactly the length passed to `readlinkat`.
            let length = unsafe {
                libc::readlinkat(
                    parent.as_raw_fd(),
                    name.as_ptr(),
                    buffer.as_mut_ptr().cast(),
                    buffer.len(),
                )
            };
            if length == -1 {
                return Err(io_error(
                    LocalFileOperation::BindPath,
                    path.as_path(),
                    None,
                    io::Error::last_os_error(),
                ));
            }
            let length = usize::try_from(length).map_err(|_| {
                io_error(
                    LocalFileOperation::BindPath,
                    path.as_path(),
                    None,
                    io::Error::other("negative symbolic-link target length"),
                )
            })?;
            if length < buffer.len() {
                buffer.truncate(length);
                return Ok(std::path::PathBuf::from(OsString::from_vec(buffer)));
            }
            buffer.resize(buffer.len().saturating_mul(2), 0);
        }
    }

    /// Opens a regular file without following its final component.
    ///
    /// Metadata and identity are captured from the opened descriptor, so a
    /// concurrent path replacement cannot split the observations.
    ///
    /// # Errors
    ///
    /// Returns an open-reader error when traversal fails, the final entry is
    /// not a regular file, or descriptor flags cannot be restored to blocking
    /// mode.
    pub(crate) fn open_reader(&self, path: &RelativePath) -> LocalResult<OpenedFile> {
        let (parent, name) = open_parent(self, path, LocalFileOperation::OpenReader)?;
        let file = open_file_at(
            &parent,
            &name,
            libc::O_RDONLY | libc::O_NONBLOCK | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            0,
        )
        .map_err(|error| io_error(LocalFileOperation::OpenReader, path.as_path(), None, error))?;
        let metadata = file
            .metadata()
            .map_err(|error| io_error(LocalFileOperation::OpenReader, path.as_path(), None, error))?;
        if !metadata.is_file() {
            return Err(type_error(
                LocalFileOperation::OpenReader,
                path.as_path(),
                "regular file",
            ));
        }
        clear_nonblocking(&file)
            .map_err(|error| io_error(LocalFileOperation::OpenReader, path.as_path(), None, error))?;
        let identity = EntryIdentity::from_metadata(&metadata);
        Ok(OpenedFile::new(
            file,
            LocalFileMetadata::from_native(&metadata),
            identity,
        ))
    }

    /// Opens a lazy cursor for a relative directory.
    ///
    /// The empty path enumerates the opened authority root.
    ///
    /// # Errors
    ///
    /// Returns a list error when traversal, directory opening, or native
    /// stream creation fails.
    pub(crate) fn open_directory(&self, path: &RelativePath) -> LocalResult<DirectoryCursor> {
        let directory = if path.as_path().as_os_str().is_empty() {
            self.descriptor
                .try_clone()
                .map_err(|error| io_error(LocalFileOperation::List, path.as_path(), None, error))?
        } else {
            let (parent, name) = open_parent(self, path, LocalFileOperation::List)?;
            open_directory_at(&parent, &name)
                .map_err(|error| io_error(LocalFileOperation::List, path.as_path(), None, error))?
        };
        DirectoryCursor::open(directory, path.as_path().to_path_buf())
    }

    /// Creates a directory and accepts an already-existing directory.
    ///
    /// Parent components must already exist and are traversed without
    /// following symbolic links.
    ///
    /// # Errors
    ///
    /// Returns a create-directory error when traversal or creation fails, or
    /// when the existing entry is not a directory.
    pub(crate) fn create_directory(&self, path: &RelativePath) -> LocalResult<()> {
        self.create_directory_impl(path, true)
    }

    /// Creates a directory only when the final entry does not exist.
    ///
    /// Parent components must already exist and are traversed without
    /// following symbolic links.
    ///
    /// # Errors
    ///
    /// Returns a create-directory error when traversal or creation fails,
    /// including an already-existing final entry.
    pub(crate) fn create_directory_new(&self, path: &RelativePath) -> LocalResult<()> {
        self.create_directory_impl(path, false)
    }

    /// Creates and opens a new private regular file.
    ///
    /// # Errors
    ///
    /// Returns an open-writer error when the parent cannot be traversed or the
    /// final entry already exists or cannot be created.
    #[allow(dead_code)]
    pub(crate) fn create_file_new(&self, path: &RelativePath) -> LocalResult<File> {
        let (parent, name) = open_parent(self, path, LocalFileOperation::OpenWriter)?;
        open_file_at(
            &parent,
            &name,
            libc::O_RDWR | libc::O_CREAT | libc::O_EXCL | libc::O_NOFOLLOW | libc::O_CLOEXEC,
            0o600,
        )
        .map_err(|error| io_error(LocalFileOperation::OpenWriter, path.as_path(), None, error))
    }

    /// Deletes a file or non-directory final entry without following it.
    ///
    /// # Errors
    ///
    /// Returns a delete-file error when traversal or `unlinkat` fails.
    pub(crate) fn delete_file(&self, path: &RelativePath) -> LocalResult<()> {
        unlink(self, path, false, LocalFileOperation::DeleteFile)
    }

    /// Deletes an empty directory without following its final component.
    ///
    /// # Errors
    ///
    /// Returns a delete-directory error when traversal or `unlinkat` fails.
    pub(crate) fn delete_directory(&self, path: &RelativePath) -> LocalResult<()> {
        unlink(self, path, true, LocalFileOperation::DeleteDirectory)
    }

    /// Renames one relative entry within this authority.
    ///
    /// # Parameters
    ///
    /// - `source`: Existing source entry.
    /// - `target`: Destination entry.
    /// - `overwrite`: Whether an existing destination may be replaced.
    ///
    /// # Errors
    ///
    /// Returns a rename error when either parent cannot be traversed or the
    /// native atomic rename fails. Targets without atomic no-replace support
    /// return an unsupported error when `overwrite` is `false`.
    pub(crate) fn rename(&self, source: &RelativePath, target: &RelativePath, overwrite: bool) -> LocalResult<()> {
        self.rename_to(source, self, target, overwrite)
    }

    /// Renames an entry between two opened namespace handles.
    ///
    /// # Errors
    ///
    /// Returns a rename error when either parent traversal or the native
    /// atomic rename fails, including a cross-filesystem move.
    pub(crate) fn rename_to(
        &self,
        source: &RelativePath,
        target_namespace: &Self,
        target: &RelativePath,
        overwrite: bool,
    ) -> LocalResult<()> {
        let (source_parent, source_name) = open_parent(self, source, LocalFileOperation::Rename)?;
        let (target_parent, target_name) = open_parent(target_namespace, target, LocalFileOperation::Rename)?;
        let result = if overwrite {
            // SAFETY: both parent descriptors and component strings remain
            // live for this non-retaining descriptor-relative rename.
            unsafe {
                libc::renameat(
                    source_parent.as_raw_fd(),
                    source_name.as_ptr(),
                    target_parent.as_raw_fd(),
                    target_name.as_ptr(),
                )
            }
        } else {
            rename_no_replace(&source_parent, &source_name, &target_parent, &target_name)?
        };
        if result == 0 {
            Ok(())
        } else {
            Err(io_error(
                LocalFileOperation::Rename,
                source.as_path(),
                Some(target.as_path()),
                io::Error::last_os_error(),
            ))
        }
    }

    /// Creates a private staging file beside `target`.
    ///
    /// The returned guard owns descriptor-relative cleanup authority. The
    /// target entry itself is not inspected or modified.
    ///
    /// # Errors
    ///
    /// Returns an open-writer error when the target parent cannot be traversed,
    /// secure random bytes cannot be generated, or a unique private entry
    /// cannot be created.
    #[allow(dead_code)]
    pub(crate) fn create_staged_file(&self, target: &RelativePath) -> LocalResult<StagedFile> {
        let (parent, _) = open_parent(self, target, LocalFileOperation::OpenWriter)?;
        StagedFile::create(parent, target.as_path())
    }

    /// Reads the native identity of one entry without following it.
    ///
    /// The empty path returns identity for the authority root.
    ///
    /// # Errors
    ///
    /// Returns a metadata error when the entry cannot be inspected.
    pub(crate) fn entry_identity(&self, path: &RelativePath) -> LocalResult<EntryIdentity> {
        if path.as_path().as_os_str().is_empty() {
            return self
                .descriptor
                .metadata()
                .map(|metadata| EntryIdentity::from_metadata(&metadata))
                .map_err(|error| io_error(LocalFileOperation::Metadata, path.as_path(), None, error));
        }
        status_at(self, path, LocalFileOperation::Metadata).map(|status| EntryIdentity::from_stat(&status))
    }

    /// Synchronizes the directory containing `path`.
    ///
    /// The empty path synchronizes the authority root itself.
    ///
    /// # Errors
    ///
    /// Returns a commit error when the parent cannot be opened or synchronized.
    pub(crate) fn sync_parent(&self, path: &RelativePath) -> LocalResult<()> {
        let parent = if path.as_path().as_os_str().is_empty() {
            self.descriptor
                .try_clone()
                .map_err(|error| io_error(LocalFileOperation::Commit, path.as_path(), None, error))?
        } else {
            let (parent, _) = open_parent(self, path, LocalFileOperation::Commit)?;
            parent
        };
        parent
            .sync_all()
            .map_err(|error| io_error(LocalFileOperation::Commit, path.as_path(), None, error))
    }

    /// Reads best-effort path limits for the filesystem containing `path`.
    ///
    /// The nearest securely opened parent descriptor is sufficient because
    /// path limits are filesystem properties.
    ///
    /// # Errors
    ///
    /// Returns a metadata error when the nearest parent cannot be traversed.
    pub(crate) fn filesystem_limits(&self, path: &RelativePath) -> LocalResult<LocalFileSystemLimits> {
        self.probe_handle(path).map(|file| filesystem_probe::limits(&file))
    }

    /// Reads best-effort capacity values for the filesystem containing `path`.
    ///
    /// # Errors
    ///
    /// Returns a metadata error when the nearest parent cannot be traversed.
    pub(crate) fn filesystem_space(&self, path: &RelativePath) -> LocalResult<LocalFileSystemSpace> {
        self.probe_handle(path).map(|file| filesystem_probe::space(&file))
    }

    /// Implements exclusive or idempotent directory creation.
    fn create_directory_impl(&self, path: &RelativePath, exists_ok: bool) -> LocalResult<()> {
        let (parent, name) = open_parent(self, path, LocalFileOperation::CreateDirectory)?;
        // SAFETY: the parent descriptor and NUL-terminated child name remain
        // live for this non-retaining creation call.
        let result = unsafe { libc::mkdirat(parent.as_raw_fd(), name.as_ptr(), 0o700) };
        if result == 0 {
            return Ok(());
        }
        let error = io::Error::last_os_error();
        if exists_ok && error.kind() == ErrorKind::AlreadyExists {
            let status = stat_child(&parent, &name, LocalFileOperation::CreateDirectory, path.as_path())?;
            if kind_from_mode(status.st_mode) == LocalFileKind::Directory {
                return Ok(());
            }
        }
        Err(io_error(
            LocalFileOperation::CreateDirectory,
            path.as_path(),
            None,
            error,
        ))
    }

    /// Opens a descriptor suitable for filesystem-level probes.
    fn probe_handle(&self, path: &RelativePath) -> LocalResult<File> {
        if path.as_path().as_os_str().is_empty() {
            return self
                .descriptor
                .try_clone()
                .map_err(|error| io_error(LocalFileOperation::Metadata, path.as_path(), None, error));
        }
        let mut candidate = path.as_path().to_path_buf();
        loop {
            let relative = RelativePath::parse(&candidate)?;
            match open_parent(self, &relative, LocalFileOperation::Metadata) {
                Ok((parent, _)) => return Ok(parent),
                Err(error) if error.kind() == LocalFileErrorKind::NotFound => {
                    let Some(parent) = relative.as_path().parent() else {
                        return self
                            .descriptor
                            .try_clone()
                            .map_err(|source| io_error(LocalFileOperation::Metadata, path.as_path(), None, source));
                    };
                    if parent.as_os_str().is_empty() {
                        return self
                            .descriptor
                            .try_clone()
                            .map_err(|source| io_error(LocalFileOperation::Metadata, path.as_path(), None, source));
                    }
                    candidate = parent.to_path_buf();
                }
                Err(error) => return Err(error),
            }
        }
    }
}

/// Opens the parent of a validated non-empty relative path.
///
/// # Errors
///
/// Returns an operation-specific error when the path is empty or a directory
/// component cannot be opened without following a symbolic link.
pub(super) fn open_parent(
    namespace: &NamespaceHandle,
    path: &RelativePath,
    operation: LocalFileOperation,
) -> LocalResult<(File, CString)> {
    let Some(final_name) = path.as_path().file_name() else {
        return Err(LocalFileError::new(LocalFileErrorKind::InvalidPath, operation)
            .with_path(path.as_path().to_path_buf())
            .with_reason("the authority root has no parent-relative entry name"));
    };
    let mut directory = namespace
        .descriptor
        .try_clone()
        .map_err(|error| io_error(operation, path.as_path(), None, error))?;
    if let Some(parent) = path.as_path().parent() {
        for component in parent.components() {
            let name = component_c_string(component.as_os_str());
            directory = open_directory_at(&directory, &name)
                .map_err(|error| io_error(operation, path.as_path(), None, error))?;
        }
    }
    Ok((directory, component_c_string(final_name)))
}

/// Opens one child through an already-opened directory descriptor.
///
/// # Errors
///
/// Returns the native `openat` error.
pub(super) fn open_file_at(parent: &File, name: &CStr, flags: libc::c_int, mode: libc::mode_t) -> io::Result<File> {
    // SAFETY: the parent descriptor and component string remain live for the
    // call. A successful descriptor is transferred immediately into `File`.
    let descriptor = unsafe { libc::openat(parent.as_raw_fd(), name.as_ptr(), flags, libc::c_uint::from(mode)) };
    if descriptor == -1 {
        return Err(io::Error::last_os_error());
    }
    // SAFETY: `openat` returned a new owned descriptor not wrapped elsewhere.
    Ok(unsafe { File::from_raw_fd(descriptor) })
}

/// Opens one no-follow directory component.
fn open_directory_at(parent: &File, name: &CStr) -> io::Result<File> {
    open_file_at(
        parent,
        name,
        libc::O_RDONLY | libc::O_DIRECTORY | libc::O_NOFOLLOW | libc::O_CLOEXEC,
        0,
    )
}

/// Reads no-follow metadata for a relative path.
fn status_at(
    namespace: &NamespaceHandle,
    path: &RelativePath,
    operation: LocalFileOperation,
) -> LocalResult<libc::stat> {
    let (parent, name) = open_parent(namespace, path, operation)?;
    stat_child(&parent, &name, operation, path.as_path())
}

/// Reads no-follow metadata for one child of an open directory.
///
/// # Errors
///
/// Returns an operation-specific error when `fstatat` fails.
pub(super) fn stat_child(
    parent: &File,
    name: &CStr,
    operation: LocalFileOperation,
    path: &Path,
) -> LocalResult<libc::stat> {
    let mut status = MaybeUninit::<libc::stat>::uninit();
    // SAFETY: output storage, parent descriptor, and child name remain live
    // for this non-retaining no-follow metadata call.
    let result = unsafe {
        libc::fstatat(
            parent.as_raw_fd(),
            name.as_ptr(),
            status.as_mut_ptr(),
            libc::AT_SYMLINK_NOFOLLOW,
        )
    };
    if result == -1 {
        return Err(io_error(operation, path, None, io::Error::last_os_error()));
    }
    // SAFETY: successful `fstatat` initialized the complete status value.
    Ok(unsafe { status.assume_init() })
}

/// Classifies one native `st_mode` value.
pub(super) fn kind_from_mode<T>(mode: T) -> LocalFileKind
where
    T: BitAnd<Output = T> + Copy + From<libc::mode_t> + PartialEq,
{
    let kind = mode & T::from(libc::S_IFMT);
    if kind == T::from(libc::S_IFREG) {
        LocalFileKind::File
    } else if kind == T::from(libc::S_IFDIR) {
        LocalFileKind::Directory
    } else if kind == T::from(libc::S_IFLNK) {
        LocalFileKind::Symlink
    } else if kind == T::from(libc::S_IFIFO) {
        LocalFileKind::Fifo
    } else if kind == T::from(libc::S_IFSOCK) {
        LocalFileKind::Socket
    } else if kind == T::from(libc::S_IFBLK) {
        LocalFileKind::BlockDevice
    } else if kind == T::from(libc::S_IFCHR) {
        LocalFileKind::CharDevice
    } else {
        LocalFileKind::Other
    }
}

/// Converts a no-follow Unix status into normalized metadata.
fn metadata_from_stat(status: &libc::stat) -> LocalFileMetadata {
    let (accessed_at, modified_at, created_at) = stat_times(status);
    LocalFileMetadata::from_parts(
        kind_from_mode(status.st_mode),
        u64::try_from(status.st_size).unwrap_or_default(),
        accessed_at,
        modified_at,
        created_at,
    )
}

/// Converts a non-negative Unix timestamp into `SystemTime`.
fn system_time<N>(seconds: libc::time_t, nanoseconds: N) -> Option<SystemTime>
where
    N: TryInto<u64>,
{
    let seconds = u64::try_from(seconds).ok()?;
    let nanoseconds = nanoseconds.try_into().ok()?;
    UNIX_EPOCH.checked_add(Duration::from_secs(seconds).saturating_add(Duration::from_nanos(nanoseconds)))
}

/// Extracts timestamps from Linux and Android status values.
#[cfg(any(target_os = "linux", target_os = "android"))]
fn stat_times(status: &libc::stat) -> (Option<SystemTime>, Option<SystemTime>, Option<SystemTime>) {
    (
        system_time(status.st_atime, status.st_atime_nsec),
        system_time(status.st_mtime, status.st_mtime_nsec),
        None,
    )
}

/// Extracts timestamps from Apple and FreeBSD status values.
#[cfg(any(target_os = "macos", target_os = "ios", target_os = "freebsd"))]
fn stat_times(status: &libc::stat) -> (Option<SystemTime>, Option<SystemTime>, Option<SystemTime>) {
    (
        system_time(status.st_atime, status.st_atime_nsec),
        system_time(status.st_mtime, status.st_mtime_nsec),
        system_time(status.st_birthtime, status.st_birthtime_nsec),
    )
}

/// Reports unavailable timestamps on other Unix layouts.
#[cfg(not(any(
    target_os = "linux",
    target_os = "android",
    target_os = "macos",
    target_os = "ios",
    target_os = "freebsd",
)))]
fn stat_times(_status: &libc::stat) -> (Option<SystemTime>, Option<SystemTime>, Option<SystemTime>) {
    (None, None, None)
}

/// Deletes one final entry using the requested directory flag.
fn unlink(
    namespace: &NamespaceHandle,
    path: &RelativePath,
    directory: bool,
    operation: LocalFileOperation,
) -> LocalResult<()> {
    let (parent, name) = open_parent(namespace, path, operation)?;
    let flags = if directory { libc::AT_REMOVEDIR } else { 0 };
    // SAFETY: the parent descriptor and child name remain live for this
    // non-retaining unlink operation.
    let result = unsafe { libc::unlinkat(parent.as_raw_fd(), name.as_ptr(), flags) };
    if result == 0 {
        Ok(())
    } else {
        Err(io_error(operation, path.as_path(), None, io::Error::last_os_error()))
    }
}

/// Clears `O_NONBLOCK` after a safety open.
fn clear_nonblocking(file: &File) -> io::Result<()> {
    // SAFETY: `file` owns a live descriptor for both non-retaining calls.
    let flags = unsafe { libc::fcntl(file.as_raw_fd(), libc::F_GETFL) };
    if flags == -1 {
        return Err(io::Error::last_os_error());
    }
    if flags & libc::O_NONBLOCK == 0 {
        return Ok(());
    }
    // SAFETY: `F_SETFL` accepts the status flags returned by `F_GETFL` after
    // clearing the nonblocking bit, and the descriptor remains live.
    if unsafe { libc::fcntl(file.as_raw_fd(), libc::F_SETFL, flags & !libc::O_NONBLOCK) } == -1 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}

/// Converts one validated native component into a C string.
#[must_use]
fn component_c_string(component: &OsStr) -> CString {
    CString::new(component.as_bytes()).expect("RelativePath guarantees components without NUL")
}

/// Creates a structured wrong-type error.
fn type_error(operation: LocalFileOperation, path: &Path, expected: &'static str) -> LocalFileError {
    LocalFileError::new(LocalFileErrorKind::TypeConflict, operation)
        .with_path(path.to_path_buf())
        .with_reason(expected)
}

/// Converts a native I/O failure into the crate's structured error.
pub(super) fn io_error(
    operation: LocalFileOperation,
    path: &Path,
    target: Option<&Path>,
    error: io::Error,
) -> LocalFileError {
    LocalFileError::from_io(
        operation,
        Some(path.to_path_buf()),
        target.map(Path::to_path_buf),
        error,
    )
}

/// Performs an atomic no-replace descriptor-relative rename on Linux.
#[cfg(any(target_os = "linux", target_os = "android"))]
fn rename_no_replace(
    source_parent: &File,
    source_name: &CStr,
    target_parent: &File,
    target_name: &CStr,
) -> LocalResult<libc::c_int> {
    // SAFETY: both parent descriptors and names remain live for this
    // non-retaining atomic rename.
    Ok(unsafe {
        libc::renameat2(
            source_parent.as_raw_fd(),
            source_name.as_ptr(),
            target_parent.as_raw_fd(),
            target_name.as_ptr(),
            libc::RENAME_NOREPLACE as _,
        )
    })
}

/// Performs an atomic no-replace descriptor-relative rename on Apple systems.
#[cfg(any(target_os = "macos", target_os = "ios"))]
fn rename_no_replace(
    source_parent: &File,
    source_name: &CStr,
    target_parent: &File,
    target_name: &CStr,
) -> LocalResult<libc::c_int> {
    // SAFETY: both parent descriptors and names remain live for this
    // non-retaining exclusive rename.
    Ok(unsafe {
        libc::renameatx_np(
            source_parent.as_raw_fd(),
            source_name.as_ptr(),
            target_parent.as_raw_fd(),
            target_name.as_ptr(),
            libc::RENAME_EXCL,
        )
    })
}

/// Reports missing atomic no-replace rename support on other Unix targets.
#[cfg(not(any(target_os = "linux", target_os = "android", target_os = "macos", target_os = "ios",)))]
fn rename_no_replace(
    _source_parent: &File,
    _source_name: &CStr,
    _target_parent: &File,
    _target_name: &CStr,
) -> LocalResult<libc::c_int> {
    Err(
        LocalFileError::new(LocalFileErrorKind::Unsupported, LocalFileOperation::Rename)
            .with_reason("atomic descriptor-relative no-replace rename is unsupported"),
    )
}
