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
    ///
    /// # Returns
    ///
    /// Writer options initialized with defaults for the selected mode.
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
    ///
    /// # Returns
    ///
    /// The mode used to publish the destination.
    #[inline]
    pub const fn mode(&self) -> LocalWriteMode {
        self.mode
    }

    /// Returns the metadata policy used for staged replacement.
    ///
    /// # Returns
    ///
    /// The policy applied when replacing an existing destination.
    #[inline]
    pub const fn metadata_policy(&self) -> LocalWriteMetadataPolicy {
        self.metadata_policy
    }

    /// Selects replacement metadata without changing publication guarantees.
    ///
    /// `UseStaging` can change access controls. Create-new and append modes do
    /// not copy old metadata under either policy.
    ///
    /// # Parameters
    ///
    /// - `policy`: Metadata source to use when replacing an existing
    ///   destination.
    ///
    /// # Returns
    ///
    /// The updated options.
    #[inline]
    pub const fn with_metadata_policy(mut self, policy: LocalWriteMetadataPolicy) -> Self {
        self.metadata_policy = policy;
        self
    }

    /// Reports whether missing parent directories are created.
    ///
    /// # Returns
    ///
    /// `true` if missing parent directories will be created.
    #[must_use]
    #[inline]
    pub const fn creates_parent(&self) -> bool {
        self.create_parent
    }

    /// Returns the required atomicity.
    ///
    /// # Returns
    ///
    /// The atomicity guarantee requested for publication.
    #[inline]
    pub const fn atomicity(&self) -> LocalAtomicityRequirement {
        self.atomicity
    }

    /// Returns the required durability.
    ///
    /// # Returns
    ///
    /// The durability guarantee requested for the operation.
    #[inline]
    pub const fn durability(&self) -> LocalDurabilityRequirement {
        self.durability
    }

    /// Returns the requested synchronization of the file itself.
    ///
    /// # Returns
    ///
    /// The requested file synchronization guarantee.
    #[inline]
    pub const fn file_sync(&self) -> LocalFileSyncRequirement {
        self.file_sync
    }

    /// Returns the configured Unix open retry timeout.
    ///
    /// # Returns
    ///
    /// `Some(duration)` when library-level retries are bounded by a timeout;
    /// `None` when no retry timeout is configured.
    #[inline]
    pub const fn open_retry_timeout(&self) -> Option<Duration> {
        self.open_retry_timeout
    }

    /// Enables creation of missing parent directories.
    ///
    /// # Returns
    ///
    /// The updated options with parent directory creation enabled.
    #[inline]
    pub const fn with_create_parent(mut self) -> Self {
        self.create_parent = true;
        self
    }

    /// Sets the required atomicity.
    ///
    /// # Parameters
    ///
    /// - `atomicity`: Atomicity guarantee requested for publication.
    ///
    /// # Returns
    ///
    /// The updated options.
    #[inline]
    pub const fn with_atomicity(mut self, atomicity: LocalAtomicityRequirement) -> Self {
        self.atomicity = atomicity;
        self
    }

    /// Sets the required durability.
    ///
    /// # Parameters
    ///
    /// - `durability`: Durability guarantee requested for the operation.
    ///
    /// # Returns
    ///
    /// The updated options.
    #[inline]
    pub const fn with_durability(mut self, durability: LocalDurabilityRequirement) -> Self {
        self.durability = durability;
        self
    }

    /// Sets the requested synchronization of the file itself.
    ///
    /// # Parameters
    ///
    /// - `requirement`: File synchronization guarantee requested before
    ///   publication.
    ///
    /// # Returns
    ///
    /// The updated options.
    #[inline]
    pub const fn with_file_sync(mut self, requirement: LocalFileSyncRequirement) -> Self {
        self.file_sync = requirement;
        self
    }

    /// Sets the maximum time spent retrying Unix lease conflicts.
    ///
    /// # Parameters
    ///
    /// - `timeout`: Maximum duration to spend retrying lease conflicts.
    ///
    /// # Returns
    ///
    /// The updated options with the specified retry timeout.
    #[inline]
    pub const fn with_open_retry_timeout(mut self, timeout: Duration) -> Self {
        self.open_retry_timeout = Some(timeout);
        self
    }

    /// Disables library-level open retries.
    ///
    /// # Returns
    ///
    /// The updated options with library-level retries disabled.
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
