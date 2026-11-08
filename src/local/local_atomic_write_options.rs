// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Atomic write options.

use std::time::Duration;

use super::internal::LocalAtomicPublicationMode;
use crate::LocalDurabilityRequirement;
use crate::options::LocalWriteMetadataPolicy;
use crate::policy::LocalFileSyncRequirement;

/// Options used when beginning a local atomic write.
///
/// Builder results must be used so that an accidentally discarded option does
/// not silently leave the original value unchanged.
///
/// # Examples
///
/// ```ignore
/// use qubit_local_files::local::LocalAtomicWriteOptions;
/// use qubit_local_files::options::LocalWriteMetadataPolicy;
/// use qubit_local_files::policy::LocalDurabilityRequirement;
///
/// let options = LocalAtomicWriteOptions::new()
///     .with_create_parent()
///     .with_metadata_policy(LocalWriteMetadataPolicy::UseStaging)
///     .with_durability(LocalDurabilityRequirement::Preferred);
///
/// assert!(options.creates_parent());
/// assert_eq!(options.metadata_policy(), LocalWriteMetadataPolicy::UseStaging);
/// assert_eq!(options.durability(), LocalDurabilityRequirement::Preferred);
/// ```
#[must_use = "atomic write options have no effect unless they are used"]
#[non_exhaustive]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LocalAtomicWriteOptions {
    /// Whether missing parent directories should be created before staging.
    create_parent: bool,
    /// Metadata applied to replacement staging independently of identity
    /// checks.
    metadata_policy: LocalWriteMetadataPolicy,
    /// Optional limit for retrying a nonblocking destination open.
    open_retry_timeout: Option<Duration>,
    /// Whether a final symlink entry may be replaced without following it.
    replace_target_symlink: bool,
    /// Publication policy enforced during final installation.
    publication_mode: LocalAtomicPublicationMode,
    /// Durability requested for staging and parent synchronization.
    durability: LocalDurabilityRequirement,
    /// Whether staging-file contents and metadata must be synchronized.
    file_sync: LocalFileSyncRequirement,
}

impl LocalAtomicWriteOptions {
    /// Returns atomic write options without parent creation.
    ///
    /// # Returns
    /// Default atomic write options.
    #[inline]
    pub const fn new() -> Self {
        Self {
            create_parent: false,
            metadata_policy: LocalWriteMetadataPolicy::PreserveExisting,
            open_retry_timeout: None,
            replace_target_symlink: false,
            publication_mode: LocalAtomicPublicationMode::ReplaceOrCreate,
            durability: LocalDurabilityRequirement::Required,
            file_sync: LocalFileSyncRequirement::NotRequired,
        }
    }

    /// Returns the metadata policy for replacing an existing regular file.
    ///
    /// # Returns
    /// The configured replacement metadata policy.
    #[inline]
    pub const fn metadata_policy(&self) -> LocalWriteMetadataPolicy {
        self.metadata_policy
    }

    /// Returns whether missing parent directories will be created.
    ///
    /// # Returns
    /// `true` when parent creation is enabled.
    #[inline]
    #[must_use]
    pub const fn creates_parent(&self) -> bool {
        self.create_parent
    }

    /// Returns the configured nonblocking-open retry timeout.
    ///
    /// On Unix, this limits how long commit waits for an existing destination
    /// whose active file lease makes a nonblocking open return
    /// [`std::io::ErrorKind::WouldBlock`]. [`None`] performs only the initial
    /// open attempt.
    ///
    /// # Returns
    /// The configured timeout, or [`None`] when retries are disabled.
    #[inline]
    #[cfg_attr(windows, allow(dead_code))]
    pub const fn open_retry_timeout(&self) -> Option<Duration> {
        self.open_retry_timeout
    }

    /// Returns the requested durability for atomic publication.
    ///
    /// # Returns
    /// The configured file and parent synchronization policy.
    #[must_use = "inspect the requested durability"]
    #[inline]
    pub const fn durability(&self) -> LocalDurabilityRequirement {
        self.durability
    }

    /// Reports whether final symbolic-link replacement is enabled.
    ///
    /// # Returns
    /// `true` when publication replaces the destination symlink entry.
    #[must_use]
    #[inline]
    pub const fn replaces_target_symlink(&self) -> bool {
        self.replace_target_symlink
    }

    /// Returns the final installation policy.
    ///
    /// # Returns
    /// The policy enforced when installing the completed staging file.
    #[must_use]
    #[inline]
    pub const fn publication_mode(&self) -> LocalAtomicPublicationMode {
        self.publication_mode
    }

    /// Selects metadata copied to a replacement staging file.
    ///
    /// This policy does not alter the destination identity checks used during
    /// publication.
    ///
    /// # Parameters
    /// - `policy`: Metadata source for replacing an existing regular file.
    ///
    /// # Returns
    /// Updated options carrying the replacement metadata policy.
    #[inline]
    pub const fn with_metadata_policy(mut self, policy: LocalWriteMetadataPolicy) -> Self {
        self.metadata_policy = policy;
        self
    }

    /// Enables parent directory creation.
    ///
    /// # Returns
    /// Updated options that create missing parent directories before staging.
    #[inline]
    pub const fn with_create_parent(mut self) -> Self {
        self.create_parent = true;
        self
    }

    /// Sets the nonblocking-open retry timeout.
    ///
    /// On Unix, [`Duration::ZERO`] returns
    /// [`std::io::ErrorKind::TimedOut`] after the first lease-conflicting open
    /// attempt. Other open errors are never retried.
    ///
    /// # Parameters
    /// - `timeout`: Maximum time to retry a lease-conflicting open.
    ///
    /// # Returns
    /// Updated options carrying the timeout.
    pub const fn with_open_retry_timeout(mut self, timeout: Duration) -> Self {
        self.open_retry_timeout = Some(timeout);
        self
    }

    /// Sets the required durability for atomic publication.
    ///
    /// # Parameters
    ///
    /// - `durability`: Requested file and parent synchronization policy.
    ///
    /// # Returns
    ///
    /// Updated options carrying the durability policy.
    #[inline]
    pub const fn with_durability(mut self, durability: LocalDurabilityRequirement) -> Self {
        self.durability = durability;
        self
    }

    /// Requires final publication to preserve every existing destination.
    ///
    /// The atomic backend checks for an existing entry while opening and uses
    /// a native no-replace installation at commit, so a concurrent creator
    /// cannot be overwritten.
    ///
    /// # Returns
    ///
    /// Updated options enforcing create-new publication.
    #[inline]
    pub const fn with_create_new(mut self) -> Self {
        self.publication_mode = LocalAtomicPublicationMode::CreateNew;
        self
    }

    /// Returns the requested synchronization of the staging file's contents
    /// and metadata before publication.
    ///
    /// # Returns
    /// The configured synchronization requirement.
    pub(crate) const fn file_sync(&self) -> LocalFileSyncRequirement {
        self.file_sync
    }

    /// Sets whether staging-file contents and metadata must be synchronized
    /// before publication.
    ///
    /// This setting is independent of the durability policy for the parent
    /// directory.
    ///
    /// # Parameters
    /// - `requirement`: File synchronization policy to use during publication.
    ///
    /// # Returns
    /// Updated options carrying the file synchronization policy.
    #[inline]
    pub(crate) const fn with_file_sync(mut self, requirement: LocalFileSyncRequirement) -> Self {
        self.file_sync = requirement;
        self
    }
}

impl Default for LocalAtomicWriteOptions {
    /// Returns the same policy as [`Self::new`].
    ///
    /// # Returns
    /// Default atomic write options.
    fn default() -> Self {
        Self::new()
    }
}
