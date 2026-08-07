// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::{
    env,
    ffi::{
        OsStr,
        OsString,
    },
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
    LocalPathCodec,
    LocalResult,
};

/// Native path validation, binding, and composition utilities.
pub struct LocalPaths {
    /// Prevents construction of this stateless utility type.
    _private: (),
}

impl LocalPaths {
    /// Decodes canonical components into an absolute native path.
    ///
    /// # Parameters
    ///
    /// - `components`: Canonical escaped-byte components in the platform's
    ///   absolute-path shape.
    ///
    /// # Returns
    ///
    /// An absolute native path without dot, parent, separator, root, or prefix
    /// components outside its required platform root. Unix paths use an empty
    /// first component as the root marker; Windows paths use `""`, the drive
    /// component (for example `"C:"`), and then normal descendants.
    ///
    /// # Errors
    ///
    /// Returns `LocalFileError` with `ComposePath` when canonical decoding
    /// fails or the components do not form a supported absolute path.
    pub fn from_canonical_absolute_components<'a>(
        components: impl IntoIterator<Item = &'a str>,
    ) -> LocalResult<PathBuf> {
        from_canonical_absolute_components(components)
    }

    /// Decodes canonical components into a relative native path.
    ///
    /// # Parameters
    ///
    /// - `components`: Canonical escaped-byte components, each of which must
    ///   decode to one normal relative component.
    ///
    /// # Returns
    ///
    /// A relative native path consisting solely of normal components.
    ///
    /// # Errors
    ///
    /// Returns `LocalFileError` with `ComposePath` when canonical decoding
    /// fails or a component would alter path authority.
    pub fn from_canonical_relative_components<'a>(
        components: impl IntoIterator<Item = &'a str>,
    ) -> LocalResult<PathBuf> {
        let mut path = PathBuf::new();
        let mut has_component = false;
        for component in components {
            path.push(decode_normal_component(component)?);
            has_component = true;
        }
        if !has_component {
            return Err(invalid_path_error());
        }
        Ok(path)
    }

    /// Encodes an absolute native path as canonical components.
    ///
    /// # Parameters
    ///
    /// - `path`: Native absolute path using only the supported platform root
    ///   and normal descendant components.
    ///
    /// # Returns
    ///
    /// Canonical escaped-byte components in the platform's absolute-path
    /// shape. Unix output starts with an empty root marker. Windows output
    /// starts with an empty marker followed by the drive component (for
    /// example `"C:"`) before normal descendants.
    ///
    /// # Errors
    ///
    /// Returns `LocalFileError` with `ComposePath` when the path is not a
    /// supported absolute shape or one component cannot be canonically encoded.
    pub fn to_canonical_absolute_components(
        path: &Path,
    ) -> LocalResult<Vec<String>> {
        to_canonical_absolute_components(path)
    }

    /// Encodes a relative native path as canonical components.
    ///
    /// # Parameters
    ///
    /// - `path`: Native relative path consisting solely of normal components.
    ///
    /// # Returns
    ///
    /// Canonical escaped-byte components in lexical order.
    ///
    /// # Errors
    ///
    /// Returns `LocalFileError` with `ComposePath` when `path` contains a
    /// root, prefix, dot, parent, or no normal components.
    pub fn to_canonical_relative_components(
        path: &Path,
    ) -> LocalResult<Vec<String>> {
        if path.is_absolute() || has_disallowed_component(path) {
            return Err(invalid_path_error());
        }
        let components = encode_normal_components(path)?;
        if components.is_empty() {
            return Err(invalid_path_error());
        }
        Ok(components)
    }

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
    pub fn bind_host_path(path: &Path) -> LocalResult<PathBuf> {
        if path.is_absolute() {
            return Ok(path.to_path_buf());
        }
        current_directory_for_binding("local-path-bind-cwd")
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
    /// - `paths`: Pair of native absolute or relative paths.
    ///
    /// # Returns
    ///
    /// Absolute paths bound to the same host namespace snapshot.
    ///
    /// # Errors
    ///
    /// Returns `LocalFileError` when the current directory cannot be read.
    pub fn bind_host_paths(paths: [&Path; 2]) -> LocalResult<[PathBuf; 2]> {
        let current = if paths.iter().any(|path| path.is_relative()) {
            Some(
                current_directory_for_binding("local-paths-bind-cwd").map_err(
                    |source| {
                        LocalFileError::from_io(
                            LocalFileOperation::BindPath,
                            None,
                            None,
                            source,
                        )
                    },
                )?,
            )
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
    pub fn is_lexically_within(
        path: &Path,
        ancestor: &Path,
    ) -> LocalResult<bool> {
        if path.is_absolute() != ancestor.is_absolute()
            || has_disallowed_component(path)
            || has_disallowed_component(ancestor)
        {
            return Err(LocalFileError::new(
                LocalFileErrorKind::InvalidPath,
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
    pub fn compose_descendant(
        base: &Path,
        descendant: &Path,
    ) -> LocalResult<PathBuf> {
        if descendant.as_os_str().is_empty()
            || descendant.is_absolute()
            || has_disallowed_component(descendant)
        {
            return Err(LocalFileError::new(
                LocalFileErrorKind::InvalidPath,
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
#[must_use]
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
#[must_use]
#[inline]
fn has_raw_dot_component(path: &Path) -> bool {
    use std::os::unix::ffi::OsStrExt;

    path.as_os_str()
        .as_bytes()
        .split(|byte| *byte == b'/')
        .any(|component| component == b"." || component == b"..")
}

/// Creates the common structured error for invalid canonical path shapes.
///
/// # Returns
///
/// A `ComposePath` invalid-input error with no native path context, because
/// the rejected shape may not be safely representable as a path.
#[must_use]
#[inline(always)]
fn invalid_path_error() -> LocalFileError {
    LocalFileError::new(
        LocalFileErrorKind::InvalidPath,
        LocalFileOperation::ComposePath,
    )
}

/// Reads the host current directory used to bind a relative path.
///
/// # Parameters
///
/// - `fault`: Test-support-only fault selector for exercising the host I/O
///   error conversion.
///
/// # Returns
///
/// The current directory snapshot used for relative host paths.
///
/// # Errors
///
/// Returns the native current-directory I/O error, including a deterministic
/// test-support-only error when the selected fault is enabled.
#[cfg(feature = "internal-test-support")]
#[inline]
fn current_directory_for_binding(fault: &str) -> std::io::Result<PathBuf> {
    if crate::local::test_support_enabled(fault) {
        return Err(crate::local::test_fault_error());
    }
    env::current_dir()
}

/// Reads the host current directory used to bind a relative path.
///
/// # Parameters
///
/// - `fault`: Ignored when test support is disabled.
///
/// # Returns
///
/// The current directory snapshot used for relative host paths.
///
/// # Errors
///
/// Returns the native current-directory I/O error.
#[cfg(not(feature = "internal-test-support"))]
#[inline(always)]
fn current_directory_for_binding(_fault: &str) -> std::io::Result<PathBuf> {
    env::current_dir()
}

/// Decodes one canonical component and verifies it is one native normal
/// component.
///
/// # Parameters
///
/// - `component`: Canonical escaped-byte component.
///
/// # Returns
///
/// The decoded native component when it contains neither a separator nor a
/// root, prefix, dot, or parent interpretation.
///
/// # Errors
///
/// Returns a `ComposePath` error retaining a `PathCodec` source for codec
/// failures, or an invalid-input error for unsafe native component shapes.
fn decode_normal_component(component: &str) -> LocalResult<OsString> {
    let native = decode_canonical_component(component)?;
    if is_normal_native_component(&native) {
        Ok(native)
    } else {
        Err(invalid_path_error())
    }
}

/// Decodes one canonical component while retaining codec failures as typed
/// errors.
///
/// # Parameters
///
/// - `component`: Canonical escaped-byte component.
///
/// # Returns
///
/// The decoded native string without making a path-shape judgment.
///
/// # Errors
///
/// Returns a `ComposePath` error retaining a `PathCodec` source when the text
/// is malformed, non-canonical, or unrepresentable on the current platform.
#[inline]
fn decode_canonical_component(component: &str) -> LocalResult<OsString> {
    LocalPathCodec::from_canonical_text(component)
        .map_err(|error| {
            LocalFileError::from_path_codec(
                LocalFileOperation::ComposePath,
                None,
                error,
            )
        })
        .map(|native| native.into_owned())
}

/// Reports whether one native string is exactly one safe normal component.
///
/// # Parameters
///
/// - `component`: Native string to classify.
///
/// # Returns
///
/// `true` only for non-empty normal components without either native path
/// separator; `false` for roots, prefixes, dots, parents, and separators.
#[must_use]
fn is_normal_native_component(component: &OsStr) -> bool {
    !has_native_separator(component)
        && matches!(
            Path::new(component).components().next(),
            Some(Component::Normal(_))
        )
        && Path::new(component).components().count() == 1
}

/// Encodes one native component while retaining codec failures as typed
/// structured errors.
///
/// # Parameters
///
/// - `component`: Native component or Windows drive prefix to encode.
///
/// # Returns
///
/// The canonical escaped-byte representation.
///
/// # Errors
///
/// Returns a `ComposePath` error retaining the underlying path-codec failure.
#[inline]
fn encode_native_component(component: &OsStr) -> LocalResult<String> {
    LocalPathCodec::to_canonical_text(component)
        .map(|canonical| canonical.into_owned())
        .map_err(|error| {
            LocalFileError::from_path_codec(
                LocalFileOperation::ComposePath,
                None,
                error,
            )
        })
}

/// Encodes normal native components from a relative or absolute descendant.
///
/// # Parameters
///
/// - `path`: Native path whose components have already passed root-shape
///   validation.
///
/// # Returns
///
/// Canonical components in their original lexical order.
///
/// # Errors
///
/// Returns an invalid-input `ComposePath` error for a non-normal component.
/// A NUL-containing native component is returned as a typed path-codec error.
fn encode_normal_components(path: &Path) -> LocalResult<Vec<String>> {
    let mut encoded = Vec::new();
    for component in path.components() {
        let Component::Normal(component) = component else {
            return Err(invalid_path_error());
        };
        encoded.push(encode_native_component(component)?);
    }
    Ok(encoded)
}

/// Reports whether a native component contains a platform path separator.
///
/// # Parameters
///
/// - `component`: Native component to inspect.
///
/// # Returns
///
/// `true` when the component contains a separator that would make `push`
/// interpret it as more than one lexical component.
#[cfg(unix)]
#[inline]
fn has_native_separator(component: &OsStr) -> bool {
    use std::os::unix::ffi::OsStrExt;

    component.as_bytes().contains(&b'/')
}

/// Reports whether a native component contains a platform path separator.
///
/// # Parameters
///
/// - `component`: Native component to inspect.
///
/// # Returns
///
/// `true` when the component contains a slash or backslash.
#[cfg(windows)]
#[inline]
fn has_native_separator(component: &OsStr) -> bool {
    use std::os::windows::ffi::OsStrExt;

    component
        .encode_wide()
        .any(|unit| unit == b'/' as u16 || unit == b'\\' as u16)
}

/// Reports whether a native component contains a separator on unsupported
/// targets.
///
/// # Parameters
///
/// - `component`: Native component to inspect.
///
/// # Returns
///
/// Always `true`, preventing platform-specific path construction on targets
/// that this API does not support.
#[cfg(not(any(unix, windows)))]
#[inline(always)]
const fn has_native_separator(_component: &OsStr) -> bool {
    true
}

/// Builds a Unix absolute path from canonical path components.
///
/// # Parameters
///
/// - `components`: Canonical components beginning with the empty root marker.
///
/// # Returns
///
/// The decoded absolute native path.
///
/// # Errors
///
/// Returns a `ComposePath` error when the root marker or any component is
/// malformed.
#[cfg(unix)]
#[inline(never)]
fn from_canonical_absolute_components<'a>(
    components: impl IntoIterator<Item = &'a str>,
) -> LocalResult<PathBuf> {
    let mut components = components.into_iter();
    if components.next() != Some("") {
        return Err(invalid_path_error());
    }
    let mut path = PathBuf::from("/");
    for component in components {
        path.push(decode_normal_component(component)?);
    }
    Ok(path)
}

/// Encodes a Unix absolute native path into canonical components.
///
/// # Parameters
///
/// - `path`: Unix path to encode.
///
/// # Returns
///
/// Components beginning with the empty Unix root component.
///
/// # Errors
///
/// Returns a `ComposePath` error when `path` is not absolute or has raw dot
/// components.
#[cfg(unix)]
fn to_canonical_absolute_components(path: &Path) -> LocalResult<Vec<String>> {
    if !path.is_absolute() || has_disallowed_component(path) {
        return Err(invalid_path_error());
    }
    let mut native_components = path.components();
    if !matches!(native_components.next(), Some(Component::RootDir)) {
        return Err(invalid_path_error());
    }
    let mut encoded = vec![String::new()];
    for component in native_components {
        let Component::Normal(component) = component else {
            return Err(invalid_path_error());
        };
        encoded.push(encode_native_component(component)?);
    }
    debug_assert!(matches!(
        LocalPaths::from_canonical_absolute_components(
            encoded.iter().map(String::as_str),
        ),
        Ok(decoded) if decoded == path
    ));
    Ok(encoded)
}

/// Builds a Windows drive-rooted path from canonical path components.
///
/// # Parameters
///
/// - `components`: Canonical components beginning with an empty root marker and
///   drive component.
///
/// # Returns
///
/// The decoded absolute native path.
///
/// # Errors
///
/// Returns a `ComposePath` error when the root marker, drive, or any component
/// is malformed.
#[cfg(windows)]
#[inline(never)]
fn from_canonical_absolute_components<'a>(
    components: impl IntoIterator<Item = &'a str>,
) -> LocalResult<PathBuf> {
    let mut components = components.into_iter();
    let (Some(root), Some(drive)) = (components.next(), components.next())
    else {
        return Err(invalid_path_error());
    };
    let native_drive = decode_canonical_component(drive)?;
    if !root.is_empty()
        || !is_windows_drive_component(drive)
        || native_drive != drive
    {
        return Err(invalid_path_error());
    }
    let mut path = PathBuf::from(format!("{drive}\\\\"));
    for component in components {
        path.push(decode_normal_component(component)?);
    }
    Ok(path)
}

/// Reports whether text is the sole supported Windows canonical drive root.
///
/// # Parameters
///
/// - `component`: Canonical component expected to contain a drive designator.
///
/// # Returns
///
/// `true` only for one ASCII letter followed by a colon.
#[cfg(windows)]
#[inline]
fn is_windows_drive_component(component: &str) -> bool {
    matches!(component.as_bytes(), [letter, b':'] if letter.is_ascii_alphabetic())
}

/// Encodes a Windows drive-rooted path into canonical components.
///
/// # Parameters
///
/// - `path`: Windows path to encode.
///
/// # Returns
///
/// Components beginning with an empty root marker and a canonical drive
/// component.
///
/// # Errors
///
/// Returns a `ComposePath` unsupported error for UNC, device, and rooted
/// relative paths; returns invalid input for malformed lexical descendants.
#[cfg(windows)]
fn to_canonical_absolute_components(path: &Path) -> LocalResult<Vec<String>> {
    if !path.is_absolute() || has_disallowed_component(path) {
        return Err(invalid_path_error());
    }
    let mut native_components = path.components();
    let Some(Component::Prefix(prefix)) = native_components.next() else {
        return Err(invalid_path_error());
    };
    if !matches!(prefix.kind(), std::path::Prefix::Disk(_)) {
        return Err(LocalFileError::new(
            LocalFileErrorKind::Unsupported,
            LocalFileOperation::ComposePath,
        ));
    }
    if !matches!(native_components.next(), Some(Component::RootDir)) {
        return Err(invalid_path_error());
    }
    let drive = encode_native_component(prefix.as_os_str())?;
    if !is_windows_drive_component(&drive) {
        return Err(invalid_path_error());
    }
    let mut encoded = vec![String::new(), drive];
    for component in native_components {
        let Component::Normal(component) = component else {
            return Err(invalid_path_error());
        };
        encoded.push(encode_native_component(component)?);
    }
    Ok(encoded)
}

/// Rejects absolute canonical conversion on unsupported native targets.
///
/// # Parameters
///
/// - `_components`: Canonical components ignored on unsupported targets.
///
/// # Returns
///
/// Never returns a native path.
///
/// # Errors
///
/// Always returns a `ComposePath` unsupported error.
#[cfg(not(any(unix, windows)))]
#[inline(never)]
fn from_canonical_absolute_components<'a>(
    _components: impl IntoIterator<Item = &'a str>,
) -> LocalResult<PathBuf> {
    Err(LocalFileError::new(
        LocalFileErrorKind::Unsupported,
        LocalFileOperation::ComposePath,
    ))
}

/// Rejects absolute canonical conversion on unsupported native targets.
///
/// # Parameters
///
/// - `path`: Ignored native path.
///
/// # Returns
///
/// Never returns canonical components.
///
/// # Errors
///
/// Always returns a `ComposePath` unsupported-platform error.
#[cfg(not(any(unix, windows)))]
#[inline(always)]
fn to_canonical_absolute_components(_path: &Path) -> LocalResult<Vec<String>> {
    Err(LocalFileError::new(
        LocalFileErrorKind::Unsupported,
        LocalFileOperation::ComposePath,
    ))
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
