// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Shared temporary-resource parent preparation.

use std::io;
use std::path::Path;

/// Prepares a host target parent before any publication attempt.
#[inline]
pub(crate) fn host(target: &Path, create_parent: bool) -> io::Result<()> {
    let parent = target
        .parent()
        .ok_or_else(|| io::Error::new(io::ErrorKind::InvalidInput, "target has no parent"))?;
    if parent.as_os_str().is_empty() {
        return Ok(());
    }
    if create_parent {
        return std::fs::create_dir_all(parent);
    }
    if std::fs::metadata(parent)?.is_dir() {
        Ok(())
    } else {
        Err(io::Error::from(io::ErrorKind::NotADirectory))
    }
}

/// Prepares a rooted target parent before any publication attempt.
#[inline]
pub(crate) fn rooted(
    root: &crate::rooted::Root,
    target: &crate::local::LocalRelativePath,
    create_parent: bool,
) -> io::Result<()> {
    let Some(parent) = target.as_path().parent() else {
        return Ok(());
    };
    if parent.as_os_str().is_empty() {
        return Ok(());
    }
    let parent = crate::local::LocalRelativePath::new(parent)?;
    if create_parent {
        root.create_dir_all(&parent)
    } else {
        let metadata = root.symlink_metadata(&parent)?;
        if metadata.kind() == crate::rooted::EntryKind::Directory {
            Ok(())
        } else {
            Err(io::Error::from(io::ErrorKind::NotADirectory))
        }
    }
}
