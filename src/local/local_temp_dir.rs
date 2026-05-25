/*******************************************************************************
 *
 *    Copyright (c) 2026 Haixing Hu.
 *
 *    SPDX-License-Identifier: Apache-2.0
 *
 *    Licensed under the Apache License, Version 2.0.
 *
 ******************************************************************************/
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
    Component,
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
};

use super::local_files::create_temp_dir_in_dir;

/// Temporary directory that is removed automatically unless kept or persisted.
///
/// `LocalTempDir` owns a directory path and removes that directory tree when the
/// object is dropped. Use [`LocalTempDir::keep`] to keep the temporary directory at
/// its generated path, or [`LocalTempDir::persist`] to move it to a final path.
///
/// Cleanup performed from `Drop` is best-effort. If removal fails, the failure
/// is reported through the `log` facade at warning level and the program is not
/// panicked.
#[derive(Debug)]
pub struct LocalTempDir {
    path: Option<PathBuf>,
}

impl LocalTempDir {
    /// Creates a temporary directory in the process temporary directory.
    ///
    /// # Errors
    /// Returns an I/O error when the process temporary directory cannot be
    /// created or a unique temporary directory cannot be created.
    #[inline]
    pub fn new() -> Result<Self> {
        Self::with_prefix(None)
    }

    /// Creates a temporary directory in the process temporary directory.
    ///
    /// # Parameters
    /// - `prefix`: Optional directory-name prefix.
    ///
    /// # Errors
    /// Returns an I/O error when the process temporary directory cannot be
    /// created, `prefix` is not a safe file-name fragment, or a unique
    /// temporary directory cannot be created.
    #[inline]
    pub fn with_prefix(prefix: Option<&str>) -> Result<Self> {
        Self::in_dir(std::env::temp_dir(), prefix, LocalFiles::DEFAULT_TEMP_FILE_RETRIES)
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
    pub fn in_dir<P>(dir: P, prefix: Option<&str>, max_tries: usize) -> Result<Self>
    where
        P: AsRef<Path>,
    {
        let path = create_temp_dir_in_dir(dir.as_ref(), prefix, max_tries)?;
        Ok(Self { path: Some(path) })
    }

    /// Returns the temporary directory path.
    ///
    /// # Returns
    /// Borrowed path managed by this temporary directory.
    #[inline]
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
    #[inline]
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
    #[inline]
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
    #[inline]
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
    /// # Parameters
    /// - `child`: Relative child path.
    ///
    /// # Returns
    /// The child path joined under this temporary directory.
    ///
    /// # Errors
    /// Returns [`ErrorKind::InvalidInput`] when `child` is not a safe relative
    /// path.
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
    /// # Parameters
    /// - `child`: Relative child directory path.
    ///
    /// # Returns
    /// The ensured child directory path.
    ///
    /// # Errors
    /// Returns an I/O error when `child` is invalid, an existing component is
    /// not a directory, a component is a symbolic link, or a directory cannot
    /// be created.
    pub fn ensure_child_dir<P>(&self, child: P) -> Result<PathBuf>
    where
        P: AsRef<Path>,
    {
        ensure_child_dir_path(self.path(), child.as_ref())
    }

    /// Opens a child file for reading.
    ///
    /// The child path must resolve to a file. Directories and other non-file
    /// resources are rejected. Symbolic links are accepted only when their
    /// canonical target remains inside this temporary directory.
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
    pub fn open_child_reader<P>(&self, child: P, options: FileReadOptions) -> Result<LocalFileReader>
    where
        P: AsRef<Path>,
    {
        let path = self.child_path(child)?;
        ensure_child_file_inside(self.path(), &path)?;
        LocalFiles::open_reader(path, options)
    }

    /// Opens a child file for writing.
    ///
    /// The child path must remain inside this temporary directory. When
    /// `options.create_parent` is enabled, missing parent directories are
    /// created with the same `mkdir -p` semantics as
    /// [`LocalTempDir::ensure_child_dir`]. Existing child targets must be files
    /// if they already exist.
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
    pub fn open_child_writer<P>(&self, child: P, options: FileWriteOptions) -> Result<LocalFileWriter>
    where
        P: AsRef<Path>,
    {
        let child = child.as_ref();
        let path = self.child_path(child)?;
        prepare_child_writer_path(self.path(), child, &path, options.create_parent)?;
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
    /// The generated temporary directory path.
    #[inline]
    pub fn keep(mut self) -> PathBuf {
        self.path
            .take()
            .expect("temporary directory path has already been released")
    }

    /// Moves the temporary directory to a final path.
    ///
    /// Parent directories for `target` are created before renaming. If the
    /// rename fails, the temporary directory remains owned by this guard and is
    /// cleaned up when the guard is dropped.
    ///
    /// # Parameters
    /// - `target`: Final directory path.
    ///
    /// # Returns
    /// The final directory path.
    ///
    /// # Errors
    /// Returns an I/O error when the parent directory cannot be created, the
    /// target already exists, or the temporary directory cannot be renamed to
    /// `target`.
    pub fn persist<P>(mut self, target: P) -> Result<PathBuf>
    where
        P: AsRef<Path>,
    {
        let target = target.as_ref().to_path_buf();
        LocalFiles::ensure_parent(&target)?;
        match fs::symlink_metadata(&target) {
            Ok(_) => {
                return Err(Error::new(
                    ErrorKind::AlreadyExists,
                    format!("target already exists: {}", target.display()),
                ));
            }
            Err(error) if error.kind() == ErrorKind::NotFound => {}
            Err(error) => return Err(error),
        }
        let source = self
            .path
            .as_ref()
            .expect("temporary directory path has already been released");
        fs::rename(source, &target)?;
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
    let mut components = Vec::new();
    for component in child.components() {
        match component {
            Component::Normal(name) => components.push(name.to_os_string()),
            _ => {
                return Err(Error::new(
                    ErrorKind::InvalidInput,
                    format!("child path must be relative and safe: {}", child.display()),
                ));
            }
        }
    }
    if components.is_empty() {
        return Err(Error::new(ErrorKind::InvalidInput, "child path must not be empty"));
    }
    Ok(components)
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
                    format!("child directory crosses a symbolic link: {}", path.display()),
                ));
            }
            Ok(metadata) if metadata.is_dir() => {}
            Ok(_) => {
                return Err(Error::new(
                    ErrorKind::AlreadyExists,
                    format!("child path component is not a directory: {}", path.display()),
                ));
            }
            Err(error) if error.kind() == ErrorKind::NotFound => fs::create_dir(&path)?,
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
/// target would escape `root`.
fn prepare_child_writer_path(root: &Path, child: &Path, path: &Path, create_parent: bool) -> Result<()> {
    if let Some(parent) = child.parent()
        && !parent.as_os_str().is_empty()
    {
        let parent_path = root.join(parent);
        if create_parent {
            ensure_child_dir_path(root, parent)?;
        }
        ensure_existing_path_inside(root, &parent_path)?;
    }

    match fs::metadata(path) {
        Ok(metadata) if metadata.is_file() => ensure_existing_path_inside(root, path),
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
            format!("child path escapes temporary directory: {}", path.display()),
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
            warn!("failed to remove temporary directory {}: {}", path.display(), error);
        }
    }
}
