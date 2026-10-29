// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================

use std::sync::Arc;

use super::LocalResolutionLimits;
use crate::directory::LocalResolutionPolicy;
use crate::policy::LocalSymlinkPolicy;

/// Configuration for opening a Rooted filesystem with additional resolution
/// rules.
///
/// `with_resolution_limits` replaces the complete limit set stored in these
/// options. At open time, those limits are combined with the Rooted
/// filesystem's other ceilings by taking the tighter value in each dimension;
/// they cannot loosen an existing limit. `None` means no limit from that set,
/// while zero is enforced as a real limit. The Host path is consumed while
/// opening and its budgets do not make the resulting filesystem a containment
/// boundary.
///
/// # Examples
///
/// ```
/// use std::fs;
/// use std::path::Path;
/// use qubit_local_files::options::{LocalResolutionLimits, LocalRootOpenOptions};
/// use qubit_local_files::LocalFileSystem;
///
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let directory = tempfile::tempdir()?;
/// fs::write(directory.path().join("item"), b"rooted")?;
/// let options = LocalRootOpenOptions::new().with_resolution_limits(
///     LocalResolutionLimits::new().with_max_actual_components(8),
/// );
/// let filesystem = LocalFileSystem::rooted_with_options(directory.path(), &options)?;
/// let reader = filesystem.open_reader(Path::new("item"))?;
/// assert_eq!(reader.metadata().len(), 6);
/// # Ok(())
/// # }
/// ```
///
/// # Errors
///
/// [`LocalFileSystem::rooted_with_options`](crate::LocalFileSystem::rooted_with_options)
/// returns an error when opening the root or resolving it exceeds a configured
/// limit, violates a policy, or fails at the native filesystem boundary.
#[derive(Clone, Debug, Default)]
#[must_use]
pub struct LocalRootOpenOptions {
    limits: LocalResolutionLimits,
    symlink: Option<LocalSymlinkPolicy>,
    policy: Option<Arc<dyn LocalResolutionPolicy>>,
}
impl LocalRootOpenOptions {
    /// Creates options with no caller-added resolution limits or policy.
    pub fn new() -> Self {
        Self::default()
    }
    /// Sets per-resolution limits; zero is a real limit and `None` is
    /// unlimited.
    pub fn with_resolution_limits(mut self, limits: LocalResolutionLimits) -> Self {
        self.limits = limits;
        self
    }
    /// Sets symlink handling for opening the root and subsequent rooted
    /// operations.
    pub fn with_symlink_policy(mut self, policy: LocalSymlinkPolicy) -> Self {
        self.symlink = Some(policy);
        self
    }
    /// Adds a pure component and symlink-target validation policy.
    pub fn with_resolution_policy(mut self, policy: Arc<dyn LocalResolutionPolicy>) -> Self {
        self.policy = Some(policy);
        self
    }
    pub(crate) const fn limits(&self) -> LocalResolutionLimits {
        self.limits
    }
    pub(crate) const fn symlink_policy(&self) -> Option<LocalSymlinkPolicy> {
        self.symlink
    }
    pub(crate) fn resolution_policy(&self) -> Option<&Arc<dyn LocalResolutionPolicy>> {
        self.policy.as_ref()
    }
}
