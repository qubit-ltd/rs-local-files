// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Validated relative paths for rooted filesystem operations.

use std::ffi::OsStr;
use std::io::{Error, ErrorKind, Result};
use std::path::{Component, Path, PathBuf};

/// An owned, validated path accepted by [`crate::rooted::Root`].
///
/// The path is non-empty and contains only normal relative components. This
/// lexical type prevents accidental unchecked input, while the open root
/// capability and descriptor-relative operations provide actual containment.
#[must_use]
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct LocalRelativePath {
    /// Sole owned path state after lexical validation.
    path: PathBuf,
}

impl LocalRelativePath {
    /// Validates and owns a relative rooted path.
    ///
    /// # Parameters
    ///
    /// * `path` - Candidate path to validate and own.
    ///
    /// # Returns
    ///
    /// A strongly typed relative path containing the original normal
    /// components.
    ///
    /// # Errors
    ///
    /// Returns [`ErrorKind::InvalidInput`] for an empty or absolute path, a
    /// root, prefix, current-directory or parent-directory component, or an
    /// embedded NUL value.
    pub fn new<P>(path: P) -> Result<Self>
    where
        P: AsRef<Path>,
    {
        let path = path.as_ref();
        if path.as_os_str().is_empty()
            || contains_nul(path)
            || contains_explicit_dot_component(path)
            || path
                .components()
                .any(|component| !matches!(component, Component::Normal(_)))
        {
            return Err(invalid_relative_path_error(path));
        }
        Ok(Self {
            path: path.to_path_buf(),
        })
    }

    /// Returns the validated relative path.
    ///
    /// # Returns
    ///
    /// The sole path state owned by this value.
    #[must_use]
    #[inline(always)]
    pub fn as_path(&self) -> &Path {
        &self.path
    }

    /// Appends a validated relative descendant.
    ///
    /// # Parameters
    ///
    /// * `child` - Non-empty relative path containing only normal components.
    ///
    /// # Returns
    ///
    /// A newly validated path containing this path followed by `child`.
    ///
    /// # Errors
    ///
    /// Returns [`ErrorKind::InvalidInput`] when `child` is empty, absolute,
    /// contains a non-normal component, or contains an embedded NUL value.
    #[inline]
    pub fn join<P>(&self, child: P) -> Result<Self>
    where
        P: AsRef<Path>,
    {
        let child = Self::new(child)?;
        Self::new(self.path.join(child.as_path()))
    }

    /// Appends exactly one validated normal path component.
    ///
    /// # Parameters
    ///
    /// * `child` - Native child name to append.
    ///
    /// # Returns
    ///
    /// A newly validated path containing this path followed by `child`.
    ///
    /// # Errors
    ///
    /// Returns [`ErrorKind::InvalidInput`] when `child` is not exactly one
    /// normal component or contains an embedded NUL value.
    pub fn join_component(&self, child: &OsStr) -> Result<Self> {
        let child_path = Path::new(child);
        let mut components = child_path.components();
        if !matches!(components.next(), Some(Component::Normal(_))) || components.next().is_some() {
            return Err(invalid_relative_path_error(child_path));
        }
        self.join(child_path)
    }
}

/// Creates the canonical validation error for a rooted relative path.
///
/// # Parameters
///
/// * `path` - Invalid path included for diagnostics.
///
/// # Returns
///
/// An invalid-input error describing the lexical contract.
#[inline]
fn invalid_relative_path_error(path: &Path) -> Error {
    Error::new(
        ErrorKind::InvalidInput,
        format!(
            "rooted path must be non-empty and contain only normal relative components without NUL: {}",
            path.display(),
        ),
    )
}

#[cfg(unix)]
/// Reports whether a Unix path contains an embedded NUL byte.
///
/// # Parameters
///
/// * `path` - Path whose platform representation is inspected.
///
/// # Returns
///
/// `true` when the path contains NUL; otherwise, `false`.
#[must_use]
#[inline]
fn contains_nul(path: &Path) -> bool {
    use std::os::unix::ffi::OsStrExt;

    path.as_os_str().as_bytes().contains(&0)
}

#[cfg(windows)]
/// Reports whether a Windows path contains an embedded NUL unit.
///
/// # Parameters
///
/// * `path` - Path whose platform representation is inspected.
///
/// # Returns
///
/// `true` when the path contains NUL; otherwise, `false`.
#[must_use]
#[inline]
fn contains_nul(path: &Path) -> bool {
    use std::os::windows::ffi::OsStrExt;

    path.as_os_str().encode_wide().any(|unit| unit == 0)
}

#[cfg(not(any(unix, windows)))]
/// Reports whether a fallback path representation contains NUL.
///
/// # Parameters
///
/// * `path` - Path whose lossy representation is inspected.
///
/// # Returns
///
/// `true` when the path contains NUL; otherwise, `false`.
#[must_use]
#[inline]
fn contains_nul(path: &Path) -> bool {
    path.to_string_lossy().contains('\0')
}

#[cfg(unix)]
/// Reports whether Unix path bytes contain an explicit `.` component.
///
/// [`Path::components`] normalizes some current-directory components, so the
/// raw representation is checked to keep the public rejection contract exact.
///
/// # Parameters
///
/// * `path` - Relative path to inspect.
///
/// # Returns
///
/// `true` when any slash-delimited component is exactly `.`.
#[must_use]
#[inline]
fn contains_explicit_dot_component(path: &Path) -> bool {
    use std::os::unix::ffi::OsStrExt;

    path.as_os_str()
        .as_bytes()
        .split(|byte| *byte == b'/')
        .any(|component| component == b".")
}

#[cfg(windows)]
/// Reports whether Windows path units contain an explicit `.` component.
///
/// # Parameters
///
/// * `path` - Relative path to inspect.
///
/// # Returns
///
/// `true` when any slash-delimited component is exactly `.`.
#[must_use]
#[inline]
fn contains_explicit_dot_component(path: &Path) -> bool {
    use std::os::windows::ffi::OsStrExt;

    let units = path.as_os_str().encode_wide().collect::<Vec<_>>();
    units
        .split(|unit| *unit == u16::from(b'/') || *unit == u16::from(b'\\'))
        .any(|component| component == [u16::from(b'.')])
}

#[cfg(not(any(unix, windows)))]
/// Reports whether a fallback path contains an explicit `.` component.
///
/// # Parameters
///
/// * `path` - Relative path to inspect.
///
/// # Returns
///
/// `true` when any slash-delimited component is exactly `.`.
#[must_use]
#[inline]
fn contains_explicit_dot_component(path: &Path) -> bool {
    path.to_string_lossy().split('/').any(|part| part == ".")
}
