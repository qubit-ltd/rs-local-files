// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Automatically cleaned local temporary directories.

use std::ffi::OsString;
use std::fs::{
    self,
    Metadata,
    ReadDir,
};
use std::io::{
    Error,
    ErrorKind,
    Result,
};
use std::path::{
    Path,
    PathBuf,
};

use log::warn;

use crate::{
    FileReadOptions,
    FileWriteOptions,
    LocalFileReader,
    LocalFileWriter,
    LocalFiles,
    LocalPersistError,
    LocalRelativePath,
};

use super::internal::{
    absolute_path,
    create_private_dir,
    create_temp_dir_in_dir,
    move_directory_without_replacing,
};

/// Temporary directory that is removed automatically unless kept or persisted.
///
/// `LocalTempDir` owns a directory path and removes that directory tree when
/// the object is dropped. Use [`LocalTempDir::keep`] to keep the temporary
/// directory at its generated path, or [`LocalTempDir::persist`] to move it to
/// a final path.
/// Relative creation directories are bound to the process current directory
/// at creation time. [`LocalTempDir::path`], child-path helpers,
/// [`LocalTempDir::keep`], and [`LocalTempDir::persist`] expose stable absolute
/// paths that remain directly usable after later current-directory changes.
///
/// Cleanup performed from `Drop` is best-effort. If removal fails, the failure
/// is reported through the `log` facade at warning level and the program is not
/// panicked.
///
/// The guard must be retained until the directory is kept, persisted, or
/// cleaned:
///
/// ```compile_fail
/// #![deny(unused_must_use)]
/// use qubit_local_files::LocalTempDir;
///
/// let temporary_dir = LocalTempDir::new()?;
/// temporary_dir;
/// # Ok::<(), std::io::Error>(())
/// ```
#[must_use = "dropping the temporary-directory guard removes its directory"]
#[derive(Debug)]
pub struct LocalTempDir {
    /// Absolute generated path while cleanup remains armed.
    path: Option<PathBuf>,
}

impl LocalTempDir {
    /// Creates a temporary directory in the process temporary directory.
    ///
    /// # Errors
    /// Returns an I/O error when the process temporary directory cannot be
    /// created or a unique temporary directory cannot be created.
    #[inline(always)]
    pub fn new() -> Result<Self> {
        Self::in_dir(
            std::env::temp_dir(),
            None,
            LocalFiles::DEFAULT_TEMP_FILE_RETRIES,
        )
    }

    /// Creates a temporary directory in the process temporary directory.
    ///
    /// # Parameters
    /// - `prefix`: Directory-name prefix.
    ///
    /// # Errors
    /// Returns an I/O error when the process temporary directory cannot be
    /// created, `prefix` is not a safe file-name fragment, or a unique
    /// temporary directory cannot be created.
    #[inline(always)]
    pub fn with_prefix(prefix: &str) -> Result<Self> {
        Self::in_dir(
            std::env::temp_dir(),
            Some(prefix),
            LocalFiles::DEFAULT_TEMP_FILE_RETRIES,
        )
    }

    /// Creates a temporary directory in the specified directory.
    ///
    /// # Parameters
    /// - `dir`: Parent directory in which the temporary directory is created.
    /// - `prefix`: Optional directory-name prefix.
    /// - `max_tries`: Maximum number of random names to try.
    ///
    /// # Errors
    /// Returns an I/O error when `dir` cannot be created, `prefix` is not a
    /// safe file-name fragment, the retry limit is zero, all generated names
    /// collide, or directory creation fails.
    pub fn in_dir<P>(
        dir: P,
        prefix: Option<&str>,
        max_tries: usize,
    ) -> Result<Self>
    where
        P: AsRef<Path>,
    {
        let operation_dir = absolute_path(dir.as_ref())?;
        let path = create_temp_dir_in_dir(&operation_dir, prefix, max_tries)?;
        Ok(Self { path: Some(path) })
    }

    /// Returns the absolute temporary directory path.
    ///
    /// # Returns
    /// Borrowed absolute path managed by this temporary directory.
    #[inline(always)]
    pub fn path(&self) -> &Path {
        self.path
            .as_deref()
            .expect("temporary directory path has already been released")
    }

    /// Tests whether the temporary directory path still exists.
    ///
    /// # Returns
    /// `true` when the path exists and `false` when it is missing.
    ///
    /// # Errors
    /// Returns an I/O error when the filesystem cannot determine whether the
    /// path exists. Unlike [`Path::exists`], this method does not silently map
    /// inspection errors to `false`.
    #[inline(always)]
    pub fn exists(&self) -> Result<bool> {
        LocalFiles::exists(self.path())
    }

    /// Reads metadata for the temporary directory path.
    ///
    /// # Returns
    /// Metadata for the temporary directory path.
    ///
    /// # Errors
    /// Returns the I/O error reported by [`fs::metadata`].
    #[inline(always)]
    pub fn metadata(&self) -> Result<Metadata> {
        LocalFiles::metadata(self.path())
    }

    /// Lists direct children of the temporary directory.
    ///
    /// # Returns
    /// A directory iterator over direct children.
    ///
    /// # Errors
    /// Returns the I/O error reported by [`fs::read_dir`].
    #[inline(always)]
    pub fn list(&self) -> Result<ReadDir> {
        LocalFiles::list(self.path())
    }

    /// Resolves a relative child path inside the temporary directory.
    ///
    /// The child path must contain only normal relative path components.
    /// Absolute paths, parent traversal, root or prefix components, and empty
    /// paths are rejected. This method only resolves the path; it does not
    /// create filesystem entries.
    ///
    /// This method performs lexical validation only. It does not inspect the
    /// filesystem, so an existing symbolic-link component may resolve outside
    /// the temporary directory when the returned path is used by another API.
    /// Use the open or ensure child helpers when observed symbolic links must
    /// be rejected. The returned path is not proof of filesystem containment.
    ///
    /// # Parameters
    /// - `child`: Relative child path.
    ///
    /// # Returns
    /// The absolute child path joined under this temporary directory.
    ///
    /// # Errors
    /// Returns [`ErrorKind::InvalidInput`] when `child` is not a non-empty
    /// relative path made only of normal lexical components.
    pub fn child_path<P>(&self, child: P) -> Result<PathBuf>
    where
        P: AsRef<Path>,
    {
        let child = child.as_ref();
        let _ = child_component_names(child)?;
        Ok(self.path().join(child))
    }

    /// Ensures that a child directory exists, creating missing parents.
    ///
    /// This method behaves like `mkdir -p` within the temporary directory: if
    /// `child` contains multiple nested components, every missing parent
    /// directory is created. Existing non-directory components and symbolic
    /// link components are rejected so the operation cannot leave the temporary
    /// directory through a child path.
    ///
    /// The containment guarantee assumes no untrusted process concurrently
    /// replaces checked path components. Do not use this helper as a sandbox
    /// boundary for attacker-controlled filesystem races.
    ///
    /// # Parameters
    /// - `child`: Relative child directory path.
    ///
    /// # Returns
    /// The absolute ensured child directory path.
    ///
    /// # Errors
    /// Returns an I/O error when `child` is invalid, an existing component is
    /// not a directory, a component is a symbolic link, or a directory cannot
    /// be created.
    pub fn ensure_child_dir<P>(&self, child: P) -> Result<PathBuf>
    where
        P: AsRef<Path>,
    {
        let child = child.as_ref();
        ensure_child_dir_path(self.path(), child)?;
        Ok(self.path().join(child))
    }

    /// Opens a child file for reading.
    ///
    /// The child path must resolve to a file. Directories and other non-file
    /// resources are rejected. Symbolic links are accepted only when their
    /// canonical target remains inside this temporary directory.
    ///
    /// The containment check is not atomic with opening the file and therefore
    /// does not defend against concurrent path replacement by an untrusted
    /// actor.
    ///
    /// # Parameters
    /// - `child`: Relative child file path.
    /// - `options`: Read options controlling buffering.
    ///
    /// # Returns
    /// A reader for the child file.
    ///
    /// # Errors
    /// Returns an I/O error when the child path is invalid, escapes the
    /// temporary directory, is not a file, cannot be opened, or requests an
    /// invalid buffer capacity.
    pub fn open_child_reader<P>(
        &self,
        child: P,
        options: FileReadOptions,
    ) -> Result<LocalFileReader>
    where
        P: AsRef<Path>,
    {
        let child = child.as_ref();
        let _ = self.child_path(child)?;
        let path = self.path().join(child);
        ensure_child_file_inside(self.path(), &path)?;
        LocalFiles::open_reader(path, options)
    }

    /// Opens a child file for writing.
    ///
    /// The child path must remain inside this temporary directory. When
    /// `options.creates_parent()` is enabled, missing parent directories are
    /// created with the same `mkdir -p` semantics as
    /// [`LocalTempDir::ensure_child_dir`]. Existing child targets must be
    /// regular files; final symbolic links are rejected without following
    /// them.
    ///
    /// Validation and opening are separate filesystem operations. This helper
    /// is not a sandbox boundary when an untrusted actor can mutate the tree
    /// concurrently.
    ///
    /// # Parameters
    /// - `child`: Relative child file path.
    /// - `options`: Write options controlling parent creation, write mode, and
    ///   buffering.
    ///
    /// # Returns
    /// A writer for the child file.
    ///
    /// # Errors
    /// Returns an I/O error when the child path is invalid, parent directories
    /// cannot be created, the child would escape the temporary directory, the
    /// target is not a file, or the file cannot be opened with the requested
    /// mode.
    pub fn open_child_writer<P>(
        &self,
        child: P,
        options: FileWriteOptions,
    ) -> Result<LocalFileWriter>
    where
        P: AsRef<Path>,
    {
        let child = child.as_ref();
        let _ = self.child_path(child)?;
        let path = self.path().join(child);
        prepare_child_writer_path(
            self.path(),
            child,
            &path,
            options.creates_parent(),
        )?;
        LocalFiles::open_writer(path, options)
    }

    /// Removes the temporary directory immediately.
    ///
    /// This consumes the guard and disables the later best-effort cleanup in
    /// `Drop` after removal succeeds. If removal fails, the guard still owns
    /// the path and will attempt best-effort cleanup when dropped.
    ///
    /// # Errors
    /// Returns the I/O error reported by [`fs::remove_dir_all`].
    pub fn cleanup(mut self) -> Result<()> {
        let path = self.path().to_path_buf();
        fs::remove_dir_all(&path)?;
        let _ = self.path.take();
        Ok(())
    }

    /// Keeps the temporary directory at its generated path.
    ///
    /// This consumes the guard and disables automatic cleanup.
    ///
    /// # Returns
    /// The absolute generated temporary directory path.
    #[inline]
    pub fn keep(mut self) -> PathBuf {
        self.path
            .take()
            .expect("temporary directory path has already been released")
    }

    /// Moves the temporary directory to a final path.
    ///
    /// Parent directories for `target` are created before a native no-replace
    /// move. If persistence fails, the returned [`LocalPersistError`] retains
    /// this guard so the caller can retry, keep, inspect, or explicitly clean
    /// up the temporary directory.
    ///
    /// Persistence uses a native move or rename and does not fall back to
    /// copying and deleting. Moving across filesystems can therefore fail with
    /// `EXDEV` on Unix or a platform-equivalent error.
    /// A relative target is bound to the process current directory when this
    /// method begins, and the returned path is absolute. On Windows, no
    /// verbatim-path prefix is added, so native path-length and verbatim-path
    /// semantics still apply.
    ///
    /// # Parameters
    /// - `target`: Final directory path.
    ///
    /// # Returns
    /// The absolute final directory path.
    ///
    /// # Errors
    /// Returns [`LocalPersistError`] when the parent directory cannot be
    /// created, the target already exists, the platform lacks a native
    /// no-replace directory move, or the temporary directory cannot be moved to
    /// `target`.
    pub fn persist<P>(
        mut self,
        target: P,
    ) -> std::result::Result<PathBuf, LocalPersistError<Self>>
    where
        P: AsRef<Path>,
    {
        let target = match absolute_path(target.as_ref()) {
            Ok(path) => path,
            Err(error) => return Err(LocalPersistError::new(error, self)),
        };
        if let Err(error) = LocalFiles::ensure_parent(&target) {
            return Err(LocalPersistError::new(error, self));
        }
        let move_result = {
            let source = self
                .path
                .as_ref()
                .expect("temporary directory path has already been released");
            move_directory_without_replacing(source, &target)
        };
        if let Err(error) = move_result {
            return Err(LocalPersistError::new(error, self));
        }
        let _ = self.path.take();
        Ok(target)
    }
}

/// Returns normal components from a safe relative child path.
///
/// # Parameters
/// - `child`: Child path to validate.
///
/// # Returns
/// Normal path components copied from `child`.
///
/// # Errors
/// Returns [`ErrorKind::InvalidInput`] when `child` is empty or contains any
/// component other than a normal relative component.
fn child_component_names(child: &Path) -> Result<Vec<OsString>> {
    let relative = LocalRelativePath::new(child).map_err(|_| {
        Error::new(
            ErrorKind::InvalidInput,
            format!(
                "child path must be relative and safe: {}",
                child.display()
            ),
        )
    })?;
    Ok(relative
        .as_path()
        .components()
        .map(|component| component.as_os_str().to_os_string())
        .collect())
}

/// Ensures a child directory under a root directory.
///
/// # Parameters
/// - `root`: Root temporary directory.
/// - `child`: Relative child directory path.
///
/// # Returns
/// The ensured child directory path.
///
/// # Errors
/// Returns an I/O error when the child path is invalid, crosses a symbolic
/// link, contains a non-directory component, or cannot be created.
fn ensure_child_dir_path(root: &Path, child: &Path) -> Result<PathBuf> {
    let components = child_component_names(child)?;
    let mut path = root.to_path_buf();
    for name in components {
        path.push(name);
        match fs::symlink_metadata(&path) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(Error::new(
                    ErrorKind::InvalidInput,
                    format!(
                        "child directory crosses a symbolic link: {}",
                        path.display()
                    ),
                ));
            }
            Ok(metadata) if metadata.is_dir() => {}
            Ok(_) => {
                return Err(Error::new(
                    ErrorKind::AlreadyExists,
                    format!(
                        "child path component is not a directory: {}",
                        path.display()
                    ),
                ));
            }
            Err(error) if error.kind() == ErrorKind::NotFound => {
                create_private_dir(&path)?
            }
            Err(error) => return Err(error),
        }
    }
    Ok(path)
}

/// Ensures an existing child file remains inside the root.
///
/// # Parameters
/// - `root`: Root temporary directory.
/// - `path`: Child file path to inspect.
///
/// # Errors
/// Returns an I/O error when `path` is not a file or its canonical path leaves
/// `root`.
fn ensure_child_file_inside(root: &Path, path: &Path) -> Result<()> {
    let metadata = fs::metadata(path)?;
    if !metadata.is_file() {
        return Err(Error::new(
            ErrorKind::InvalidInput,
            format!("child path is not a file: {}", path.display()),
        ));
    }
    ensure_existing_path_inside(root, path)
}

/// Prepares and validates a child writer path.
///
/// # Parameters
/// - `root`: Root temporary directory.
/// - `child`: Relative child file path.
/// - `path`: Joined child file path.
/// - `create_parent`: Whether missing parents should be created.
///
/// # Errors
/// Returns an I/O error when parents are missing, cannot be created, or the
/// target would escape `root`, or the final target is a symbolic link.
fn prepare_child_writer_path(
    root: &Path,
    child: &Path,
    path: &Path,
    create_parent: bool,
) -> Result<()> {
    if let Some(parent) = child.parent()
        && !parent.as_os_str().is_empty()
    {
        let parent_path = root.join(parent);
        if create_parent {
            ensure_child_dir_path(root, parent)?;
        }
        ensure_existing_path_inside(root, &parent_path)?;
    }

    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.file_type().is_symlink() => Err(Error::new(
            ErrorKind::InvalidInput,
            format!("child file target is a symbolic link: {}", path.display()),
        )),
        Ok(metadata) if metadata.is_file() => {
            ensure_existing_path_inside(root, path)
        }
        Ok(_) => Err(Error::new(
            ErrorKind::InvalidInput,
            format!("child path is not a file: {}", path.display()),
        )),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(()),
        Err(error) => Err(error),
    }
}

/// Ensures an existing path canonicalizes under a root directory.
///
/// # Parameters
/// - `root`: Root directory.
/// - `path`: Existing path to inspect.
///
/// # Errors
/// Returns an I/O error when either path cannot be canonicalized or `path`
/// canonicalizes outside `root`.
fn ensure_existing_path_inside(root: &Path, path: &Path) -> Result<()> {
    let root = fs::canonicalize(root)?;
    let path = fs::canonicalize(path)?;
    if !path.starts_with(&root) {
        return Err(Error::new(
            ErrorKind::InvalidInput,
            format!(
                "child path escapes temporary directory: {}",
                path.display()
            ),
        ));
    }
    Ok(())
}

impl Drop for LocalTempDir {
    /// Removes the temporary directory unless ownership has been released.
    fn drop(&mut self) {
        if let Some(path) = self.path.take()
            && let Err(error) = fs::remove_dir_all(&path)
        {
            warn!(
                "failed to remove temporary directory {}: {}",
                path.display(),
                error
            );
        }
    }
}
