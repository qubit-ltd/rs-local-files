// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
use crate::LocalSymlinkPolicy;
use crate::directory::LocalComponentPolicy;
use crate::directory::LocalResolutionPolicy;
use crate::options::LocalPathBindingOptions;
use crate::options::LocalResolutionLimits;
use std::sync::Arc;

/// Options captured by a fixed directory handle.
#[derive(Clone, Debug, Default)]
#[must_use = "directory options have no effect unless used"]
pub struct LocalDirectoryOpenOptions {
    symlink_policy: Option<LocalSymlinkPolicy>,
    component_policy: LocalComponentPolicy,
    path_binding: Option<LocalPathBindingOptions>,
    resolution_limits: Option<LocalResolutionLimits>,
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

    pub fn with_resolution_limits(mut self, limits: LocalResolutionLimits) -> Self {
        self.resolution_limits = Some(limits);
        self
    }
    pub fn with_resolution_policy(mut self, policy: Arc<dyn LocalResolutionPolicy>) -> Self {
        self.resolution_policy = Some(policy);
        self
    }

    pub(crate) const fn symlink_policy(&self) -> Option<LocalSymlinkPolicy> {
        self.symlink_policy
    }
    pub(crate) fn component_policy(&self) -> &LocalComponentPolicy {
        &self.component_policy
    }
    pub(crate) const fn path_binding(&self) -> Option<&LocalPathBindingOptions> {
        self.path_binding.as_ref()
    }
    pub(crate) const fn resolution_limits(&self) -> Option<LocalResolutionLimits> {
        self.resolution_limits
    }
    pub(crate) fn resolution_policy(&self) -> Option<&Arc<dyn LocalResolutionPolicy>> {
        self.resolution_policy.as_ref()
    }
}
