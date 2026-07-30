// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Result normalization for descriptor failures and namespace races.
// qubit-style: allow source-test-pair
// qubit-style: allow coverage-cfg
// Public APIs retain live descriptors and cannot force these interleavings.

use std::fs;
use std::io::{
    Error,
    ErrorKind,
    Result,
};
use std::path::Path;

use super::io_result_context::with_path_context;
use super::path_operations::add_path_context;
use super::rooted_file_io::rooted_type_error;

/// Normalizes the native result of creating a rooted directory component.
///
/// This function must be called immediately after `mkdirat` so it captures the
/// matching operating-system error before any other syscall can replace it.
///
/// # Parameters
///
/// * `result` - The raw return value from `mkdirat`.
/// * `diagnostic_path` - The path attached to a creation error.
///
/// # Returns
///
/// `Ok(())` after successful creation or an `AlreadyExists` race.
///
/// # Errors
///
/// Returns a contextual operating-system error for any other negative result.
pub(super) fn normalize_mkdirat_result(
    result: libc::c_int,
    diagnostic_path: &Path,
) -> Result<()> {
    #[cfg(coverage)]
    let injected_error =
        super::coverage_fault::is_enabled("rooted-mkdir-error");
    #[cfg(not(coverage))]
    let injected_error = false;
    if result == -1 || injected_error {
        let error = if injected_error {
            Error::from_raw_os_error(libc::EIO)
        } else {
            Error::last_os_error()
        };
        if error.kind() != ErrorKind::AlreadyExists {
            return Err(add_path_context(
                error,
                "create rooted directory",
                diagnostic_path,
            ));
        }
    }
    Ok(())
}

/// Converts a failed final-entry status lookup to absence or an I/O error.
///
/// # Parameters
///
/// * `error` - The operating-system error captured immediately after the failed
///   lookup.
/// * `diagnostic_path` - The path attached to a non-absence error.
///
/// # Returns
///
/// `Ok(())` when the final entry is absent.
///
/// # Errors
///
/// Returns a contextual operating-system error when the entry was not merely
/// absent.
pub(super) fn missing_rooted_entry(
    error: Error,
    diagnostic_path: &Path,
) -> Result<()> {
    #[cfg(coverage)]
    let error = if super::coverage_fault::is_enabled("rooted-entry-inspect") {
        Error::from_raw_os_error(libc::EIO)
    } else {
        error
    };
    if error.kind() == ErrorKind::NotFound {
        Ok(())
    } else {
        Err(add_path_context(
            error,
            "inspect rooted file entry",
            diagnostic_path,
        ))
    }
}

/// Normalizes metadata for an opened rooted directory.
///
/// # Parameters
///
/// * `result` - The opened handle's metadata result.
/// * `operation` - The operation label attached to metadata errors.
/// * `diagnostic_path` - The path attached to errors and type diagnostics.
///
/// # Returns
///
/// `Ok(())` when the metadata identifies a directory.
///
/// # Errors
///
/// Returns a contextual metadata error or `InvalidInput` for a non-directory.
pub(super) fn normalize_opened_directory_metadata(
    result: Result<fs::Metadata>,
    operation: &'static str,
    diagnostic_path: &Path,
) -> Result<()> {
    #[cfg(coverage)]
    let result =
        if super::coverage_fault::is_enabled("rooted-directory-metadata") {
            Err(Error::from_raw_os_error(libc::EIO))
        } else {
            result
        };
    let metadata = with_path_context(result, operation, diagnostic_path)?;
    if !metadata.is_dir() || rooted_directory_type_fault_enabled() {
        return Err(rooted_type_error(diagnostic_path, "directory"));
    }
    Ok(())
}

/// Normalizes metadata for an opened rooted regular file.
///
/// # Parameters
///
/// * `result` - The opened handle's metadata result.
/// * `diagnostic_path` - The path attached to metadata and type errors.
///
/// # Returns
///
/// `Ok(())` when the metadata identifies a regular file.
///
/// # Errors
///
/// Returns a contextual metadata error or `InvalidInput` for a non-regular
/// handle.
pub(super) fn normalize_opened_regular_file_metadata(
    result: Result<fs::Metadata>,
    diagnostic_path: &Path,
) -> Result<()> {
    #[cfg(coverage)]
    let result = if super::coverage_fault::is_enabled("rooted-file-metadata") {
        Err(Error::from_raw_os_error(libc::EIO))
    } else {
        result
    };
    let metadata = with_path_context(
        result,
        "inspect rooted file handle",
        diagnostic_path,
    )?;
    if !metadata.is_file() || rooted_file_type_fault_enabled() {
        return Err(rooted_type_error(diagnostic_path, "regular file"));
    }
    Ok(())
}

/// Returns whether coverage should reject an opened directory's type.
#[must_use]
#[inline]
fn rooted_directory_type_fault_enabled() -> bool {
    #[cfg(coverage)]
    return super::coverage_fault::is_enabled("rooted-directory-type");
    #[cfg(not(coverage))]
    false
}

/// Returns whether coverage should reject an opened file's type.
#[inline]
fn rooted_file_type_fault_enabled() -> bool {
    #[cfg(coverage)]
    return super::coverage_fault::is_enabled("rooted-file-type");
    #[cfg(not(coverage))]
    false
}
