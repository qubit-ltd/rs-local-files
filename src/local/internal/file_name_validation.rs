// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Private portable file-name validation.
// qubit-style: allow source-test-pair

use std::io::{Error, ErrorKind, Result};
use std::path::{Component, Path};

/// Maximum byte length accepted for a portable UTF-8 file name.
const MAX_PORTABLE_FILE_NAME_BYTES: usize = 255;

/// Validates a portable single-component file name.
///
/// # Parameters
/// - `name`: File-name component to validate.
///
/// # Errors
/// Returns [`ErrorKind::InvalidInput`] when `name` is not portable.
pub(crate) fn validate_portable_file_name_impl(name: &str) -> Result<()> {
    if name.is_empty() {
        return Err(Error::new(
            ErrorKind::InvalidInput,
            "portable file name must not be empty",
        ));
    }
    if name == "." || name == ".." {
        return Err(Error::new(
            ErrorKind::InvalidInput,
            "portable file name must not be a dot segment",
        ));
    }
    if name.len() > MAX_PORTABLE_FILE_NAME_BYTES {
        return Err(Error::new(
            ErrorKind::InvalidInput,
            format!("portable file name exceeds {MAX_PORTABLE_FILE_NAME_BYTES} UTF-8 bytes"),
        ));
    }
    if name.ends_with([' ', '.']) {
        return Err(Error::new(
            ErrorKind::InvalidInput,
            "portable file name must not end with a space or dot",
        ));
    }
    if let Some(character) = name.chars().find(|character| {
        character.is_control()
            || matches!(
                character,
                '/' | '\\' | '<' | '>' | ':' | '"' | '|' | '?' | '*'
            )
    }) {
        return Err(Error::new(
            ErrorKind::InvalidInput,
            format!("portable file name contains forbidden character {character:?}"),
        ));
    }
    if is_windows_reserved_file_name(name) {
        return Err(Error::new(
            ErrorKind::InvalidInput,
            "portable file name must not be a Windows reserved device name",
        ));
    }
    Ok(())
}

/// Validates a caller-provided file-name fragment.
///
/// # Parameters
/// - `role`: Fragment role used in error messages.
/// - `fragment`: File-name fragment to validate.
///
/// # Errors
/// Returns [`ErrorKind::InvalidInput`] when `fragment` can behave like a path
/// instead of a plain file-name fragment.
pub(super) fn validate_file_name_fragment(role: &str, fragment: &str) -> Result<()> {
    if fragment.contains('\0') {
        return Err(invalid_file_name_fragment_error(
            role,
            "NUL bytes are not allowed",
        ));
    }
    if fragment.contains('/') || fragment.contains('\\') {
        return Err(invalid_file_name_fragment_error(
            role,
            "path separators are not allowed",
        ));
    }
    if Path::new(fragment).components().any(|component| {
        matches!(
            component,
            Component::Prefix(_) | Component::RootDir | Component::ParentDir
        )
    }) {
        return Err(invalid_file_name_fragment_error(
            role,
            "path components are not allowed",
        ));
    }
    Ok(())
}

/// Builds an invalid file-name fragment error.
///
/// # Parameters
/// - `role`: Fragment role used in error messages.
/// - `reason`: Validation failure reason.
///
/// # Returns
/// An [`ErrorKind::InvalidInput`] error.
#[must_use]
#[inline]
fn invalid_file_name_fragment_error(role: &str, reason: &str) -> Error {
    Error::new(
        ErrorKind::InvalidInput,
        format!("random file name {role} is invalid: {reason}"),
    )
}

/// Tests whether a single-component file name is reserved by Windows.
///
/// # Parameters
/// - `name`: File name to inspect.
///
/// # Returns
/// `true` when `name` uses a reserved device name, including a reserved base
/// name followed by an extension.
#[must_use]
fn is_windows_reserved_file_name(name: &str) -> bool {
    let base_name = name
        .split_once('.')
        .map_or(name, |(base_name, _)| base_name);
    let base_name = base_name.trim_end_matches([' ', '.']);

    if base_name.eq_ignore_ascii_case("CON")
        || base_name.eq_ignore_ascii_case("PRN")
        || base_name.eq_ignore_ascii_case("AUX")
        || base_name.eq_ignore_ascii_case("NUL")
        || base_name.eq_ignore_ascii_case("CONIN$")
        || base_name.eq_ignore_ascii_case("CONOUT$")
    {
        return true;
    }

    let Some((suffix_index, suffix)) = base_name.char_indices().next_back() else {
        return false;
    };
    let prefix = &base_name[..suffix_index];
    let reserved_digit = matches!(suffix, '1'..='9' | '¹' | '²' | '³');
    (prefix.eq_ignore_ascii_case("COM") || prefix.eq_ignore_ascii_case("LPT")) && reserved_digit
}
