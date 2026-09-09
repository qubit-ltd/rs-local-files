// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Stateful-PWD binding for Host and Rooted native paths.

use std::ffi::OsString;
use std::path::Component;
use std::path::Path;
use std::path::PathBuf;

use super::LocalNamespacePath;
use crate::LocalFileError;
use crate::LocalFileErrorKind;
use crate::LocalFileOperation;
use crate::LocalFileSystemScope;
use crate::LocalPathCodecError;
use crate::LocalResult;

/// Resolves operation inputs against one normalized filesystem PWD snapshot.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LocalPathResolver {
    /// Namespace whose anchoring rules are applied to operation paths.
    scope: LocalFileSystemScope,
    /// Namespace-absolute PWD snapshot supplied at construction.
    current_directory: Option<PathBuf>,
    /// Normal components retained from the normalized PWD.
    current_components: Vec<OsString>,
    /// Host-native prefix retained separately from normal components.
    current_prefix: Option<OsString>,
}

impl LocalPathResolver {
    /// Creates a resolver from a normalized namespace-absolute PWD.
    ///
    /// Performs lexical validation without filesystem access. Returns a
    /// typed path-codec error for native NUL, or `InvalidPath` for a missing
    /// namespace root, parent components, or a native prefix in Rooted scope.
    /// Interior dot components normalized away by `Path::components` are
    /// accepted; the exposed PWD snapshot retains the supplied spelling.
    pub fn new(scope: LocalFileSystemScope, current_directory: &Path) -> LocalResult<Self> {
        reject_native_nul(current_directory)?;
        let (current_prefix, current_components) = parse_current_directory(scope, current_directory)?;
        Ok(Self {
            scope,
            current_directory: Some(current_directory.to_path_buf()),
            current_components,
            current_prefix,
        })
    }

    /// Returns the PWD snapshot used by this resolver.
    #[must_use]
    // qubit-style: allow coverage-cfg
    #[cfg_attr(not(coverage), inline(always))]
    #[cfg_attr(coverage, inline(never))]
    pub fn current_directory(&self) -> Option<&Path> {
        self.current_directory.as_deref()
    }

    /// Creates a Host resolver that can resolve absolute paths without a PWD.
    #[must_use]
    pub(crate) const fn absolute_host() -> Self {
        Self {
            scope: LocalFileSystemScope::Host,
            current_directory: None,
            current_components: Vec::new(),
            current_prefix: None,
        }
    }

    /// Binds one absolute or PWD-relative operation path.
    ///
    /// Preserves Host component spelling for native traversal, including
    /// dots and parents. Rooted inputs are normalized lexically without I/O
    /// and absolute inputs start at the virtual namespace root. Empty
    /// inputs and trailing separators or dots retain directory intent.
    ///
    /// Returns a typed path-codec error for native NUL, `InvalidPath` for
    /// traversal above the namespace root, Rooted native prefixes, or
    /// ambiguous Host drive-relative paths. A relative Host path without a
    /// PWD snapshot returns `InvalidState`.
    pub fn resolve(&self, path: &Path) -> LocalResult<LocalNamespacePath> {
        reject_native_nul(path)?;
        if self.current_directory.is_none() && !path.is_absolute() {
            return Err(
                LocalFileError::new(LocalFileErrorKind::InvalidState, LocalFileOperation::BindPath)
                    .with_path(path.to_path_buf())
                    .with_reason("a relative Host path requires a process current directory snapshot"),
            );
        }
        let directory_required = directory_required(path);
        if self.scope == LocalFileSystemScope::Host {
            let bound = bind_host_operand(path, self.current_directory.as_deref())?;
            return Ok(LocalNamespacePath::new(bound.clone(), bound, directory_required));
        }
        let mut components = self.current_components.clone();
        if path
            .components()
            .any(|component| matches!(component, Component::Prefix(_)))
        {
            return Err(invalid_path(
                path,
                "native prefixes are not valid in a Rooted namespace",
            ));
        }
        if path.has_root() {
            components.clear();
        }

        for component in path.components() {
            match component {
                Component::Prefix(_) | Component::RootDir | Component::CurDir => {}
                Component::Normal(name) => components.push(name.to_os_string()),
                Component::ParentDir => {
                    if components.pop().is_none() {
                        return Err(invalid_path(
                            path,
                            "path traversal escaped the filesystem namespace root",
                        ));
                    }
                }
            }
        }

        let namespace_absolute = namespace_absolute(self.scope, self.current_prefix.as_deref(), &components);
        let authority_relative = match self.scope {
            LocalFileSystemScope::Host => namespace_absolute.clone(),
            LocalFileSystemScope::Rooted => components.iter().collect(),
        };
        Ok(LocalNamespacePath::new(
            namespace_absolute,
            authority_relative,
            directory_required,
        ))
    }
}

/// Extracts the prefix and normal components of a namespace-absolute PWD.
///
/// Returns `InvalidPath` for a missing namespace root or any prefix, dot,
/// or parent component disallowed by the scope and exposed by
/// `Path::components`; that iterator may normalize interior dots away.
fn parse_current_directory(
    scope: LocalFileSystemScope,
    current_directory: &Path,
) -> LocalResult<(Option<OsString>, Vec<OsString>)> {
    if !has_namespace_root(scope, current_directory) {
        return Err(invalid_path(
            current_directory,
            "filesystem current directory must be namespace-absolute",
        ));
    }
    let mut prefix = None;
    let mut components = Vec::new();
    for component in current_directory.components() {
        match component {
            Component::Prefix(value) if scope == LocalFileSystemScope::Host => {
                prefix = Some(value.as_os_str().to_os_string());
            }
            Component::RootDir => {}
            Component::Normal(name) => components.push(name.to_os_string()),
            Component::Prefix(_) | Component::CurDir | Component::ParentDir => {
                return Err(invalid_path(
                    current_directory,
                    "filesystem current directory is not normalized",
                ));
            }
        }
    }
    Ok((prefix, components))
}

/// Binds a Host operand without rebuilding or simplifying its components.
///
/// Absolute inputs need no PWD. Relative inputs require `pwd`; Windows
/// root-relative inputs retain its native prefix. Ambiguous drive-relative
/// input returns `InvalidPath`, and a missing PWD returns `InvalidState`.
fn bind_host_operand(path: &Path, pwd: Option<&Path>) -> LocalResult<PathBuf> {
    if path.is_absolute() {
        return Ok(path.to_path_buf());
    }
    if path
        .components()
        .any(|component| matches!(component, Component::Prefix(_)))
    {
        return Err(invalid_path(path, "drive-relative Host paths are ambiguous"));
    }
    let pwd = pwd.ok_or_else(|| {
        LocalFileError::new(LocalFileErrorKind::InvalidState, LocalFileOperation::BindPath)
            .with_path(path.to_path_buf())
            .with_reason("a relative Host path requires a process current directory snapshot")
    })?;
    #[cfg(windows)]
    if path.has_root() {
        let mut bound = match pwd.components().next() {
            Some(Component::Prefix(prefix)) => prefix.as_os_str().to_os_string(),
            _ => return Err(invalid_path(path, "root-relative Host paths require a drive anchor")),
        };
        bound.push(path.as_os_str());
        return Ok(PathBuf::from(bound));
    }
    let mut bound = pwd.as_os_str().to_os_string();
    if !path.as_os_str().is_empty() {
        bound.push(std::path::MAIN_SEPARATOR_STR);
        bound.push(path.as_os_str());
    }
    Ok(PathBuf::from(bound))
}

/// Reports whether a path has the namespace root syntax required for a PWD.
fn has_namespace_root(scope: LocalFileSystemScope, path: &Path) -> bool {
    match scope {
        LocalFileSystemScope::Host => path.is_absolute(),
        LocalFileSystemScope::Rooted => {
            path.has_root()
                && !path
                    .components()
                    .any(|component| matches!(component, Component::Prefix(_)))
        }
    }
}

/// Builds one namespace-absolute path without converting components to text.
fn namespace_absolute(
    scope: LocalFileSystemScope,
    prefix: Option<&std::ffi::OsStr>,
    components: &[OsString],
) -> PathBuf {
    let mut result = PathBuf::new();
    if scope == LocalFileSystemScope::Host
        && let Some(prefix) = prefix
    {
        result.push(prefix);
    }
    result.push(std::path::MAIN_SEPARATOR_STR);
    for component in components {
        result.push(component);
    }
    result
}

/// Creates one structured lexical-path error.
fn invalid_path(path: &Path, reason: &'static str) -> LocalFileError {
    LocalFileError::new(LocalFileErrorKind::InvalidPath, LocalFileOperation::BindPath)
        .with_path(path.to_path_buf())
        .with_reason(reason)
}

/// Rejects embedded native NUL without lossy text conversion.
fn reject_native_nul(path: &Path) -> LocalResult<()> {
    if contains_native_nul(path) {
        return Err(LocalFileError::from_path_codec(
            LocalFileOperation::BindPath,
            Some(path.to_path_buf()),
            LocalPathCodecError::NativeNul,
        ));
    }
    Ok(())
}

/// Reports whether native syntax requires the normalized entry to be a
/// directory.
fn directory_required(path: &Path) -> bool {
    path.as_os_str().is_empty() || has_trailing_separator_or_dot(path)
}

/// Reports whether a Unix path contains a native NUL byte.
#[cfg(unix)]
fn contains_native_nul(path: &Path) -> bool {
    use std::os::unix::ffi::OsStrExt;

    path.as_os_str().as_bytes().contains(&0)
}

/// Reports whether a Windows path contains a native NUL code unit.
#[cfg(windows)]
fn contains_native_nul(path: &Path) -> bool {
    use std::os::windows::ffi::OsStrExt;

    path.as_os_str().encode_wide().any(|unit| unit == 0)
}

/// Conservatively rejects paths on unsupported platforms.
#[cfg(not(any(unix, windows)))]
const fn contains_native_nul(_path: &Path) -> bool {
    true
}

/// Reports Unix syntax that preserves directory-qualified intent.
#[cfg(unix)]
fn has_trailing_separator_or_dot(path: &Path) -> bool {
    use std::os::unix::ffi::OsStrExt;

    let bytes = path.as_os_str().as_bytes();
    if bytes.last() == Some(&b'/') {
        return true;
    }
    let final_component = bytes.rsplit(|byte| *byte == b'/').next().unwrap_or_default();
    final_component == b"." || final_component == b".."
}

/// Reports Windows syntax that preserves directory-qualified intent.
#[cfg(windows)]
fn has_trailing_separator_or_dot(path: &Path) -> bool {
    use std::os::windows::ffi::OsStrExt;

    let units = path.as_os_str().encode_wide().collect::<Vec<_>>();
    let is_separator = |unit: &u16| *unit == u16::from(b'/') || *unit == u16::from(b'\\');
    if units.last().is_some_and(is_separator) {
        return true;
    }
    let final_component = units.rsplit(is_separator).next().unwrap_or_default();
    final_component == [u16::from(b'.')] || final_component == [u16::from(b'.'), u16::from(b'.')]
}

/// Returns false on unsupported platforms, whose paths are rejected earlier.
#[cfg(not(any(unix, windows)))]
const fn has_trailing_separator_or_dot(_path: &Path) -> bool {
    false
}
