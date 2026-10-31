// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::ffi::OsStr;
use std::path::Path;

use crate::LocalResult;

use super::LocalResolutionOrigin;
use super::LocalResolutionScope;

/// Pure validation hooks applied before the resolver accesses a component or
/// target.
///
/// Implementations should be deterministic and must not perform filesystem
/// operations. The library may invoke hooks more than once while resolving or
/// rechecking a binding; it does not promise a particular invocation count.
///
/// # Examples
///
/// ```
/// use std::ffi::OsStr;
/// use std::path::Path;
/// use std::sync::Arc;
///
/// use qubit_local_files::directory::{
///     LocalResolutionOrigin, LocalResolutionPolicy, LocalResolutionScope,
/// };
/// use qubit_local_files::error::{LocalFileError, LocalFileErrorKind, LocalFileOperation};
/// use qubit_local_files::options::LocalRootOpenOptions;
/// use qubit_local_files::{LocalFileSystem, LocalResult};
///
/// #[derive(Debug)]
/// struct RejectForbidden;
///
/// impl LocalResolutionPolicy for RejectForbidden {
///     fn validate_component(
///         &self,
///         component: &OsStr,
///         _scope: LocalResolutionScope,
///         _origin: LocalResolutionOrigin,
///     ) -> LocalResult<()> {
///         if component == OsStr::new("forbidden") {
///             Err(LocalFileError::new(
///                 LocalFileErrorKind::InvalidPath,
///                 LocalFileOperation::BindPath,
///             ))
///         } else {
///             Ok(())
///         }
///     }
///
///     fn validate_symlink_target(
///         &self,
///         _target: &Path,
///         _scope: LocalResolutionScope,
///     ) -> LocalResult<()> {
///         Ok(())
///     }
/// }
///
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let fixture = tempfile::tempdir()?;
/// std::fs::write(fixture.path().join("forbidden"), b"present")?;
/// let options = LocalRootOpenOptions::new()
///     .with_resolution_policy(Arc::new(RejectForbidden));
/// let filesystem = LocalFileSystem::rooted_with_options(fixture.path(), &options)?;
/// let result = filesystem.open_reader(Path::new("forbidden"));
/// assert!(matches!(result, Err(ref error) if error.kind() == LocalFileErrorKind::InvalidPath));
/// # Ok(())
/// # }
/// ```
pub trait LocalResolutionPolicy: std::fmt::Debug + Send + Sync {
    /// Validates a normal component before the resolver performs metadata or
    /// open calls.
    fn validate_component(
        &self,
        component: &OsStr,
        scope: LocalResolutionScope,
        origin: LocalResolutionOrigin,
    ) -> LocalResult<()>;
    /// Validates the unnormalized native target before the resolver interprets
    /// its components.
    fn validate_symlink_target(&self, target: &Path, scope: LocalResolutionScope) -> LocalResult<()>;
}
