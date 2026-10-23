// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Thread-isolated temporary cleanup failure boundaries.
// Covered through public temporary cleanup fault-injection tests.

#[cfg(unix)]
use std::fs::File;
use std::io;
#[cfg(unix)]
use std::path::Path;

#[cfg(unix)]
use qubit_io::Output;

use crate::LocalRelativePath;
use crate::local::create_rooted_directory;
use crate::local::create_rooted_symlink;
use crate::local::open_rooted_native_writer;
use crate::local::remove_rooted_entry;
use crate::local::take_test_support;
use crate::local::take_test_support_on_nth;
use crate::local::test_fault_error;
use crate::write::OpenOptions;

/// Fails the second removal after one successful leaf removal, retaining the
/// original native cause. No effect occurs without a matching scoped selector.
pub(crate) fn temp_cleanup_before_remove() -> io::Result<()> {
    if take_test_support_on_nth("temp-directory-remove-second", 2) {
        return Err(test_fault_error());
    }
    Ok(())
}

/// Simulates disappearance of the first queued child after root enumeration.
/// Only the owner of the scoped fault selector observes the error.
pub(crate) fn temp_cleanup_metadata() -> io::Result<()> {
    if take_test_support_on_nth("temp-directory-child-not-found", 2) {
        return Err(io::Error::from(io::ErrorKind::NotFound));
    }
    Ok(())
}

/// Selects a one-shot concurrent directory addition at the final unlink.
pub(crate) fn temp_cleanup_add_child() -> bool {
    take_test_support("temp-directory-concurrent-child")
}

/// Expires the shared deadline at the sandbox boundary of an empty-tree
/// cleanup. Counts checks through the same scoped selector as existing delete
/// faults.
pub(crate) fn temp_cleanup_deadline_expired() -> bool {
    take_test_support_on_nth("local-delete-deadline-5", 5)
}

/// Replaces an entry after its type was observed, through the original root.
/// This deterministic race affects only the scoped selector's owning thread.
/// Native replacement setup errors propagate without concealing their cause.
#[cfg(unix)]
pub(crate) fn temp_cleanup_replace_observed_entry(
    root: &File,
    diagnostic_root: &Path,
    path: &LocalRelativePath,
    directory: bool,
) -> io::Result<()> {
    if !directory && take_test_support("temp-observed-file-becomes-directory") {
        remove_rooted_entry(root, diagnostic_root, path)?;
        return create_rooted_directory(root, diagnostic_root, path, false, false);
    }
    if directory && take_test_support("temp-observed-directory-becomes-file") {
        remove_rooted_entry(root, diagnostic_root, path)?;
        return open_rooted_native_writer(root, diagnostic_root, path, &OpenOptions::default())?
            .write_fully(b"replacement");
    }
    if directory && take_test_support("temp-observed-directory-becomes-symlink") {
        remove_rooted_entry(root, diagnostic_root, path)?;
        return create_rooted_symlink(root, diagnostic_root, Path::new("../../outside"), path);
    }
    Ok(())
}
