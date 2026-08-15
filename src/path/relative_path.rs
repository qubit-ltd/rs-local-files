// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Validated authority-relative native paths.

use std::path::Component;
use std::path::Path;
use std::path::PathBuf;

use crate::LocalFileError;
use crate::LocalFileErrorKind;
use crate::LocalFileOperation;
use crate::LocalPathCodecError;
use crate::LocalResult;

/// An owned native path that cannot change a rooted authority.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
#[must_use]
pub(crate) struct RelativePath(PathBuf);

impl RelativePath {
    /// Validates and owns an authority-relative native path.
    ///
    /// # Parameters
    ///
    /// - `path`: Candidate relative path. The empty path denotes the authority
    ///   root.
    ///
    /// # Returns
    ///
    /// The original native representation after lexical validation.
    ///
    /// # Errors
    ///
    /// Returns an invalid-path error for a root, prefix, dot or parent
    /// component, or embedded native NUL.
    pub(crate) fn parse(path: &Path) -> LocalResult<Self> {
        validate_relative_components(path)?;
        Ok(Self(path.to_path_buf()))
    }

    /// Returns the preserved authority-relative native path.
    #[must_use]
    #[inline(always)]
    pub(crate) fn as_path(&self) -> &Path {
        &self.0
    }
}

/// Validates the lexical components of an authority-relative path.
///
/// # Parameters
///
/// - `path`: Candidate native path.
///
/// # Errors
///
/// Returns an invalid-path error when the native form contains NUL, an
/// explicit dot segment, or any non-normal component. The empty authority root
/// is accepted.
fn validate_relative_components(path: &Path) -> LocalResult<()> {
    if contains_native_nul(path) {
        return Err(LocalFileError::from_path_codec(
            LocalFileOperation::ComposePath,
            Some(path.to_path_buf()),
            LocalPathCodecError::NativeNul,
        ));
    }
    if contains_explicit_dot_component(path)
        || path
            .components()
            .any(|component| !matches!(component, Component::Normal(_)))
    {
        return Err(invalid_relative_path_error(path));
    }
    Ok(())
}

/// Creates the structured validation error for an authority-relative path.
///
/// # Parameters
///
/// - `path`: Invalid native path retained as diagnostic context.
///
/// # Returns
///
/// An invalid compose-path error.
#[must_use]
#[inline]
fn invalid_relative_path_error(path: &Path) -> LocalFileError {
    LocalFileError::new(
        LocalFileErrorKind::InvalidPath,
        LocalFileOperation::ComposePath,
    )
    .with_path(path.to_path_buf())
}

/// Reports whether a Unix path contains an embedded NUL byte.
#[cfg(unix)]
#[must_use]
fn contains_native_nul(path: &Path) -> bool {
    use std::os::unix::ffi::OsStrExt;

    path.as_os_str().as_bytes().contains(&0)
}

/// Reports whether a Windows path contains an embedded NUL code unit.
#[cfg(windows)]
#[must_use]
fn contains_native_nul(path: &Path) -> bool {
    use std::os::windows::ffi::OsStrExt;

    path.as_os_str().encode_wide().any(|unit| unit == 0)
}

/// Rejects native paths on unsupported targets.
#[cfg(not(any(unix, windows)))]
#[must_use]
const fn contains_native_nul(_path: &Path) -> bool {
    true
}

/// Reports whether a Unix path contains an explicit dot component.
#[cfg(unix)]
#[must_use]
fn contains_explicit_dot_component(path: &Path) -> bool {
    use std::os::unix::ffi::OsStrExt;

    path.as_os_str()
        .as_bytes()
        .split(|byte| *byte == b'/')
        .any(|component| component == b".")
}

/// Reports whether a Windows path contains an explicit dot component.
#[cfg(windows)]
#[must_use]
fn contains_explicit_dot_component(path: &Path) -> bool {
    use std::os::windows::ffi::OsStrExt;

    let units = path.as_os_str().encode_wide().collect::<Vec<_>>();
    units
        .split(|unit| *unit == u16::from(b'/') || *unit == u16::from(b'\\'))
        .any(|component| component == [u16::from(b'.')])
}

/// Rejects relative paths on unsupported targets.
#[cfg(not(any(unix, windows)))]
#[must_use]
const fn contains_explicit_dot_component(_path: &Path) -> bool {
    true
}
