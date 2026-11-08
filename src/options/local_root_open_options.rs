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
///     LocalResolutionLimits::new().with_max_actual_components(64),
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
    /// Additional per-resolution budgets applied when opening a rooted file
    /// system. These limits can tighten existing ceilings but cannot loosen
    /// them.
    limits: LocalResolutionLimits,
    /// Optional symbolic-link behavior for root opening and rooted operations.
    symlink: Option<LocalSymlinkPolicy>,
    /// Optional pure validation hooks for resolved path components and link
    /// targets.
    policy: Option<Arc<dyn LocalResolutionPolicy>>,
}
impl LocalRootOpenOptions {
    /// Creates options with no caller-added resolution limits or policy.
    ///
    /// # Returns
    ///
    /// New options containing no caller-supplied limits or policy overrides.
    #[inline]
    pub fn new() -> Self {
        Self::default()
    }
    /// Sets per-resolution limits; zero is a real limit and `None` is
    /// unlimited.
    ///
    /// # Parameters
    ///
    /// * `limits` — The complete additional budget set to apply. Replaces any
    ///   budget previously stored in these options.
    ///
    /// # Returns
    ///
    /// These options with the supplied complete limit set.
    #[inline]
    pub fn with_resolution_limits(mut self, limits: LocalResolutionLimits) -> Self {
        self.limits = limits;
        self
    }
    /// Sets symlink handling for opening the root and subsequent rooted
    /// operations.
    ///
    /// # Parameters
    ///
    /// * `policy` — The symbolic-link behavior to use for the opened root.
    ///
    /// # Returns
    ///
    /// These options with the supplied symbolic-link behavior.
    #[inline]
    pub fn with_symlink_policy(mut self, policy: LocalSymlinkPolicy) -> Self {
        self.symlink = Some(policy);
        self
    }
    /// Adds a pure component and symlink-target validation policy.
    ///
    /// # Parameters
    ///
    /// * `policy` — A thread-safe policy implementation. Its validation hooks
    ///   should be deterministic and must not perform filesystem operations.
    ///
    /// # Returns
    ///
    /// These options with the supplied resolution validation policy.
    #[inline]
    pub fn with_resolution_policy(mut self, policy: Arc<dyn LocalResolutionPolicy>) -> Self {
        self.policy = Some(policy);
        self
    }
    /// Returns the additional resolution budgets configured for this open.
    ///
    /// # Returns
    ///
    /// The complete additional budget set.
    #[inline]
    pub(crate) const fn limits(&self) -> LocalResolutionLimits {
        self.limits
    }
    /// Returns the configured symbolic-link behavior, if one was supplied.
    ///
    /// `None` means these options do not override the filesystem default.
    ///
    /// # Returns
    ///
    /// `Some(policy)` when these options override symlink handling, or `None`
    /// when the filesystem default applies.
    #[inline]
    pub(crate) const fn symlink_policy(&self) -> Option<LocalSymlinkPolicy> {
        self.symlink
    }
    /// Borrows the configured resolution policy, if one was supplied.
    ///
    /// `None` means no additional component or link-target validation hooks
    /// are configured.
    ///
    /// # Returns
    ///
    /// `Some(policy)` when additional validation hooks are configured, or
    /// `None` otherwise.
    #[inline]
    pub(crate) fn resolution_policy(&self) -> Option<&Arc<dyn LocalResolutionPolicy>> {
        self.policy.as_ref()
    }
}
