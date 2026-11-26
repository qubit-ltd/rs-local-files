// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Shared temporary-resource parent preparation.

use std::fs;
use std::io;
use std::path::Path;
use std::path::PathBuf;

/// Prepares a host target parent before any publication attempt.
///
/// Creates missing ancestors only when `create_parent` is true and returns
/// directories requiring later namespace synchronization. Otherwise verifies
/// the existing parent is a directory. Returns `InvalidInput` for a target
/// without a parent, `NotADirectory` for a non-directory parent, or a native
/// inspection/creation error. Created ancestors remain after later failures.
///
/// # Parameters
///
/// - `target`: Host path whose parent is prepared.
/// - `create_parent`: Whether missing ancestors may be created.
#[inline]
pub(crate) fn host(target: &Path, create_parent: bool) -> io::Result<Vec<PathBuf>> {
    let parent = target
        .parent()
        .ok_or(io::Error::new(io::ErrorKind::InvalidInput, "target has no parent"))?;
    if parent.as_os_str().is_empty() {
        return Ok(Vec::new());
    }
    if create_parent {
        return crate::local::ensure_parent_path_with_sync_dirs(target);
    }
    if fs::metadata(parent)?.is_dir() {
        Ok(Vec::new())
    } else {
        Err(io::Error::from(io::ErrorKind::NotADirectory))
    }
}

/// Prepares a rooted target parent before any publication attempt.
///
/// Creates missing ancestors when `create_parent` is true; otherwise verifies
/// the parent directory without following its final link. A direct child uses
/// the retained root. Returns path-validation, native traversal/creation, or
/// `NotADirectory` errors. Successfully created ancestors are not rolled back.
///
/// # Parameters
///
/// - `root`: Retained authority used to inspect or create parent directories.
/// - `target`: Root-relative target whose parent is prepared.
/// - `create_parent`: Whether missing ancestors may be created.
#[inline]
pub(crate) fn rooted(
    root: &crate::rooted::Root,
    target: &crate::local::LocalRelativePath,
    create_parent: bool,
) -> io::Result<bool> {
    let Some(parent) = target.as_path().parent() else {
        return Ok(false);
    };
    if parent.as_os_str().is_empty() {
        return Ok(false);
    }
    let parent = crate::local::LocalRelativePath::new(parent)?;
    if create_parent {
        let mut prefixes = Vec::new();
        let mut prefix = PathBuf::new();
        for component in parent.as_path().components() {
            prefix.push(component.as_os_str());
            prefixes.push(prefix.clone());
        }
        let mut missing = Vec::new();
        for prefix in prefixes {
            let relative = crate::local::LocalRelativePath::new(&prefix)?;
            match root.symlink_metadata(&relative) {
                Ok(metadata) if metadata.kind() == crate::rooted::EntryKind::Directory => {}
                Ok(_) => return Err(io::Error::from(io::ErrorKind::NotADirectory)),
                Err(error) if error.kind() == io::ErrorKind::NotFound => missing.push(prefix),
                Err(error) => return Err(error),
            }
        }
        let mut created = false;
        for prefix in missing {
            let relative = crate::local::LocalRelativePath::new(&prefix)?;
            match root.create_dir(&relative) {
                Ok(()) => created = true,
                Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                    if !root
                        .symlink_metadata(&relative)
                        .is_ok_and(|metadata| metadata.kind() == crate::rooted::EntryKind::Directory)
                    {
                        return Err(error);
                    }
                }
                Err(error) => return Err(error),
            }
        }
        Ok(created)
    } else {
        let metadata = root.symlink_metadata(&parent)?;
        if metadata.kind() == crate::rooted::EntryKind::Directory {
            Ok(false)
        } else {
            Err(io::Error::from(io::ErrorKind::NotADirectory))
        }
    }
}

/// Injects a deterministic pre-install conflict after parent preparation.
#[inline]
pub(crate) fn persist_install_fault() -> Option<io::Error> {
    #[cfg(feature = "test-support")]
    if crate::local::take_test_support("temp-persist-install") {
        return Some(io::Error::new(
            io::ErrorKind::AlreadyExists,
            "injected persistence install conflict",
        ));
    }
    None
}
