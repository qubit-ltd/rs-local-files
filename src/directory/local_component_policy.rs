// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
use std::ffi::OsString;
use std::path::Component;
use std::path::Path;

use crate::LocalFileError;
use crate::LocalFileErrorKind;
use crate::LocalFileOperation;
use crate::LocalResult;

/// Exact native names rejected while binding paths through a fixed directory.
///
/// Comparisons use native name equality; this policy does not add
/// case-folding. It checks components supplied to handle operations and names
/// encountered while resolving those paths. It does not filter names
/// discovered by recursive listing or copying, and it does not isolate names
/// from concurrent namespace changes. In particular, it is not a private
/// subtree boundary. Windows host directory handles currently return
/// `Unsupported` when opened with a custom component policy.
///
/// # Examples
///
/// ```
/// use std::ffi::OsString;
/// use std::path::Path;
///
/// use qubit_local_files::LocalFileSystem;
/// use qubit_local_files::directory::LocalComponentPolicy;
/// use qubit_local_files::error::LocalFileErrorKind;
/// use qubit_local_files::options::LocalDirectoryOpenOptions;
///
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let scratch = tempfile::tempdir()?;
/// std::fs::create_dir(scratch.path().join("blocked"))?;
/// let filesystem = LocalFileSystem::host()?;
/// let options = LocalDirectoryOpenOptions::new().with_component_policy(
///     LocalComponentPolicy::deny_names(vec![OsString::from("blocked")])?,
/// );
/// #[cfg(not(windows))]
/// {
///     let directory = filesystem.open_directory_with_options(scratch.path(), &options)?;
///     let error = directory.entry_metadata(Path::new("blocked")).unwrap_err();
///     assert_eq!(error.kind(), LocalFileErrorKind::InvalidPath);
/// }
/// #[cfg(windows)]
/// {
///     let error = filesystem.open_directory_with_options(scratch.path(), &options).unwrap_err();
///     assert_eq!(error.kind(), LocalFileErrorKind::Unsupported);
/// }
/// # Ok(())
/// # }
/// ```
#[must_use = "a component policy has no effect unless it is used"]
#[derive(Clone, Debug, Default)]
pub struct LocalComponentPolicy {
    /// Native component names rejected by this policy.
    denied: Vec<OsString>,
}

impl LocalComponentPolicy {
    /// Allows every component name.
    ///
    /// # Returns
    ///
    /// A policy that rejects no component names.
    #[inline]
    pub const fn allow_all() -> Self {
        Self { denied: Vec::new() }
    }

    /// Rejects the exact native names supplied by the caller.
    ///
    /// Names are compared using the platform's native name equality, without
    /// additional case-folding. An empty name, `.`, `..`, or a value that is
    /// not exactly one normal path component returns `InvalidOptions` with
    /// `BindPath` context. The resulting policy applies to components used by
    /// handle operations and path resolution; it does not filter names found
    /// by recursive discovery.
    ///
    /// # Parameters
    ///
    /// - `names`: Native names to reject. Each value must be one non-empty
    ///   normal path component other than `.` or `..`.
    ///
    /// # Returns
    ///
    /// A policy containing the validated denied names.
    ///
    /// # Errors
    ///
    /// Returns `InvalidOptions` with `BindPath` context when any name violates
    /// the component constraint.
    pub fn deny_names(names: Vec<OsString>) -> LocalResult<Self> {
        for name in &names {
            let mut components = Path::new(name).components();
            let one_normal_component = matches!(components.next(), Some(Component::Normal(value)) if value == name)
                && components.next().is_none();
            if name.is_empty() || name == "." || name == ".." || !one_normal_component {
                return Err(
                    LocalFileError::new(LocalFileErrorKind::InvalidOptions, LocalFileOperation::BindPath)
                        .with_reason("denied component names must be non-empty single native names other than . or .."),
                );
            }
        }
        Ok(Self { denied: names })
    }

    /// Reports whether `component` exactly matches a denied native name.
    ///
    /// # Parameters
    ///
    /// - `component`: Native path component to compare against the denied
    ///   names.
    ///
    /// # Returns
    ///
    /// `true` when the component exactly matches a denied name.
    #[must_use]
    pub(crate) fn denies(&self, component: &std::ffi::OsStr) -> bool {
        self.denied.iter().any(|name| name == component)
    }

    /// Reports whether the policy permits every component name.
    ///
    /// # Returns
    ///
    /// `true` when no component names are denied.
    #[cfg(not(unix))]
    #[must_use]
    pub(crate) fn is_allow_all(&self) -> bool {
        self.denied.is_empty()
    }

    /// Combines parent and child restrictions, retaining each denied name once.
    ///
    /// # Parameters
    ///
    /// - `other`: Additional policy whose denied names are merged with this
    ///   policy.
    ///
    /// # Returns
    ///
    /// A policy containing the unique union of both denied-name lists.
    pub(crate) fn combined_with(&self, other: &Self) -> Self {
        let mut denied = self.denied.clone();
        for name in &other.denied {
            if !denied.iter().any(|existing| existing == name) {
                denied.push(name.clone());
            }
        }
        Self { denied }
    }

    /// Accepts an allowed component or returns `InvalidPath` for a denied one.
    ///
    /// # Parameters
    ///
    /// - `component`: Native path component to validate against this policy.
    ///
    /// # Returns
    ///
    /// `Ok(())` when the component is allowed.
    ///
    /// # Errors
    ///
    /// Returns `InvalidPath` with `BindPath` context when the component is
    /// denied.
    pub(crate) fn validate_component(&self, component: &std::ffi::OsStr) -> LocalResult<()> {
        if self.denies(component) {
            return Err(
                LocalFileError::new(LocalFileErrorKind::InvalidPath, LocalFileOperation::BindPath)
                    .with_reason("path component is rejected by the configured component policy"),
            );
        }
        Ok(())
    }
}
