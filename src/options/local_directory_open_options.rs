// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
use crate::LocalSymlinkPolicy;
use crate::directory::LocalComponentPolicy;
use crate::options::LocalPathBindingOptions;

/// Options captured by a fixed directory handle.
#[derive(Clone, Debug, Default)]
#[must_use = "directory options have no effect unless used"]
pub struct LocalDirectoryOpenOptions {
    symlink_policy: Option<LocalSymlinkPolicy>,
    component_policy: LocalComponentPolicy,
    path_binding: Option<LocalPathBindingOptions>,
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

    /// Sets the native component-name policy applied to relative operands.
    pub fn with_component_policy(mut self, policy: LocalComponentPolicy) -> Self {
        self.component_policy = policy;
        self
    }

    /// Enables retaining a binding for the handle's opening path.
    pub fn with_path_binding(mut self, options: LocalPathBindingOptions) -> Self {
        self.path_binding = Some(options);
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
}
