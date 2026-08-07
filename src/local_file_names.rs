// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
// qubit-style: allow source-test-pair
// Covered by filename and foundation integration tests.
// qubit-style: allow inline-tests
// qubit-style: allow explicit-imports

use std::ffi::{
    OsStr,
    OsString,
};

use crate::{
    LocalFileError,
    LocalFileErrorKind,
    LocalFileOperation,
    LocalResult,
};

/// Stateless native and portable filename operations.
pub struct LocalFileNames {
    /// Prevents construction of this stateless utility type.
    _private: (),
}

impl LocalFileNames {
    /// Generates a cryptographically random portable filename component.
    ///
    /// # Errors
    ///
    /// Returns `LocalFileError` when the operating-system random source fails.
    #[inline(always)]
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
                LocalFileErrorKind::InvalidPath,
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generates_and_validates_portable_names() {
        let generated = LocalFileNames::random_name().expect("random name");
        LocalFileNames::validate_portable(&generated)
            .expect("generated name is portable");
        let affixed =
            LocalFileNames::random_name_with(Some("pre-"), Some("-suf"))
                .expect("affixed random name");
        let text = affixed.to_str().expect("random name is UTF-8");
        assert!(text.starts_with("pre-"));
        assert!(text.ends_with("-suf"));
    }
}
