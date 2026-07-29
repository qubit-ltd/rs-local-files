// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Random and lexical file-name helpers.

use std::ffi::OsStr;
use std::io::Result;
use std::path::Path;

use crate::local::{
    file_name_from_path as extract_file_name_from_path,
    file_name_from_url as extract_file_name_from_url, normalize_extension, try_random_file_name,
    validate_portable_file_name_impl,
};

/// Default prefix used by random file-name generation.
pub const DEFAULT_RANDOM_FILE_NAME_PREFIX: &str = "qubit-local-files-";

/// Builds a random file-name component using the default prefix.
///
/// # Returns
/// A random, single-component file name.
///
/// # Errors
/// Returns an I/O error when the operating-system random source fails.
#[inline(always)]
pub fn random_file_name() -> Result<String> {
    random_file_name_with(None, None)
}

/// Builds a random file-name component with optional affixes.
///
/// # Parameters
/// - `prefix`: Optional prefix; the default random prefix is used when absent.
/// - `suffix`: Optional suffix; no suffix is used when absent.
///
/// # Returns
/// A random, single-component file name.
///
/// # Errors
/// Returns [`std::io::ErrorKind::InvalidInput`] when either affix is not a
/// safe file-name fragment, or an I/O error when the operating-system random
/// source fails.
#[inline(always)]
pub fn random_file_name_with(prefix: Option<&str>, suffix: Option<&str>) -> Result<String> {
    try_random_file_name(DEFAULT_RANDOM_FILE_NAME_PREFIX, prefix, suffix)
}

/// Validates one portable file-name component.
///
/// # Parameters
/// - `name`: UTF-8 file-name component to validate.
///
/// # Errors
/// Returns [`std::io::ErrorKind::InvalidInput`] when `name` is empty,
/// composite, reserved, too long, or contains a non-portable character or
/// suffix.
#[inline(always)]
pub fn validate_portable_file_name(name: &str) -> Result<()> {
    validate_portable_file_name_impl(name)
}

/// Returns the final file-name component as UTF-8.
///
/// # Parameters
/// - `path`: Path to inspect.
///
/// # Returns
/// The final component, or `None` when absent or not valid UTF-8.
#[must_use]
#[inline(always)]
pub fn file_name(path: &Path) -> Option<&str> {
    path.file_name().and_then(OsStr::to_str)
}

/// Returns the file stem as UTF-8.
///
/// # Parameters
/// - `path`: Path to inspect.
///
/// # Returns
/// The stem, or `None` when absent or not valid UTF-8.
#[must_use]
#[inline(always)]
pub fn file_stem(path: &Path) -> Option<&str> {
    path.file_stem().and_then(OsStr::to_str)
}

/// Returns the file prefix as UTF-8.
///
/// # Parameters
/// - `path`: Path to inspect.
///
/// # Returns
/// The prefix, or `None` when absent or not valid UTF-8.
#[must_use]
#[inline(always)]
pub fn file_prefix(path: &Path) -> Option<&str> {
    path.file_prefix().and_then(OsStr::to_str)
}

/// Returns the final extension without its leading dot.
///
/// # Parameters
/// - `path`: Path to inspect.
///
/// # Returns
/// The extension, or `None` when absent or not valid UTF-8.
#[must_use]
#[inline(always)]
pub fn extension(path: &Path) -> Option<&str> {
    path.extension().and_then(OsStr::to_str)
}

/// Returns the final extension with a leading dot.
///
/// # Parameters
/// - `path`: Path to inspect.
///
/// # Returns
/// The dotted extension, an empty string for an empty extension, or `None`
/// when no valid UTF-8 extension exists.
#[must_use]
#[inline]
pub fn dot_extension(path: &Path) -> Option<String> {
    extension(path).map(|value| {
        if value.is_empty() {
            String::new()
        } else {
            format!(".{value}")
        }
    })
}

/// Tests whether a path has the specified final extension.
///
/// # Parameters
/// - `path`: Path to inspect.
/// - `expected`: Expected extension with or without a leading dot.
///
/// # Returns
/// `true` when the final extension matches exactly.
#[must_use]
#[inline(always)]
pub fn has_extension(path: &Path, expected: &str) -> bool {
    extension(path) == Some(normalize_extension(expected))
}

/// Tests whether a path has an ASCII-case-insensitive final extension.
///
/// # Parameters
/// - `path`: Path to inspect.
/// - `expected`: Expected extension with or without a leading dot.
///
/// # Returns
/// `true` when the final extension matches after ASCII case folding.
#[must_use]
#[inline]
pub fn has_extension_ignore_ascii_case(path: &Path, expected: &str) -> bool {
    extension(path).is_some_and(|value| value.eq_ignore_ascii_case(normalize_extension(expected)))
}

/// Returns the final segment from a path-like string.
///
/// # Parameters
/// - `path`: Lexical path containing slash or backslash separators.
///
/// # Returns
/// The substring after the final separator, or the original string when no
/// separator exists.
#[must_use]
#[inline(always)]
pub fn file_name_from_path(path: &str) -> &str {
    extract_file_name_from_path(path)
}

/// Returns the final decoded segment from a URL-like string.
///
/// Unsafe decoded separators, dot segments, NUL bytes, invalid UTF-8, and
/// invalid percent encodings preserve the original selected segment.
///
/// # Parameters
/// - `url`: URL-like string to inspect lexically.
///
/// # Returns
/// The decoded safe final segment, or an empty string when none exists.
#[must_use]
#[inline]
pub fn file_name_from_url(url: &str) -> String {
    extract_file_name_from_url(url)
}
