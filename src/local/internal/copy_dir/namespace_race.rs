// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Result normalization for externally timed copy-destination races.
// qubit-style: allow source-test-pair
// Public fixtures cannot deterministically interleave these namespace changes.

use std::fs;
use std::io::ErrorKind;
use std::io::Result;
use std::path::Path;

use super::source::is_real_directory;
#[cfg(feature = "test-support")]
use crate::local::internal::test_support;

/// Reconciles a directory-creation result with a concurrent creator.
///
/// # Type Parameters
///
/// * `I` - The filesystem metadata inspection callable.
///
/// # Parameters
///
/// * `dst` - The destination path inspected after an `AlreadyExists` result.
/// * `result` - The private-directory creation result to normalize.
/// * `inspect` - The covered filesystem reinspection operation to invoke only
///   after an `AlreadyExists` race.
///
/// # Returns
///
/// `true` when this caller created the directory, or `false` when a real
/// directory appeared concurrently.
///
/// # Errors
///
/// Returns the creation error when the failure was not `AlreadyExists`, the
/// racing entry cannot be inspected, or that entry is not a real directory.
pub(super) fn reconcile_directory_creation<I>(dst: &Path, result: Result<()>, inspect: I) -> Result<bool>
where
    I: FnOnce(&Path) -> Result<fs::Metadata>,
{
    match result {
        Ok(()) => Ok(true),
        Err(error) if error.kind() == ErrorKind::AlreadyExists => {
            let metadata = inspect(dst)?;
            if is_real_directory(&metadata) && !test_non_directory_race_enabled() {
                Ok(false)
            } else {
                Err(error)
            }
        }
        Err(error) => Err(error),
    }
}

/// Returns whether test support should classify a racing entry as
/// non-directory.
#[must_use]
// qubit-style: allow coverage-cfg
#[cfg_attr(not(coverage), inline)]
#[cfg_attr(coverage, inline(never))]
fn test_non_directory_race_enabled() -> bool {
    #[cfg(feature = "test-support")]
    return test_support::is_enabled("copy-directory-race-nondirectory");
    #[cfg(not(feature = "test-support"))]
    false
}

/// Normalizes metadata reinspection after a non-directory was observed.
///
/// # Parameters
///
/// * `result` - The destination metadata result captured immediately before
///   removal.
///
/// # Returns
///
/// `Some(metadata)` when the entry still is not a real directory, or `None`
/// when it disappeared or changed into a real directory.
///
/// # Errors
///
/// Returns metadata errors other than `NotFound`.
pub(super) fn removable_non_directory_metadata(result: Result<fs::Metadata>) -> Result<Option<fs::Metadata>> {
    match result {
        Ok(metadata) if is_real_directory(&metadata) || test_removal_directory_race_enabled() => Ok(None),
        Ok(metadata) => Ok(Some(metadata)),
        Err(error) if error.kind() == ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error),
    }
}

/// Returns whether test support should classify a replacement race as
/// directory.
#[cfg_attr(not(coverage), inline)]
#[cfg_attr(coverage, inline(never))]
fn test_removal_directory_race_enabled() -> bool {
    #[cfg(feature = "test-support")]
    return test_support::is_enabled("copy-removal-race-directory");
    #[cfg(not(feature = "test-support"))]
    false
}
