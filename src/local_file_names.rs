// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
// qubit-style: allow source-test-pair
// Covered by filename and foundation integration tests.

use std::{
    ffi::{
        OsStr,
        OsString,
    },
    path::Path,
};

use crate::{
    LocalFileError,
    LocalFileErrorKind,
    LocalFileOperation,
    LocalResult,
};

/// Namespace for native and portable filename operations.
pub enum LocalFileNames {}

impl LocalFileNames {
    /// Generates a cryptographically random portable filename component.
    ///
    /// # Errors
    ///
    /// Returns `LocalFileError` when the operating-system random source fails.
    pub fn random_name() -> LocalResult<OsString> {
        Self::random_name_with(None, None)
    }

    /// Generates a cryptographically random filename with optional affixes.
    ///
    /// # Parameters
    ///
    /// - `prefix`: Optional portable prefix.
    /// - `suffix`: Optional portable suffix.
    ///
    /// # Errors
    ///
    /// Returns `LocalFileError` when an affix is invalid or the
    /// operating-system random source fails.
    #[inline]
    pub fn random_name_with(
        prefix: Option<&str>,
        suffix: Option<&str>,
    ) -> LocalResult<OsString> {
        crate::local::try_random_file_name("qubit-local-files-", prefix, suffix)
            .map(OsString::from)
            .map_err(|source| {
                LocalFileError::from_io(
                    LocalFileOperation::GenerateName,
                    None,
                    None,
                    source,
                )
            })
    }

    /// Returns the final native filename component.
    #[must_use]
    pub fn file_name(path: &Path) -> Option<&OsStr> {
        path.file_name()
    }

    /// Returns the native filename without its final extension.
    #[must_use]
    pub fn file_stem(path: &Path) -> Option<&OsStr> {
        path.file_stem()
    }

    /// Returns the native filename prefix before the first non-leading dot.
    #[must_use]
    pub fn file_prefix(path: &Path) -> Option<&OsStr> {
        path.file_prefix()
    }

    /// Returns the final native extension without a leading dot.
    #[must_use]
    pub fn extension(path: &Path) -> Option<&OsStr> {
        path.extension()
    }

    /// Returns the final extension including a leading dot.
    ///
    /// `None` means the path has no extension.
    #[must_use]
    #[inline]
    pub fn dot_extension(path: &Path) -> Option<OsString> {
        path.extension().map(|extension| {
            let mut result = OsString::from(".");
            result.push(extension);
            result
        })
    }

    /// Validates a conservative cross-platform filename component.
    ///
    /// # Parameters
    ///
    /// - `name`: Native filename component to validate.
    ///
    /// # Errors
    ///
    /// Returns `LocalFileError` when the name is not UTF-8 portable text or
    /// violates the crate's portable filename rules.
    #[inline]
    pub fn validate_portable(name: &OsStr) -> LocalResult<()> {
        let Some(name) = name.to_str() else {
            return Err(LocalFileError::new(
                LocalFileErrorKind::InvalidInput,
                LocalFileOperation::ValidateName,
            ));
        };
        crate::local::validate_portable_file_name_impl(name).map_err(|source| {
            LocalFileError::from_io(
                LocalFileOperation::ValidateName,
                None,
                None,
                source,
            )
        })
    }
}
