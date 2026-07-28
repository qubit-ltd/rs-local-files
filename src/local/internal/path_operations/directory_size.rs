// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Private iterative directory-size traversal.
// qubit-style: allow coverage-cfg
// qubit-style: allow source-test-pair

use std::collections::HashSet;
use std::fs;
use std::io::{Error, ErrorKind, Result};
use std::path::Path;

#[cfg(coverage)]
use super::super::coverage_fault;
use super::super::dir_size_frame::DirSizeFrame;
use super::super::directory_identity::DirectoryIdentity;

// A portable test fixture cannot reliably provision more than `u64::MAX` bytes
// of aggregate file length. Keep construction inline so that limitation does
// not create an otherwise unreachable helper function in coverage data.
macro_rules! dir_size_overflow_error {
    ($path:expr) => {
        Error::new(
            ErrorKind::InvalidData,
            format!("directory size exceeds u64 at {}", $path.display()),
        )
    };
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
pub(crate) fn dir_size_path(path: &Path) -> Result<u64> {
    let metadata = fs::symlink_metadata(path)?;
    if !metadata.is_dir() || metadata.file_type().is_symlink() {
        return Err(Error::new(
            ErrorKind::InvalidInput,
            format!("path is not a directory: {}", path.display()),
        ));
    }
    let canonical_path = fs::canonicalize(path)?;
    let root_identity = DirectoryIdentity::from_metadata(&metadata, &canonical_path);
    dir_size_iterative(path, root_identity)
}

/// Computes regular-file sizes below a directory without recursive calls.
///
/// # Parameters
/// - `path`: Directory path to measure.
/// - `root_identity`: Stable identity of the traversal root.
///
/// # Returns
/// The total byte length of regular files under `path`.
///
/// # Errors
/// Returns an I/O error when a directory entry cannot be read, or
/// [`ErrorKind::InvalidData`] when the aggregate exceeds [`u64::MAX`].
///
/// # Panics
/// Panics if the iterative traversal loses its root frame.
fn dir_size_iterative(path: &Path, root_identity: DirectoryIdentity) -> Result<u64> {
    let mut active_directories = HashSet::new();
    let _ = active_directories.insert(root_identity.clone());
    let mut directories = vec![DirSizeFrame::new(
        path.to_path_buf(),
        root_identity,
        fs::read_dir(path)?,
    )];
    loop {
        let current = directories
            .last_mut()
            .expect("directory-size traversal should retain its root frame");
        let entry = next_dir_size_entry(current);
        let Some(entry) = entry else {
            let completed = directories
                .pop()
                .expect("directory-size traversal should retain its root frame");
            let (completed_path, completed_identity, completed_size) = completed.into_parts();
            let _ = active_directories.remove(&completed_identity);
            let Some(parent) = directories.last_mut() else {
                return Ok(completed_size);
            };
            let parent_size = checked_dir_size_add(
                parent.size(),
                completed_size,
                &completed_path,
                "dir-size-directory-overflow",
            )?;
            parent.set_size(parent_size);
            continue;
        };
        let entry = entry?;
        let entry_path = entry.path();
        let metadata = read_dir_size_metadata(&entry_path)?;
        let file_type = metadata.file_type();
        if file_type.is_symlink() {
            continue;
        }
        if metadata.is_dir() {
            let canonical_path = fs::canonicalize(&entry_path)?;
            let identity = DirectoryIdentity::from_metadata(&metadata, &canonical_path);
            if active_directories.contains(&identity) {
                return Err(Error::new(
                    ErrorKind::InvalidInput,
                    format!(
                        "directory cycle detected while measuring size: {}",
                        entry_path.display(),
                    ),
                ));
            }
            let entries = read_dir_size_entries(&entry_path)?;
            let _ = active_directories.insert(identity.clone());
            directories.push(DirSizeFrame::new(entry_path, identity, entries));
        } else if metadata.is_file() {
            let current = directories
                .last_mut()
                .expect("directory-size traversal should retain its root frame");
            let current_size = checked_dir_size_add(
                current.size(),
                metadata.len(),
                &entry_path,
                "dir-size-file-overflow",
            )?;
            current.set_size(current_size);
        }
    }
}

/// Reads the next directory-size entry and applies the isolated entry fault.
///
/// # Parameters
/// - `frame`: Active traversal frame whose iterator should advance.
///
/// # Returns
/// The next entry result, or `None` when the directory is exhausted.
fn next_dir_size_entry(frame: &mut DirSizeFrame) -> Option<Result<fs::DirEntry>> {
    let entry = frame.next_entry();
    #[cfg(coverage)]
    let entry = entry.map(|entry| {
        if coverage_fault::is_enabled("dir-size-entry") {
            Err(Error::from_raw_os_error(libc::EIO))
        } else {
            entry
        }
    });
    entry
}

/// Reads metadata for one entry during directory-size traversal.
///
/// # Parameters
/// - `path`: Entry path whose metadata should be inspected.
///
/// # Returns
/// Metadata obtained without following the final symbolic link.
///
/// # Errors
/// Returns the native metadata error or the selected coverage fault.
fn read_dir_size_metadata(path: &Path) -> Result<fs::Metadata> {
    #[cfg(coverage)]
    if coverage_fault::is_enabled("dir-size-metadata") {
        return Err(Error::from_raw_os_error(libc::EIO));
    }
    fs::symlink_metadata(path)
}

/// Opens a child directory iterator during directory-size traversal.
///
/// # Parameters
/// - `path`: Directory entry to traverse.
///
/// # Returns
/// Iterator over direct entries below `path`.
///
/// # Errors
/// Returns the native directory-open error or the selected coverage fault.
fn read_dir_size_entries(path: &Path) -> Result<fs::ReadDir> {
    #[cfg(coverage)]
    if coverage_fault::is_enabled("dir-size-read-dir") {
        return Err(Error::from_raw_os_error(libc::EIO));
    }
    fs::read_dir(path)
}

/// Adds one directory-size subtotal with checked overflow handling.
///
/// # Parameters
/// - `current`: Size accumulated before this entry or child directory.
/// - `additional`: Size contributed by the entry or completed child.
/// - `path`: Path reported when the aggregate cannot be represented.
/// - `overflow_fault`: Coverage-only overflow fault for this addition phase.
///
/// # Returns
/// Exact aggregate size.
///
/// # Errors
/// Returns [`ErrorKind::InvalidData`] when the aggregate exceeds [`u64::MAX`]
/// or the corresponding coverage fault is selected.
fn checked_dir_size_add(
    current: u64,
    additional: u64,
    path: &Path,
    overflow_fault: &str,
) -> Result<u64> {
    let total = current.checked_add(additional);
    #[cfg(coverage)]
    let total = if coverage_fault::is_enabled(overflow_fault) {
        None
    } else {
        total
    };
    #[cfg(not(coverage))]
    let _ = overflow_fault;
    total.ok_or_else(|| dir_size_overflow_error!(path))
}
