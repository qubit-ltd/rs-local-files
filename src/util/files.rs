/*******************************************************************************
 *
 *    Copyright (c) 2026 Haixing Hu.
 *
 *    SPDX-License-Identifier: Apache-2.0
 *
 *    Licensed under the Apache License, Version 2.0.
 *
 ******************************************************************************/
use std::env;
use std::ffi::OsString;
use std::fs::{
    self,
    File,
    OpenOptions,
};
use std::io::{
    BufReader,
    BufWriter,
    Error,
    ErrorKind,
    Result,
    Write,
};
use std::path::{
    Path,
    PathBuf,
};

use crate::{
    CopyDirOptions,
    CopyDirStats,
    Filenames,
};

#[cfg(windows)]
use std::ffi::c_void;
#[cfg(windows)]
use std::os::windows::ffi::OsStrExt;
#[cfg(windows)]
use std::os::windows::io::{
    FromRawHandle,
    RawHandle,
};

#[cfg(windows)]
const MOVEFILE_REPLACE_EXISTING: u32 = 0x0000_0001;
#[cfg(windows)]
const MOVEFILE_WRITE_THROUGH: u32 = 0x0000_0008;
#[cfg(windows)]
const GENERIC_READ: u32 = 0x8000_0000;
#[cfg(windows)]
const FILE_SHARE_READ: u32 = 0x0000_0001;
#[cfg(windows)]
const FILE_SHARE_WRITE: u32 = 0x0000_0002;
#[cfg(windows)]
const FILE_SHARE_DELETE: u32 = 0x0000_0004;
#[cfg(windows)]
const OPEN_EXISTING: u32 = 3;
#[cfg(windows)]
const FILE_FLAG_BACKUP_SEMANTICS: u32 = 0x0200_0000;
#[cfg(windows)]
const INVALID_HANDLE_VALUE: RawHandle = -1isize as RawHandle;

/// Default suffix used by atomic-write temporary files.
const ATOMIC_WRITE_TEMP_SUFFIX: &str = ".tmp";

/// Prefix used by atomic-write temporary files.
const ATOMIC_WRITE_TEMP_PREFIX: &str = ".atomic-write-";

#[cfg(windows)]
unsafe extern "system" {
    fn MoveFileExW(existing_file_name: *const u16, new_file_name: *const u16, flags: u32) -> i32;

    fn CreateFileW(
        file_name: *const u16,
        desired_access: u32,
        share_mode: u32,
        security_attributes: *mut c_void,
        creation_disposition: u32,
        flags_and_attributes: u32,
        template_file: RawHandle,
    ) -> RawHandle;
}

/// File-system utility namespace.
///
/// This type is an uninstantiable namespace. Use its associated methods for
/// small recurring file operations, including parent creation, local directory
/// operations, and atomic replacement writes.
///
/// # Examples
/// ```
/// use qubit_local_fs::{
///     Files,
///     TempDir,
/// };
///
/// let dir = TempDir::with_prefix(Some("qubit-local-fs-doc-"))?;
/// let path = dir.path().join("nested").join("data.txt");
///
/// Files::atomic_write(&path, b"payload")?;
/// assert_eq!(b"payload", std::fs::read(&path)?.as_slice());
/// # Ok::<(), std::io::Error>(())
/// ```
pub enum Files {}

impl Files {
    /// Default prefix used when callers do not provide a temporary file prefix.
    pub const DEFAULT_TEMP_FILE_PREFIX: &str = "qubit-local-fs-";

    /// Default number of attempts used when creating a random temporary entry.
    pub const DEFAULT_TEMP_FILE_RETRIES: usize = 256;

    /// Opens a file as a buffered reader.
    ///
    /// # Parameters
    /// - `path`: File path to open.
    ///
    /// # Returns
    /// A [`BufReader`] wrapping the opened file.
    ///
    /// # Errors
    /// Returns the error reported by [`File::open`].
    #[inline]
    pub fn open_buffered_reader<P>(path: P) -> Result<BufReader<File>>
    where
        P: AsRef<Path>,
    {
        File::open(path).map(BufReader::new)
    }

    /// Ensures that a directory exists.
    ///
    /// # Parameters
    /// - `path`: Directory path to create if missing.
    ///
    /// # Errors
    /// Returns an I/O error when the directory or one of its ancestors cannot
    /// be created.
    #[inline]
    pub fn ensure_dir<P>(path: P) -> Result<()>
    where
        P: AsRef<Path>,
    {
        ensure_dir_path(path.as_ref())
    }

    /// Ensures that a path's parent directory exists.
    ///
    /// Parentless paths and paths whose parent is empty are accepted without
    /// creating any directory.
    ///
    /// # Parameters
    /// - `path`: File path whose parent directory should be created.
    ///
    /// # Errors
    /// Returns an I/O error when the parent directory or one of its ancestors
    /// cannot be created.
    #[inline]
    pub fn ensure_parent<P>(path: P) -> Result<()>
    where
        P: AsRef<Path>,
    {
        ensure_parent_path(path.as_ref())
    }

    /// Creates a file after creating missing parent directories.
    ///
    /// # Parameters
    /// - `path`: File path to create.
    ///
    /// # Returns
    /// The created file.
    ///
    /// # Errors
    /// Returns an I/O error when parent directories or the file cannot be
    /// created.
    pub fn create_file_with_parent<P>(path: P) -> Result<File>
    where
        P: AsRef<Path>,
    {
        let path = path.as_ref();
        ensure_parent_path(path)?;
        File::create(path)
    }

    /// Creates a buffered writer after creating missing parent directories.
    ///
    /// # Parameters
    /// - `path`: File path to create.
    ///
    /// # Returns
    /// A [`BufWriter`] wrapping the created file.
    ///
    /// # Errors
    /// Returns an I/O error when parent directories or the file cannot be
    /// created.
    #[inline]
    pub fn create_buffered_writer_with_parent<P>(path: P) -> Result<BufWriter<File>>
    where
        P: AsRef<Path>,
    {
        Self::create_file_with_parent(path).map(BufWriter::new)
    }

    /// Computes the total size of regular files under a directory.
    ///
    /// The root path must be a directory. This method walks the directory tree
    /// recursively, sums the byte length of regular files, and does not follow
    /// symbolic links. Symbolic links are ignored.
    ///
    /// # Parameters
    /// - `path`: Directory whose regular-file contents should be measured.
    ///
    /// # Returns
    /// Total byte length of regular files contained in the directory tree.
    ///
    /// # Errors
    /// Returns an I/O error when `path` cannot be inspected, is not a directory,
    /// or one of the directory entries cannot be read.
    #[inline]
    pub fn dir_size<P>(path: P) -> Result<u64>
    where
        P: AsRef<Path>,
    {
        dir_size_path(path.as_ref())
    }

    /// Removes all entries from a directory while keeping the directory itself.
    ///
    /// This method deletes files, directories, and symbolic links directly
    /// contained in `path`. Nested directories are removed recursively. Symbolic
    /// links are removed as links and are not followed.
    ///
    /// # Parameters
    /// - `path`: Directory to clean.
    ///
    /// # Errors
    /// Returns an I/O error when `path` cannot be read, is not a directory, or
    /// one of its entries cannot be removed.
    #[inline]
    pub fn clean_dir<P>(path: P) -> Result<()>
    where
        P: AsRef<Path>,
    {
        clean_dir_path(path.as_ref())
    }

    /// Removes a file, directory, or symbolic link.
    ///
    /// Directories are removed recursively. Symbolic links are removed as links
    /// and are not followed, including links that point to directories.
    ///
    /// # Parameters
    /// - `path`: Path to remove.
    ///
    /// # Errors
    /// Returns an I/O error when `path` cannot be inspected or removed.
    #[inline]
    pub fn remove_any<P>(path: P) -> Result<()>
    where
        P: AsRef<Path>,
    {
        remove_any_path(path.as_ref())
    }

    /// Recursively copies a directory tree.
    ///
    /// The source path must be a directory. The destination directory is created
    /// when missing. Existing files are rejected unless
    /// [`CopyDirOptions::overwrite`] is enabled. By default, symbolic links are
    /// rejected instead of followed so a copy cannot accidentally leave the
    /// requested source tree.
    ///
    /// This method also rejects destinations located inside the source tree,
    /// because copying a directory into itself can recurse indefinitely.
    ///
    /// # Parameters
    /// - `src`: Source directory.
    /// - `dst`: Destination directory.
    /// - `options`: Copy behavior options.
    ///
    /// # Returns
    /// Statistics describing copied files, created directories, and copied
    /// bytes.
    ///
    /// # Errors
    /// Returns an I/O error when the source is not a directory, the destination
    /// is inside the source tree, a destination entry exists without overwrite
    /// permission, a symbolic link is encountered while `follow_symlinks` is
    /// `false`, or an underlying filesystem operation fails.
    #[inline]
    pub fn copy_dir_all_with<S, D>(src: S, dst: D, options: CopyDirOptions) -> Result<CopyDirStats>
    where
        S: AsRef<Path>,
        D: AsRef<Path>,
    {
        copy_dir_all_with_paths(src.as_ref(), dst.as_ref(), options)
    }

    /// Atomically writes bytes to a path using a temporary file in the same
    /// directory.
    ///
    /// This method is intended for replacing a whole file with newly generated
    /// contents without exposing a partially written destination. Typical use
    /// cases include configuration files, cache manifests, checkpoint files,
    /// generated indexes, and other small to medium state files where callers
    /// want readers to observe either the old complete file or the new complete
    /// file.
    ///
    /// Parent directories are created before writing. The data is written to a
    /// randomly named same-directory temporary file, flushed and synced, and
    /// then renamed over the destination path with platform-specific replace
    /// semantics. Using the same directory keeps the temporary file on the same
    /// filesystem as the destination, which is required for atomic replacement
    /// on common platforms. After the replacement, the parent directory is
    /// synced so directory metadata reaches durable storage on platforms that
    /// support directory syncing.
    ///
    /// If writing or syncing the temporary file fails, the temporary file is
    /// removed and the existing destination is left untouched. If replacement
    /// succeeds but syncing the parent directory fails, the destination may
    /// already contain the new contents even though this method returns an
    /// error.
    ///
    /// This method is not a multi-file transaction and does not coordinate
    /// concurrent writers. Use an external lock if multiple processes or
    /// threads may replace the same path at the same time. It is also not an
    /// append helper; append-only logs should use normal append-mode writes.
    ///
    /// # Examples
    /// ```
    /// use qubit_local_fs::{
    ///     Files,
    ///     TempDir,
    /// };
    ///
    /// let dir = TempDir::with_prefix(Some("qubit-local-fs-atomic-"))?;
    /// let path = dir.path().join("state").join("manifest.json");
    ///
    /// Files::atomic_write(&path, br#"{"version":1,"complete":true}"#)?;
    ///
    /// assert_eq!(
    ///     br#"{"version":1,"complete":true}"#,
    ///     std::fs::read(&path)?.as_slice(),
    /// );
    /// # Ok::<(), std::io::Error>(())
    /// ```
    ///
    /// # Parameters
    /// - `path`: Destination path.
    /// - `bytes`: Bytes to write.
    ///
    /// # Errors
    /// Returns the first I/O error reported while creating, writing, syncing,
    /// removing, replacing, or syncing the temporary file or parent directory.
    #[inline]
    pub fn atomic_write<P, B>(path: P, bytes: B) -> Result<()>
    where
        P: AsRef<Path>,
        B: AsRef<[u8]>,
    {
        atomic_write_bytes_path(path.as_ref(), bytes.as_ref())
    }

    /// Atomically writes a file using caller-provided write logic.
    ///
    /// The closure receives the temporary file. After the closure succeeds, the
    /// file is flushed, synced, closed, replaced over the destination path, and
    /// the parent directory is synced. If replacement succeeds but syncing the
    /// parent directory fails, the destination may already contain the new
    /// contents even though this method returns an error.
    ///
    /// # Parameters
    /// - `path`: Destination path.
    /// - `write`: Function that writes the desired contents into the temporary
    ///   file.
    ///
    /// # Errors
    /// Returns the first I/O error reported while creating, writing, syncing,
    /// removing, replacing, or syncing the temporary file or parent directory.
    #[inline]
    pub fn atomic_write_with<P, F>(path: P, write: F) -> Result<()>
    where
        P: AsRef<Path>,
        F: FnMut(&mut File) -> Result<()>,
    {
        let mut write = write;
        atomic_write_with_path(path.as_ref(), &mut write)
    }
}

/// Ensures that the directory at `path` exists.
///
/// # Parameters
/// - `path`: Directory path to create.
///
/// # Errors
/// Returns an I/O error when the directory or one of its ancestors cannot be
/// created.
fn ensure_dir_path(path: &Path) -> Result<()> {
    fs::create_dir_all(path)
}

/// Ensures that the parent directory of `path` exists.
///
/// # Parameters
/// - `path`: File path whose parent directory should be created.
///
/// # Errors
/// Returns an I/O error when the parent directory or one of its ancestors cannot
/// be created.
fn ensure_parent_path(path: &Path) -> Result<()> {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        ensure_dir_path(parent)?;
    }
    Ok(())
}

/// Atomically writes `bytes` to `path`.
///
/// # Parameters
/// - `path`: Destination path.
/// - `bytes`: Bytes to write.
///
/// # Errors
/// Returns the first I/O error reported while writing the temporary file,
/// replacing the destination, or syncing the parent directory.
fn atomic_write_bytes_path(path: &Path, bytes: &[u8]) -> Result<()> {
    let mut write = |file: &mut File| file.write_all(bytes);
    atomic_write_with_path(path, &mut write)
}

/// Atomically writes a file at `path` using `write`.
///
/// # Parameters
/// - `path`: Destination path.
/// - `write`: Function that writes the desired contents into the temporary file.
///
/// # Errors
/// Returns the first I/O error reported while creating, writing, syncing,
/// replacing, or syncing the temporary file or parent directory.
fn atomic_write_with_path(
    path: &Path,
    write: &mut dyn FnMut(&mut File) -> Result<()>,
) -> Result<()> {
    ensure_parent_path(path)?;
    let existing_permissions = existing_file_permissions(path)?;
    let parent = parent_dir_for(path);
    let (temp_path, mut file) = create_temp_file_in_dir(
        parent,
        Some(ATOMIC_WRITE_TEMP_PREFIX),
        Some(ATOMIC_WRITE_TEMP_SUFFIX),
        Files::DEFAULT_TEMP_FILE_RETRIES,
    )?;

    let result = write(&mut file)
        .and_then(|()| apply_existing_permissions(&file, existing_permissions.as_ref(), &temp_path))
        .and_then(|()| file.flush())
        .and_then(|()| file.sync_all());
    if let Err(error) = result {
        drop(file);
        drop(fs::remove_file(&temp_path));
        return Err(error);
    }

    drop(file);
    if let Err(error) = replace_file(&temp_path, path) {
        drop(fs::remove_file(&temp_path));
        return Err(error);
    }
    sync_parent_dir(path)
}

/// Returns existing destination permissions to preserve during atomic writes.
///
/// # Parameters
/// - `path`: Destination file path.
///
/// # Returns
/// Existing file permissions when `path` points to a regular file.
///
/// # Errors
/// Returns an I/O error when destination metadata exists but cannot be read.
fn existing_file_permissions(path: &Path) -> Result<Option<fs::Permissions>> {
    match fs::metadata(path) {
        Ok(metadata) if metadata.is_file() => Ok(Some(metadata.permissions())),
        Ok(_) => Ok(None),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
        Err(error) => Err(add_path_context(error, "read destination metadata", path)),
    }
}

/// Applies preserved destination permissions to the temporary file.
///
/// # Parameters
/// - `file`: Temporary file handle.
/// - `permissions`: Optional permissions to apply.
/// - `temp_path`: Temporary file path used for error context.
///
/// # Errors
/// Returns an I/O error when permissions cannot be applied.
fn apply_existing_permissions(
    file: &File,
    permissions: Option<&fs::Permissions>,
    temp_path: &Path,
) -> Result<()> {
    if let Some(permissions) = permissions {
        match file.set_permissions(permissions.clone()) {
            Ok(()) => {}
            Err(error) => {
                return Err(add_path_context(
                    error,
                    "set temporary file permissions",
                    temp_path,
                ));
            }
        }
    }
    Ok(())
}

/// Creates a unique temporary file in `dir`.
///
/// # Parameters
/// - `dir`: Directory in which to create the file.
/// - `prefix`: Optional file-name prefix.
/// - `suffix`: Optional file-name suffix.
/// - `max_tries`: Maximum number of generated names to try.
///
/// # Returns
/// The created temporary path and open file handle.
///
/// # Errors
/// Returns an I/O error when `dir` cannot be created, `max_tries` is zero, all
/// generated names collide, or file creation fails.
pub(crate) fn create_temp_file_in_dir(
    dir: &Path,
    prefix: Option<&str>,
    suffix: Option<&str>,
    max_tries: usize,
) -> Result<(PathBuf, File)> {
    validate_max_tries(max_tries)?;
    ensure_dir_path(dir)?;
    let mut attempt = 0;
    loop {
        attempt += 1;
        let path = dir.join(Filenames::try_random_with(prefix, suffix)?);
        match OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .open(&path)
        {
            Ok(file) => return Ok((path, file)),
            Err(error) => {
                if error.kind() == ErrorKind::AlreadyExists && attempt < max_tries {
                    continue;
                }
                return Err(add_path_context(error, "create temporary file", &path));
            }
        }
    }
}

/// Creates a unique temporary directory in `dir`.
///
/// # Parameters
/// - `dir`: Directory in which to create the directory.
/// - `prefix`: Optional directory-name prefix.
/// - `max_tries`: Maximum number of generated names to try.
///
/// # Returns
/// The created temporary directory path.
///
/// # Errors
/// Returns an I/O error when `dir` cannot be created, `max_tries` is zero, all
/// generated names collide, or directory creation fails.
pub(crate) fn create_temp_dir_in_dir(
    dir: &Path,
    prefix: Option<&str>,
    max_tries: usize,
) -> Result<PathBuf> {
    validate_max_tries(max_tries)?;
    ensure_dir_path(dir)?;
    let mut attempt = 0;
    loop {
        attempt += 1;
        let path = dir.join(Filenames::try_random_with(prefix, None)?);
        match fs::create_dir(&path) {
            Ok(()) => return Ok(path),
            Err(error) => {
                if error.kind() == ErrorKind::AlreadyExists && attempt < max_tries {
                    continue;
                }
                return Err(add_path_context(error, "create temporary directory", &path));
            }
        }
    }
}

/// Validates a retry count.
///
/// # Parameters
/// - `max_tries`: Retry count to validate.
///
/// # Errors
/// Returns [`ErrorKind::InvalidInput`] when `max_tries` is zero.
fn validate_max_tries(max_tries: usize) -> Result<()> {
    if max_tries == 0 {
        return Err(Error::new(
            ErrorKind::InvalidInput,
            "temporary entry retry count must be greater than zero",
        ));
    }
    Ok(())
}

/// Adds path context to an I/O error while preserving its kind.
///
/// # Parameters
/// - `error`: Original I/O error.
/// - `operation`: Operation that failed.
/// - `path`: Path involved in the operation.
///
/// # Returns
/// A new I/O error with the same [`ErrorKind`] and a more descriptive message.
fn add_path_context(error: Error, operation: &str, path: &Path) -> Error {
    Error::new(
        error.kind(),
        format!("failed to {operation} '{}': {error}", path.display()),
    )
}

/// Replaces `destination` with `source`.
///
/// # Parameters
/// - `source`: Existing temporary file path.
/// - `destination`: Destination file path.
///
/// # Errors
/// Returns the platform I/O error reported while replacing the destination.
#[cfg(not(windows))]
fn replace_file(source: &Path, destination: &Path) -> Result<()> {
    fs::rename(source, destination)
}

/// Replaces `destination` with `source`.
///
/// # Parameters
/// - `source`: Existing temporary file path.
/// - `destination`: Destination file path.
///
/// # Errors
/// Returns the platform I/O error reported while replacing the destination.
#[cfg(windows)]
fn replace_file(source: &Path, destination: &Path) -> Result<()> {
    let source = wide_path(source);
    let destination = wide_path(destination);
    let result = unsafe {
        MoveFileExW(
            source.as_ptr(),
            destination.as_ptr(),
            MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
        )
    };
    if result == 0 {
        Err(std::io::Error::last_os_error())
    } else {
        Ok(())
    }
}

/// Syncs the parent directory for `path`.
///
/// # Parameters
/// - `path`: File path whose parent directory should be synced.
///
/// # Errors
/// Returns an I/O error when opening or syncing the parent directory fails.
#[cfg(not(windows))]
fn sync_parent_dir(path: &Path) -> Result<()> {
    let parent_dir = parent_dir_for(path);
    let parent = File::open(parent_dir)?;
    parent.sync_all()
}

/// Syncs the parent directory for `path`.
///
/// # Parameters
/// - `path`: File path whose parent directory should be synced.
///
/// # Errors
/// Returns an I/O error when opening or syncing the parent directory fails.
#[cfg(windows)]
fn sync_parent_dir(path: &Path) -> Result<()> {
    let parent = wide_path(parent_dir_for(path));
    let handle = unsafe {
        CreateFileW(
            parent.as_ptr(),
            GENERIC_READ,
            FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE,
            std::ptr::null_mut(),
            OPEN_EXISTING,
            FILE_FLAG_BACKUP_SEMANTICS,
            std::ptr::null_mut(),
        )
    };
    if handle == INVALID_HANDLE_VALUE {
        let error = std::io::Error::last_os_error();
        return if is_ignorable_windows_parent_sync_error(&error) {
            Ok(())
        } else {
            Err(error)
        };
    }
    let directory = unsafe { File::from_raw_handle(handle) };
    match directory.sync_all() {
        Ok(()) => Ok(()),
        Err(error) if is_ignorable_windows_parent_sync_error(&error) => Ok(()),
        Err(error) => Err(error),
    }
}

/// Tests whether a Windows parent-directory sync error should be ignored.
///
/// # Parameters
/// - `error`: Error reported while opening or syncing the parent directory.
///
/// # Returns
/// `true` when the error only means the best-effort parent directory sync is
/// unavailable on Windows.
#[cfg(windows)]
fn is_ignorable_windows_parent_sync_error(error: &Error) -> bool {
    const ERROR_SHARING_VIOLATION: i32 = 32;

    error.kind() == ErrorKind::PermissionDenied
        || error.raw_os_error() == Some(ERROR_SHARING_VIOLATION)
}

/// Gets the parent directory that should be synced for `path`.
///
/// # Parameters
/// - `path`: File path whose parent directory is needed.
///
/// # Returns
/// The parent directory, or the current directory for parentless paths.
fn parent_dir_for(path: &Path) -> &Path {
    if let Some(parent) = path.parent()
        && !parent.as_os_str().is_empty()
    {
        return parent;
    }
    Path::new(".")
}

/// Converts a path into a null-terminated Windows wide string.
///
/// # Parameters
/// - `path`: Path to convert.
///
/// # Returns
/// Null-terminated UTF-16 path buffer.
#[cfg(windows)]
fn wide_path(path: &Path) -> Vec<u16> {
    path.as_os_str().encode_wide().chain(Some(0)).collect()
}

/// Computes the total size of regular files below a directory path.
///
/// # Parameters
/// - `path`: Directory path to measure.
///
/// # Returns
/// The total byte length of regular files under `path`.
///
/// # Errors
/// Returns an I/O error when `path` is not a directory or cannot be read.
fn dir_size_path(path: &Path) -> Result<u64> {
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(Error::new(
            ErrorKind::InvalidInput,
            format!("path is not a directory: {}", path.display()),
        ));
    }
    dir_size_recursive(path)
}

/// Recursively computes regular-file sizes below a directory.
///
/// # Parameters
/// - `path`: Directory path to measure.
///
/// # Returns
/// The total byte length of regular files under `path`.
///
/// # Errors
/// Returns an I/O error when a directory entry cannot be read.
fn dir_size_recursive(path: &Path) -> Result<u64> {
    let mut total = 0u64;
    for entry in fs::read_dir(path)? {
        let entry = entry?;
        let metadata = fs::symlink_metadata(entry.path())?;
        let file_type = metadata.file_type();
        if file_type.is_symlink() {
            continue;
        }
        if metadata.is_dir() {
            total += dir_size_recursive(&entry.path())?;
        } else if metadata.is_file() {
            total += metadata.len();
        }
    }
    Ok(total)
}

/// Removes all children from a directory while keeping the directory itself.
///
/// # Parameters
/// - `path`: Directory path to clean.
///
/// # Errors
/// Returns an I/O error when `path` is not a directory, cannot be read, or a
/// child cannot be removed.
fn clean_dir_path(path: &Path) -> Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(Error::new(
            ErrorKind::InvalidInput,
            format!("path is not a directory: {}", path.display()),
        ));
    }
    for entry in fs::read_dir(path)? {
        remove_any_path(&entry?.path())?;
    }
    Ok(())
}

/// Removes a path regardless of whether it is a file, directory, or symlink.
///
/// # Parameters
/// - `path`: Path to remove.
///
/// # Errors
/// Returns an I/O error when `path` cannot be inspected or removed.
fn remove_any_path(path: &Path) -> Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    let file_type = metadata.file_type();
    if metadata.is_dir() && !file_type.is_symlink() {
        fs::remove_dir_all(path)
    } else {
        fs::remove_file(path)
    }
}

/// Recursively copies a directory tree with the supplied options.
///
/// # Parameters
/// - `src`: Source directory.
/// - `dst`: Destination directory.
/// - `options`: Copy behavior options.
///
/// # Returns
/// Copy statistics for regular files, created directories, and bytes.
///
/// # Errors
/// Returns an I/O error when the source is invalid, the destination is inside
/// the source tree, or an underlying filesystem operation fails.
fn copy_dir_all_with_paths(
    src: &Path,
    dst: &Path,
    options: CopyDirOptions,
) -> Result<CopyDirStats> {
    let source_metadata = metadata_for_copy_source(src, options.follow_symlinks)?;
    if !source_metadata.is_dir() {
        return Err(Error::new(
            ErrorKind::InvalidInput,
            format!("source is not a directory: {}", src.display()),
        ));
    }
    reject_destination_inside_source(src, dst)?;
    let mut stats = CopyDirStats::default();
    copy_dir_recursive(src, dst, options, &mut stats)?;
    Ok(stats)
}

/// Recursively copies one source directory into one destination directory.
///
/// # Parameters
/// - `src`: Source directory.
/// - `dst`: Destination directory.
/// - `options`: Copy behavior options.
/// - `stats`: Mutable copy statistics accumulator.
///
/// # Errors
/// Returns an I/O error when a directory or file cannot be copied.
fn copy_dir_recursive(
    src: &Path,
    dst: &Path,
    options: CopyDirOptions,
    stats: &mut CopyDirStats,
) -> Result<()> {
    let source_metadata = metadata_for_copy_source(src, options.follow_symlinks)?;
    if !source_metadata.is_dir() {
        return Err(Error::new(
            ErrorKind::InvalidInput,
            format!("source is not a directory: {}", src.display()),
        ));
    }
    ensure_copy_destination_dir(dst, options.overwrite, stats)?;
    if options.preserve_permissions {
        fs::set_permissions(dst, source_metadata.permissions())?;
    }
    for entry in fs::read_dir(src)? {
        let entry = entry?;
        let source_path = entry.path();
        let destination_path = dst.join(entry.file_name());
        let file_type = entry.file_type()?;
        if file_type.is_symlink() {
            copy_symlink_source(&source_path, &destination_path, options, stats)?;
        } else if file_type.is_dir() {
            copy_dir_recursive(&source_path, &destination_path, options, stats)?;
        } else if file_type.is_file() {
            copy_file_with_options(&source_path, &destination_path, options, stats)?;
        } else {
            return Err(Error::new(
                ErrorKind::Unsupported,
                format!("unsupported source file type: {}", source_path.display()),
            ));
        }
    }
    Ok(())
}

/// Copies a symbolic link source when link following is enabled.
///
/// # Parameters
/// - `src`: Source symbolic link.
/// - `dst`: Destination path.
/// - `options`: Copy behavior options.
/// - `stats`: Mutable copy statistics accumulator.
///
/// # Errors
/// Returns an I/O error when symbolic links are disabled or the target cannot
/// be copied.
fn copy_symlink_source(
    src: &Path,
    dst: &Path,
    options: CopyDirOptions,
    stats: &mut CopyDirStats,
) -> Result<()> {
    if !options.follow_symlinks {
        return Err(Error::new(
            ErrorKind::Unsupported,
            format!("symbolic links are not followed: {}", src.display()),
        ));
    }
    let target_metadata = fs::metadata(src)?;
    if target_metadata.is_dir() {
        copy_dir_recursive(src, dst, options, stats)
    } else if target_metadata.is_file() {
        copy_file_with_options(src, dst, options, stats)
    } else {
        Err(Error::new(
            ErrorKind::Unsupported,
            format!("unsupported symbolic link target type: {}", src.display()),
        ))
    }
}

/// Ensures a directory copy destination exists as a directory.
///
/// # Parameters
/// - `dst`: Destination directory path.
/// - `overwrite`: Whether an existing non-directory destination may be removed.
/// - `stats`: Mutable copy statistics accumulator.
///
/// # Errors
/// Returns an I/O error when the destination cannot be created or cannot be
/// replaced according to `overwrite`.
fn ensure_copy_destination_dir(
    dst: &Path,
    overwrite: bool,
    stats: &mut CopyDirStats,
) -> Result<()> {
    match fs::symlink_metadata(dst) {
        Ok(metadata) => {
            if metadata.is_dir() && !metadata.file_type().is_symlink() {
                return Ok(());
            }
            if !overwrite {
                return Err(Error::new(
                    ErrorKind::AlreadyExists,
                    format!("destination already exists: {}", dst.display()),
                ));
            }
            remove_any_path(dst)?;
        }
        Err(error) if error.kind() == ErrorKind::NotFound => {}
        Err(error) => return Err(error),
    }
    fs::create_dir(dst)?;
    stats.directories = stats.directories.saturating_add(1);
    Ok(())
}

/// Copies one regular source file into a destination path.
///
/// # Parameters
/// - `src`: Source file path.
/// - `dst`: Destination file path.
/// - `options`: Copy behavior options.
/// - `stats`: Mutable copy statistics accumulator.
///
/// # Errors
/// Returns an I/O error when the destination exists without overwrite
/// permission or the file cannot be copied.
fn copy_file_with_options(
    src: &Path,
    dst: &Path,
    options: CopyDirOptions,
    stats: &mut CopyDirStats,
) -> Result<()> {
    prepare_copy_file_destination(dst, options.overwrite)?;
    let source_metadata = metadata_for_copy_source(src, options.follow_symlinks)?;
    let copied = fs::copy(src, dst)?;
    if options.preserve_permissions {
        fs::set_permissions(dst, source_metadata.permissions())?;
    }
    stats.files = stats.files.saturating_add(1);
    stats.bytes = stats.bytes.saturating_add(copied);
    Ok(())
}

/// Prepares a destination path for file copy.
///
/// # Parameters
/// - `dst`: Destination file path.
/// - `overwrite`: Whether an existing destination may be removed.
///
/// # Errors
/// Returns an I/O error when the destination exists and cannot be overwritten.
fn prepare_copy_file_destination(dst: &Path, overwrite: bool) -> Result<()> {
    match fs::symlink_metadata(dst) {
        Ok(_) if overwrite => remove_any_path(dst),
        Ok(_) => Err(Error::new(
            ErrorKind::AlreadyExists,
            format!("destination already exists: {}", dst.display()),
        )),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

/// Loads metadata for a source path according to symlink policy.
///
/// # Parameters
/// - `path`: Source path.
/// - `follow_symlinks`: Whether symbolic links may be followed.
///
/// # Returns
/// Metadata for `path`, following a symbolic link when allowed.
///
/// # Errors
/// Returns an I/O error when metadata cannot be loaded or a symbolic link is
/// encountered while `follow_symlinks` is `false`.
fn metadata_for_copy_source(path: &Path, follow_symlinks: bool) -> Result<fs::Metadata> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.file_type().is_symlink() {
        if follow_symlinks {
            fs::metadata(path)
        } else {
            Err(Error::new(
                ErrorKind::Unsupported,
                format!("symbolic links are not followed: {}", path.display()),
            ))
        }
    } else {
        Ok(metadata)
    }
}

/// Rejects copy destinations located inside the source tree.
///
/// # Parameters
/// - `src`: Source directory.
/// - `dst`: Destination directory.
///
/// # Errors
/// Returns an I/O error when canonicalization fails or when `dst` is equal to
/// or nested under `src`.
fn reject_destination_inside_source(src: &Path, dst: &Path) -> Result<()> {
    let source = fs::canonicalize(src)?;
    let destination = canonicalize_existing_prefix(dst)?;
    if destination == source || destination.starts_with(&source) {
        return Err(Error::new(
            ErrorKind::InvalidInput,
            format!(
                "destination must not be inside source: source={}, destination={}",
                src.display(),
                dst.display(),
            ),
        ));
    }
    Ok(())
}

/// Canonicalizes the existing prefix of a path while preserving missing tail components.
///
/// # Parameters
/// - `path`: Path that may not exist yet.
///
/// # Returns
/// A canonicalized path for the existing prefix with missing components appended.
///
/// # Errors
/// Returns an I/O error when the existing prefix cannot be canonicalized.
fn canonicalize_existing_prefix(path: &Path) -> Result<PathBuf> {
    if path.exists() {
        return fs::canonicalize(path);
    }
    let mut missing = Vec::<OsString>::new();
    let mut current = path.to_path_buf();
    while !current.exists() {
        if let Some(name) = current.file_name() {
            missing.push(name.to_os_string());
        } else {
            break;
        }
        match current.parent() {
            Some(parent) if !parent.as_os_str().is_empty() => current = parent.to_path_buf(),
            _ => {
                current = env::current_dir()?;
                break;
            }
        }
    }
    let mut canonical = fs::canonicalize(current)?;
    for component in missing.into_iter().rev() {
        canonical.push(component);
    }
    Ok(canonical)
}
