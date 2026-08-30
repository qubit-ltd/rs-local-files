// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::ffi::OsStr;
use std::ffi::OsString;
use std::path::Component;
use std::path::Path;
use std::path::PathBuf;

use crate::LocalFileError;
use crate::LocalFileErrorKind;
use crate::LocalFileNames;
use crate::LocalFileOperation;
use crate::LocalFileSystemScope;
use crate::LocalPathCodec;
use crate::LocalResult;
use crate::RelativePath;

/// Scope-bound native path validation and canonical conversion.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[must_use]
pub struct LocalPaths {
    /// Namespace in which native paths are interpreted.
    scope: LocalFileSystemScope,
    /// Filename policy associated with the namespace.
    names: LocalFileNames,
}

impl LocalPaths {
    /// Creates path operations for the process-visible host namespace.
    #[inline(always)]
    pub const fn host() -> Self {
        Self {
            scope: LocalFileSystemScope::Host,
            names: LocalFileNames::native(),
        }
    }

    /// Creates path operations for authority-relative rooted paths.
    #[inline(always)]
    pub const fn rooted() -> Self {
        Self {
            scope: LocalFileSystemScope::Rooted,
            names: LocalFileNames::native(),
        }
    }

    /// Returns the namespace interpreted by this path object.
    #[inline(always)]
    pub const fn scope(&self) -> LocalFileSystemScope {
        self.scope
    }

    /// Returns the native filename policy for this path namespace.
    #[inline(always)]
    pub const fn file_names(&self) -> LocalFileNames {
        self.names
    }

    /// Decodes canonical components in the selected filesystem scope.
    ///
    /// Host paths are absolute and omit an artificial root marker. Rooted
    /// paths are relative descendants, with an empty component sequence
    /// representing the opened authority root.
    pub fn from_canonical_components<'a>(&self, components: impl IntoIterator<Item = &'a str>) -> LocalResult<PathBuf> {
        match self.scope {
            LocalFileSystemScope::Host => from_canonical_host_components(components),
            LocalFileSystemScope::Rooted => {
                let mut path = PathBuf::new();
                for component in components {
                    path.push(decode_normal_component(component)?);
                }
                self.validate_native_form(&path)?;
                Ok(path)
            }
        }
    }

    /// Encodes a native path as canonical components in the selected scope.
    ///
    /// Host output contains the platform root authority; rooted output is
    /// relative and is empty for the authority root.
    pub fn to_canonical_components(&self, path: &Path) -> LocalResult<Vec<String>> {
        match self.scope {
            LocalFileSystemScope::Host => to_canonical_host_components(path),
            LocalFileSystemScope::Rooted => {
                let relative = RelativePath::parse(path)?;
                encode_normal_components(relative.as_path())
            }
        }
    }

    /// Validates a native path against this object's namespace shape.
    ///
    /// # Parameters
    ///
    /// - `path`: Native path to validate.
    ///
    /// # Errors
    ///
    /// Returns an invalid-path error when a rooted path is absolute, prefixed,
    /// or contains a dot or parent component.
    #[inline]
    fn validate_native_form(&self, path: &Path) -> LocalResult<()> {
        match self.scope {
            LocalFileSystemScope::Host => Ok(()),
            LocalFileSystemScope::Rooted => RelativePath::parse(path).map(|_| ()),
        }
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
    path.components()
        .any(|component| matches!(component, Component::CurDir | Component::ParentDir))
        || has_raw_dot_component(path)
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
    LocalFileError::new(LocalFileErrorKind::InvalidPath, LocalFileOperation::ComposePath)
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
    LocalPathCodec::decode_component(component)
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
        && matches!(Path::new(component).components().next(), Some(Component::Normal(_)))
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
    LocalPathCodec::encode_component(component)
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

/// Builds a Unix Host path from canonical path components.
///
/// # Parameters
///
/// - `components`: Canonical descendant components; an empty sequence is `/`.
///
/// # Returns
///
/// The decoded absolute native path.
///
/// # Errors
///
/// Returns a `ComposePath` error when any component is malformed.
#[cfg(unix)]
#[inline(never)]
fn from_canonical_host_components<'a>(components: impl IntoIterator<Item = &'a str>) -> LocalResult<PathBuf> {
    let mut path = PathBuf::from("/");
    for component in components {
        path.push(decode_normal_component(component)?);
    }
    Ok(path)
}

/// Encodes a Unix Host path into canonical components.
///
/// # Parameters
///
/// - `path`: Unix path to encode.
///
/// # Returns
///
/// Components contain only normal descendants; `/` is represented by `[]`.
///
/// # Errors
///
/// Returns a `ComposePath` error when `path` is not absolute or has raw dot
/// components.
#[cfg(unix)]
fn to_canonical_host_components(path: &Path) -> LocalResult<Vec<String>> {
    if !path.is_absolute() || has_disallowed_component(path) {
        return Err(invalid_path_error());
    }
    let mut native_components = path.components();
    if !matches!(native_components.next(), Some(Component::RootDir)) {
        return Err(invalid_path_error());
    }
    let mut encoded = Vec::new();
    for component in native_components {
        let Component::Normal(component) = component else {
            return Err(invalid_path_error());
        };
        encoded.push(encode_native_component(component)?);
    }
    debug_assert!(matches!(
        LocalPaths::host()
            .from_canonical_components(encoded.iter().map(String::as_str)),
        Ok(decoded) if decoded == path
    ));
    Ok(encoded)
}

/// Builds a Windows Host path from canonical path components.
///
/// # Parameters
///
/// - `components`: Canonical components beginning with a drive component.
///
/// # Returns
///
/// The decoded absolute native path.
///
/// # Errors
///
/// Returns a `ComposePath` error when the drive or any component is malformed.
#[cfg(windows)]
#[inline(never)]
fn from_canonical_host_components<'a>(components: impl IntoIterator<Item = &'a str>) -> LocalResult<PathBuf> {
    let mut components = components.into_iter();
    let Some(drive) = components.next() else {
        return Err(invalid_path_error());
    };
    let native_drive = decode_canonical_component(drive)?;
    if !is_windows_drive_component(drive) || native_drive != drive {
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

/// Encodes a Windows Host path into canonical components.
///
/// # Parameters
///
/// - `path`: Windows path to encode.
///
/// # Returns
///
/// Components beginning with a canonical drive component.
///
/// # Errors
///
/// Returns a `ComposePath` unsupported error for UNC, device, and rooted
/// relative paths; returns invalid input for malformed lexical descendants.
#[cfg(windows)]
fn to_canonical_host_components(path: &Path) -> LocalResult<Vec<String>> {
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
    let mut encoded = vec![drive];
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
fn from_canonical_host_components<'a>(_components: impl IntoIterator<Item = &'a str>) -> LocalResult<PathBuf> {
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
fn to_canonical_host_components(_path: &Path) -> LocalResult<Vec<String>> {
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
        .any(|component| component == [b'.' as u16] || component == [b'.' as u16; 2])
}

/// Detects raw dot components on unsupported native targets.
#[cfg(not(any(unix, windows)))]
#[inline(always)]
const fn has_raw_dot_component(_path: &Path) -> bool {
    false
}
