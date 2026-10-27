// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
use super::LocalPathBindingOptions;
use super::LocalReadOptions;
use crate::observation::LocalObservationRequirement;

/// Options for opening a reader with same-handle metadata observations.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[must_use = "observed-read options have no effect unless they are used"]
pub struct LocalObservedReadOptions {
    read_options: Option<LocalReadOptions>,
    native_observation: LocalObservationRequirement,
    path_binding: Option<LocalPathBindingOptions>,
}

impl LocalObservedReadOptions {
    /// Creates options with optional native details and no retained path
    /// binding.
    pub const fn new() -> Self {
        Self {
            read_options: None,
            native_observation: LocalObservationRequirement::Optional,
            path_binding: None,
        }
    }

    /// Replaces the filesystem's reader options.
    pub const fn with_read_options(mut self, options: LocalReadOptions) -> Self {
        self.read_options = Some(options);
        self
    }

    /// Sets whether native identity and timestamp details are required.
    pub const fn with_native_observation(mut self, requirement: LocalObservationRequirement) -> Self {
        self.native_observation = requirement;
        self
    }

    /// Enables retaining and verifying the source path binding.
    pub const fn with_path_binding(mut self, options: LocalPathBindingOptions) -> Self {
        self.path_binding = Some(options);
        self
    }

    pub(crate) const fn read_options(&self) -> Option<&LocalReadOptions> {
        self.read_options.as_ref()
    }

    pub(crate) const fn native_observation(&self) -> LocalObservationRequirement {
        self.native_observation
    }

    pub(crate) const fn path_binding(&self) -> Option<&LocalPathBindingOptions> {
        self.path_binding.as_ref()
    }
}
