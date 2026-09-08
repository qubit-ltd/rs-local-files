// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Stateful Host or Rooted local filesystem service.
// qubit-style: allow source-test-pair

use super::LocalFileError;
use super::LocalFileOperation;
use super::LocalFileSystem;
use super::LocalFileSystemCapabilities;
use super::LocalFileSystemLimits;
use super::LocalFileSystemSpace;
use super::LocalNamespace;
use super::LocalNamespacePath;
use super::LocalResult;
use super::LocalSymlinkPolicy;
use super::Path;
use super::fs;
use super::operation_error;
use super::resolve_operation_path;

impl LocalFileSystem {
    /// Returns the immutable capability snapshot for this authority.
    #[must_use = "inspect the capability snapshot"]
    // qubit-style: allow coverage-cfg
    #[cfg_attr(not(coverage), inline)]
    #[cfg_attr(coverage, inline(never))]
    pub fn capabilities(&self) -> LocalFileSystemCapabilities {
        self.core.capabilities
    }

    /// Returns authority-level objective path-limit observations.
    #[must_use = "inspect the path-limit observations"]
    #[cfg_attr(not(coverage), inline)]
    #[cfg_attr(coverage, inline(never))]
    pub fn limits(&self) -> LocalFileSystemLimits {
        self.core.limits
    }

    /// Observes path limits at the requested path or nearest existing ancestor.
    ///
    /// Returns namespace-resolution, symlink-policy, handle-open, or native
    /// probe errors. Unsupported individual limits remain unknown in the
    /// returned observation.
    pub fn limits_at(&self, path: &Path) -> LocalResult<LocalFileSystemLimits> {
        let resolver = self.resolver_for(path, LocalFileOperation::Capabilities)?;
        let resolved = resolve_operation_path(&resolver, path, LocalFileOperation::Capabilities)?;
        match &self.core.namespace {
            LocalNamespace::Host => host_probe(&resolved, self.symlink_policy, crate::capability::probe_limits)
                .map_err(|error| {
                    operation_error(
                        error,
                        LocalFileOperation::Capabilities,
                        resolved.namespace_absolute(),
                        None,
                        resolver.current_directory(),
                    )
                }),
            LocalNamespace::Rooted(rooted) => rooted
                .limits_at(resolved.authority_relative(), self.symlink_policy)
                .map_err(|error| {
                    operation_error(
                        error,
                        LocalFileOperation::Capabilities,
                        resolved.namespace_absolute(),
                        None,
                        resolver.current_directory(),
                    )
                }),
        }
    }

    /// Observes dynamic filesystem capacity at a path or existing ancestor.
    ///
    /// Returns namespace-resolution, symlink-policy, handle-open, or native
    /// probe errors. Capacity is a snapshot and does not reserve free space.
    pub fn space_at(&self, path: &Path) -> LocalResult<LocalFileSystemSpace> {
        let resolver = self.resolver_for(path, LocalFileOperation::Capabilities)?;
        let resolved = resolve_operation_path(&resolver, path, LocalFileOperation::Capabilities)?;
        match &self.core.namespace {
            LocalNamespace::Host => {
                host_probe(&resolved, self.symlink_policy, crate::capability::probe_space).map_err(|error| {
                    operation_error(
                        error,
                        LocalFileOperation::Capabilities,
                        resolved.namespace_absolute(),
                        None,
                        resolver.current_directory(),
                    )
                })
            }
            LocalNamespace::Rooted(rooted) => rooted
                .space_at(resolved.authority_relative(), self.symlink_policy)
                .map_err(|error| {
                    operation_error(
                        error,
                        LocalFileOperation::Capabilities,
                        resolved.namespace_absolute(),
                        None,
                        resolver.current_directory(),
                    )
                }),
        }
    }
}

/// Probes the nearest existing Host path after applying symlink policy.
///
/// Retries missing paths at their parent. Other resolution, open, or supplied
/// `probe` errors are returned with capability-operation context.
fn host_probe<T>(
    path: &LocalNamespacePath,
    symlink_policy: LocalSymlinkPolicy,
    probe: fn(&fs::File) -> std::io::Result<T>,
) -> LocalResult<T> {
    let mut candidate = crate::local::resolve_host_path(path.authority_relative(), symlink_policy, true)?;
    loop {
        match open_host_probe(&candidate) {
            Ok(file) => match probe(&file) {
                Ok(value) => return Ok(value),
                Err(error) => {
                    return Err(LocalFileError::from_io(
                        LocalFileOperation::Capabilities,
                        Some(path.namespace_absolute().to_path_buf()),
                        None,
                        error,
                    ));
                }
            },
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                if !candidate.pop() {
                    return Err(LocalFileError::from_io(
                        LocalFileOperation::Capabilities,
                        Some(path.namespace_absolute().to_path_buf()),
                        None,
                        error,
                    ));
                }
            }
            Err(error) => {
                return Err(LocalFileError::from_io(
                    LocalFileOperation::Capabilities,
                    Some(path.namespace_absolute().to_path_buf()),
                    None,
                    error,
                ));
            }
        }
    }
}

/// Opens a Host file or directory for handle-based capability probing.
///
/// Returns native metadata or handle-open errors.
fn open_host_probe(path: &Path) -> std::io::Result<fs::File> {
    let metadata = fs::symlink_metadata(path)?;
    if metadata.is_dir() {
        crate::local::open_root_directory(path)
    } else if metadata.is_file() {
        crate::local::open_native_reader_path(path, &crate::read::OpenOptions::default())
    } else {
        Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "capability probing requires a regular file or directory",
        ))
    }
}
