// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
use std::sync::Arc;

use crate::LocalSymlinkPolicy;
use crate::directory::LocalComponentPolicy;
use crate::directory::LocalResolutionPolicy;
use crate::options::LocalPathBindingOptions;
use crate::options::LocalResolutionLimits;

/// Options captured by a fixed directory handle.
///
/// Opening a handle performs native resolution and I/O. These builders only
/// store the policy that the opening operation will apply. A symlink override
/// replaces the enclosing default for operations through the new handle;
/// component denials, resolution limits, and validation policies combine with
/// enclosing constraints and can only restrict resolution. Optional path
/// binding retains evidence for later verification, but does not lock the
/// namespace or prevent concurrent changes.
#[derive(Clone, Debug, Default)]
#[must_use = "directory options have no effect unless used"]
pub struct LocalDirectoryOpenOptions {
    /// Optional policy override; `None` inherits the filesystem policy.
    symlink_policy: Option<LocalSymlinkPolicy>,
    /// Additional denied components applied while resolving paths.
    component_policy: LocalComponentPolicy,
    /// Optional binding limits for the directory's opening name.
    path_binding: Option<LocalPathBindingOptions>,
    /// Optional limits combined with enclosing resolution limits.
    resolution_limits: Option<LocalResolutionLimits>,
    /// Optional pure validation policy combined with enclosing policies.
    resolution_policy: Option<Arc<dyn LocalResolutionPolicy>>,
}

impl LocalDirectoryOpenOptions {
    /// Creates options that inherit the filesystem's symlink policy.
    pub fn new() -> Self {
        Self::default()
    }

    /// Sets the symlink policy used by operations through the handle.
    pub fn with_symlink_policy(mut self, policy: LocalSymlinkPolicy) -> Self {
        self.symlink_policy = Some(policy);
        self
    }

    /// Sets additional native component names to reject while binding paths
    /// through the opened handle. When opening a child directory from an
    /// existing handle, its denied names are combined with the existing
    /// handle's denied names. This does not filter names discovered during
    /// recursive listing or copying, or provide isolation from concurrent
    /// namespace changes.
    pub fn with_component_policy(mut self, policy: LocalComponentPolicy) -> Self {
        self.component_policy = policy;
        self
    }

    /// Enables retaining a binding for the handle's opening path.
    pub fn with_path_binding(mut self, options: LocalPathBindingOptions) -> Self {
        self.path_binding = Some(options);
        self
    }

    /// Sets limits for resolving this directory's opening path.
    ///
    /// When opening a child from an existing handle, the limits are combined
    /// with the handle's limits and can only make resolution more restrictive.
    pub fn with_resolution_limits(mut self, limits: LocalResolutionLimits) -> Self {
        self.resolution_limits = Some(limits);
        self
    }
    /// Adds a pure component and symlink-target validation policy.
    ///
    /// The policy is combined with policies from an enclosing handle, so all
    /// policies must accept each component or target that is resolved.
    pub fn with_resolution_policy(mut self, policy: Arc<dyn LocalResolutionPolicy>) -> Self {
        self.resolution_policy = Some(policy);
        self
    }

    /// Returns the optional symlink override without performing I/O.
    pub(crate) const fn symlink_policy(&self) -> Option<LocalSymlinkPolicy> {
        self.symlink_policy
    }
    /// Returns component restrictions without performing I/O.
    pub(crate) fn component_policy(&self) -> &LocalComponentPolicy {
        &self.component_policy
    }
    /// Returns optional path-binding limits without performing I/O.
    pub(crate) const fn path_binding(&self) -> Option<&LocalPathBindingOptions> {
        self.path_binding.as_ref()
    }
    /// Returns optional resolution limits without performing I/O.
    pub(crate) const fn resolution_limits(&self) -> Option<LocalResolutionLimits> {
        self.resolution_limits
    }
    /// Returns the optional policy handle without performing I/O.
    pub(crate) fn resolution_policy(&self) -> Option<&Arc<dyn LocalResolutionPolicy>> {
        self.resolution_policy.as_ref()
    }
}
