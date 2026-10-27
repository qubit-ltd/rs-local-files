// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
use crate::directory::LocalTreeValidationOptions;
use crate::policy::LocalDurabilityRequirement;
use crate::policy::LocalFileSyncRequirement;
use crate::policy::LocalTreeFinalAccess;

/// Optional verification, access, file synchronization, and durability
/// requested before tree publication.
#[derive(Clone, Debug)]
#[must_use]
pub struct LocalTreePublishOptions {
    validation: Option<LocalTreeValidationOptions>,
    durability: LocalDurabilityRequirement,
    file_sync: LocalFileSyncRequirement,
    access: LocalTreeFinalAccess,
}
impl LocalTreePublishOptions {
    /// Creates no-replace options that preserve access and request no
    /// synchronization.
    pub const fn new() -> Self {
        Self {
            validation: None,
            durability: LocalDurabilityRequirement::NotRequired,
            file_sync: LocalFileSyncRequirement::NotRequired,
            access: LocalTreeFinalAccess::Preserve,
        }
    }
    /// Validates the staged tree before any access or publication mutation.
    pub fn with_validation(mut self, validation: LocalTreeValidationOptions) -> Self {
        self.validation = Some(validation);
        self
    }
    /// Sets the requested parent namespace durability.
    pub const fn with_durability(mut self, requirement: LocalDurabilityRequirement) -> Self {
        self.durability = requirement;
        self
    }
    /// Sets the requested synchronization policy for regular files in the
    /// stage.
    pub const fn with_file_sync(mut self, requirement: LocalFileSyncRequirement) -> Self {
        self.file_sync = requirement;
        self
    }
    /// Sets the access policy applied to the stage before publication.
    pub const fn with_access(mut self, access: LocalTreeFinalAccess) -> Self {
        self.access = access;
        self
    }
    pub(crate) const fn validation(&self) -> Option<&LocalTreeValidationOptions> {
        self.validation.as_ref()
    }
    pub(crate) const fn durability(&self) -> LocalDurabilityRequirement {
        self.durability
    }
    pub(crate) const fn file_sync(&self) -> LocalFileSyncRequirement {
        self.file_sync
    }
    pub(crate) const fn access(&self) -> LocalTreeFinalAccess {
        self.access
    }
}
impl Default for LocalTreePublishOptions {
    fn default() -> Self {
        Self::new()
    }
}
