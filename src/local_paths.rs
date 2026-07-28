// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::{
    env,
    path::{
        Component,
        Path,
        PathBuf,
    },
};

use crate::{
    LocalFileError,
    LocalFileErrorKind,
    LocalFileOperation,
    LocalResult,
};

/// Namespace for native path validation, binding, and composition.
pub enum LocalPaths {}

impl LocalPaths {
    /// Binds a relative host path to one current-working-directory snapshot.
    ///
    /// Absolute paths are returned unchanged.
    ///
    /// # Parameters
    ///
    /// - `path`: Native absolute or relative host path.
    ///
    /// # Returns
    ///
    /// An absolute path that remains stable if the process working directory
    /// changes.
    ///
    /// # Errors
    ///
    /// Returns `LocalFileError` when the current directory cannot be read.
    #[inline]
    pub fn bind_host_path(path: &Path) -> LocalResult<PathBuf> {
        if path.is_absolute() {
            return Ok(path.to_path_buf());
        }
        env::current_dir()
            .map(|current| current.join(path))
            .map_err(|source| {
                LocalFileError::from_io(
                    LocalFileOperation::BindPath,
                    Some(path.to_path_buf()),
                    None,
                    source,
                )
            })
    }

    /// Binds multiple host paths using exactly one current-directory snapshot.
    ///
    /// # Parameters
    ///
    /// - `paths`: Fixed-size group of native absolute or relative paths.
    ///
    /// # Returns
    ///
    /// Absolute paths bound to the same host namespace snapshot.
    ///
    /// # Errors
    ///
    /// Returns `LocalFileError` when the current directory cannot be read.
    #[inline]
    pub fn bind_host_paths<const N: usize>(
        paths: [&Path; N],
    ) -> LocalResult<[PathBuf; N]> {
        let current = if paths.iter().any(|path| path.is_relative()) {
            Some(env::current_dir().map_err(|source| {
                LocalFileError::from_io(
                    LocalFileOperation::BindPath,
                    None,
                    None,
                    source,
                )
            })?)
        } else {
            None
        };
        Ok(paths.map(|path| {
            current.as_ref().map_or_else(
                || path.to_path_buf(),
                |directory| directory.join(path),
            )
        }))
    }

    /// Tests normalized lexical containment without accessing the filesystem.
    ///
    /// # Parameters
    ///
    /// - `path`: Candidate descendant path.
    /// - `ancestor`: Candidate ancestor path.
    ///
    /// # Returns
    ///
    /// `true` when `path` is equal to or lexically below `ancestor`.
    ///
    /// # Errors
    ///
    /// Returns `LocalFileError` when either input contains `.` or `..`, or when
    /// absolute and relative forms differ.
    #[inline]
    pub fn is_lexically_within(
        path: &Path,
        ancestor: &Path,
    ) -> LocalResult<bool> {
        if path.is_absolute() != ancestor.is_absolute()
            || has_disallowed_component(path)
            || has_disallowed_component(ancestor)
        {
            return Err(LocalFileError::new(
                LocalFileErrorKind::InvalidInput,
                LocalFileOperation::ComposePath,
            )
            .with_path(path.to_path_buf())
            .with_target(ancestor.to_path_buf()));
        }
        Ok(path.starts_with(ancestor))
    }

    /// Composes a validated relative descendant beneath a native base path.
    ///
    /// # Parameters
    ///
    /// - `base`: Native base path.
    /// - `descendant`: Relative path containing only normal components.
    ///
    /// # Returns
    ///
    /// The lexically joined path.
    ///
    /// # Errors
    ///
    /// Returns `LocalFileError` for absolute, prefixed, dot, or parent
    /// components.
    #[inline]
    pub fn compose_descendant(
        base: &Path,
        descendant: &Path,
    ) -> LocalResult<PathBuf> {
        if descendant.as_os_str().is_empty()
            || descendant.is_absolute()
            || has_disallowed_component(descendant)
            || descendant
                .components()
                .any(|component| !matches!(component, Component::Normal(_)))
        {
            return Err(LocalFileError::new(
                LocalFileErrorKind::InvalidInput,
                LocalFileOperation::ComposePath,
            )
            .with_path(descendant.to_path_buf()));
        }
        Ok(base.join(descendant))
    }
}

/// Reports components that can change lexical authority.
///
/// # Parameters
///
/// - `path`: Native path to inspect.
///
/// # Returns
///
/// `true` when `.` or `..` is present.
#[inline]
fn has_disallowed_component(path: &Path) -> bool {
    path.components().any(|component| {
        matches!(component, Component::CurDir | Component::ParentDir)
    }) || has_raw_dot_component(path)
}

/// Detects raw dot components that `Path::components` may normalize away.
///
/// # Parameters
///
/// - `path`: Native path to inspect.
///
/// # Returns
///
/// `true` when a raw component is `.` or `..`.
#[cfg(unix)]
#[inline]
fn has_raw_dot_component(path: &Path) -> bool {
    use std::os::unix::ffi::OsStrExt;

    path.as_os_str()
        .as_bytes()
        .split(|byte| *byte == b'/')
        .any(|component| component == b"." || component == b"..")
}

/// Detects raw dot components on Windows.
///
/// # Parameters
///
/// - `path`: Native path to inspect.
///
/// # Returns
///
/// `true` when a raw component is `.` or `..`.
#[cfg(windows)]
#[inline]
fn has_raw_dot_component(path: &Path) -> bool {
    use std::os::windows::ffi::OsStrExt;

    let separator = |unit: &u16| *unit == b'/' as u16 || *unit == b'\\' as u16;
    path.as_os_str()
        .encode_wide()
        .collect::<Vec<_>>()
        .split(separator)
        .any(|component| {
            component == [b'.' as u16] || component == [b'.' as u16; 2]
        })
}

/// Detects raw dot components on unsupported native targets.
#[cfg(not(any(unix, windows)))]
#[inline(always)]
const fn has_raw_dot_component(_path: &Path) -> bool {
    false
}
