// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Thread-isolated temporary cleanup failure boundaries.
// qubit-style: allow source-test-pair
// Covered through public temporary cleanup fault-injection tests.

use std::io;

/// Fails the second removal after one successful leaf removal, retaining the
/// original native cause. No effect occurs without a matching scoped selector.
pub(crate) fn temp_cleanup_before_remove() -> io::Result<()> {
    if crate::local::take_test_support_on_nth("temp-directory-remove-second", 2) {
        return Err(crate::local::test_fault_error());
    }
    Ok(())
}

/// Simulates disappearance of the first queued child after root enumeration.
/// Only the owner of the scoped fault selector observes the error.
pub(crate) fn temp_cleanup_metadata() -> io::Result<()> {
    if crate::local::take_test_support_on_nth("temp-directory-child-not-found", 2) {
        return Err(io::Error::from(io::ErrorKind::NotFound));
    }
    Ok(())
}

/// Selects a one-shot concurrent directory addition at the final unlink.
pub(crate) fn temp_cleanup_add_child() -> bool {
    crate::local::take_test_support("temp-directory-concurrent-child")
}

/// Expires the shared deadline at the sandbox boundary of an empty-tree
/// cleanup. Counts checks through the same scoped selector as existing delete
/// faults.
pub(crate) fn temp_cleanup_deadline_expired() -> bool {
    crate::local::take_test_support_on_nth("local-delete-deadline-5", 5)
}

/// Replaces an entry after its type was observed, through the original root.
/// This deterministic race affects only the scoped selector's owning thread.
/// Native replacement setup errors propagate without concealing their cause.
#[cfg(unix)]
pub(crate) fn temp_cleanup_replace_observed_entry(
    root: &std::fs::File,
    diagnostic_root: &std::path::Path,
    path: &crate::LocalRelativePath,
    directory: bool,
) -> io::Result<()> {
    if !directory && crate::local::take_test_support("temp-observed-file-becomes-directory") {
        crate::local::remove_rooted_entry(root, diagnostic_root, path)?;
        return crate::local::create_rooted_directory(root, diagnostic_root, path, false, false);
    }
    if directory && crate::local::take_test_support("temp-observed-directory-becomes-file") {
        use std::io::Write;
        crate::local::remove_rooted_entry(root, diagnostic_root, path)?;
        return crate::local::open_rooted_native_writer(
            root,
            diagnostic_root,
            path,
            &crate::write::OpenOptions::default(),
        )?
        .write_all(b"replacement");
    }
    if directory && crate::local::take_test_support("temp-observed-directory-becomes-symlink") {
        crate::local::remove_rooted_entry(root, diagnostic_root, path)?;
        return crate::local::create_rooted_symlink(root, diagnostic_root, std::path::Path::new("../../outside"), path);
    }
    Ok(())
}
