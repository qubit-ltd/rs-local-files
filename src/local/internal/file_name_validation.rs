// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Private legacy file-name fragment validation.
// qubit-style: allow source-test-pair

use std::io::Error;
use std::io::ErrorKind;
use std::io::Result;
use std::path::Component;
use std::path::Path;

/// Validates a caller-provided file-name fragment.
///
/// # Parameters
/// - `role`: Fragment role used in error messages.
/// - `fragment`: File-name fragment to validate.
///
/// # Errors
/// Returns [`ErrorKind::InvalidInput`] when `fragment` can behave like a path
/// instead of a plain file-name fragment.
pub(super) fn validate_file_name_fragment(
    role: &str,
    fragment: &str,
) -> Result<()> {
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
