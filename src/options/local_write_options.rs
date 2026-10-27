// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
// Covered by writer integration tests.

use std::time::Duration;

use super::LocalWriteMetadataPolicy;
use super::LocalWriteMode;
use crate::policy::LocalAtomicityRequirement;
use crate::policy::LocalDurabilityRequirement;
use crate::policy::LocalFileSyncRequirement;

/// Options fixed for a local file writer session.
///
/// # Examples
///
/// ```
/// use qubit_local_files::options::LocalWriteMode;
/// use qubit_local_files::options::LocalWriteOptions;
/// use qubit_local_files::policy::LocalAtomicityRequirement;
///
/// let options = LocalWriteOptions::new(LocalWriteMode::CreateOrReplace)
///     .with_create_parent()
///     .with_atomicity(LocalAtomicityRequirement::Required);
/// assert!(options.creates_parent());
/// assert_eq!(options.atomicity(), LocalAtomicityRequirement::Required);
/// ```
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[must_use = "write options have no effect unless they are used"]
pub struct LocalWriteOptions {
    /// Destination publication mode.
    mode: LocalWriteMode,
    /// Metadata protocol used when replacing an existing destination.
    metadata_policy: LocalWriteMetadataPolicy,
    /// Whether missing parent directories should be created.
    create_parent: bool,
    /// Required atomicity.
    atomicity: LocalAtomicityRequirement,
    /// Required durability.
    durability: LocalDurabilityRequirement,
    /// Explicit synchronization requirement for the file itself.
    file_sync: LocalFileSyncRequirement,
    /// Optional maximum time spent retrying Unix lease conflicts.
    open_retry_timeout: Option<Duration>,
}

impl LocalWriteOptions {
    /// Creates writer options for the specified publication mode.
    ///
    /// # Parameters
    ///
    /// - `mode`: Destination publication mode.
    pub const fn new(mode: LocalWriteMode) -> Self {
        Self {
            mode,
            metadata_policy: LocalWriteMetadataPolicy::PreserveExisting,
            create_parent: false,
            atomicity: LocalAtomicityRequirement::Preferred,
            durability: LocalDurabilityRequirement::NotRequired,
            file_sync: LocalFileSyncRequirement::NotRequired,
            open_retry_timeout: None,
        }
    }

    /// Returns the publication mode.
    #[inline]
    pub const fn mode(&self) -> LocalWriteMode {
        self.mode
    }

    /// Returns the metadata policy used for staged replacement.
    #[inline]
    pub const fn metadata_policy(&self) -> LocalWriteMetadataPolicy {
        self.metadata_policy
    }

    /// Selects replacement metadata without changing publication guarantees.
    ///
    /// `UseStaging` can change access controls. Create-new and append modes do
    /// not copy old metadata under either policy.
    #[inline]
    pub const fn with_metadata_policy(mut self, policy: LocalWriteMetadataPolicy) -> Self {
        self.metadata_policy = policy;
        self
    }

    /// Reports whether missing parent directories are created.
    #[must_use]
    #[inline]
    pub const fn creates_parent(&self) -> bool {
        self.create_parent
    }

    /// Returns the required atomicity.
    #[inline]
    pub const fn atomicity(&self) -> LocalAtomicityRequirement {
        self.atomicity
    }

    /// Returns the required durability.
    #[inline]
    pub const fn durability(&self) -> LocalDurabilityRequirement {
        self.durability
    }

    /// Returns the requested synchronization of the file itself.
    #[inline]
    pub const fn file_sync(&self) -> LocalFileSyncRequirement {
        self.file_sync
    }

    /// Returns the configured Unix open retry timeout.
    #[inline]
    pub const fn open_retry_timeout(&self) -> Option<Duration> {
        self.open_retry_timeout
    }

    /// Enables creation of missing parent directories.
    #[inline]
    pub const fn with_create_parent(mut self) -> Self {
        self.create_parent = true;
        self
    }

    /// Sets the required atomicity.
    #[inline]
    pub const fn with_atomicity(mut self, atomicity: LocalAtomicityRequirement) -> Self {
        self.atomicity = atomicity;
        self
    }

    /// Sets the required durability.
    #[inline]
    pub const fn with_durability(mut self, durability: LocalDurabilityRequirement) -> Self {
        self.durability = durability;
        self
    }

    /// Sets the requested synchronization of the file itself.
    #[inline]
    pub const fn with_file_sync(mut self, requirement: LocalFileSyncRequirement) -> Self {
        self.file_sync = requirement;
        self
    }

    /// Sets the maximum time spent retrying Unix lease conflicts.
    #[inline]
    pub const fn with_open_retry_timeout(mut self, timeout: Duration) -> Self {
        self.open_retry_timeout = Some(timeout);
        self
    }

    /// Disables library-level open retries.
    #[inline]
    pub const fn without_open_retry_timeout(mut self) -> Self {
        self.open_retry_timeout = None;
        self
    }
}

impl Default for LocalWriteOptions {
    /// Returns create-new options without hidden retries or resource limits.
    fn default() -> Self {
        Self::new(LocalWriteMode::CreateNew)
    }
}
