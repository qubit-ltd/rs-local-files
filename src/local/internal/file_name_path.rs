// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Private lexical path-like file-name helpers.
// qubit-style: allow source-test-pair

/// Removes one leading dot from an extension argument.
///
/// # Parameters
/// - `extension`: Extension argument supplied by a caller.
///
/// # Returns
/// The extension without one leading dot.
#[must_use]
pub(crate) fn normalize_extension(extension: &str) -> &str {
    extension.strip_prefix('.').unwrap_or(extension)
}

/// Returns the final segment from a path-like string.
///
/// # Parameters
/// - `path`: Path-like string to inspect.
///
/// # Returns
/// The substring after the final slash or backslash, or the original string
/// when no separator is present.
#[must_use]
#[inline]
pub(crate) fn file_name_from_path(path: &str) -> &str {
    match path.rfind(['/', '\\']) {
        Some(index) => &path[index + 1..],
        None => path,
    }
}
