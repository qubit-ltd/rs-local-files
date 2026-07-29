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

/// Options used when beginning a local atomic write.
///
/// Builder results must be used so that an accidentally discarded option does
/// not silently leave the original value unchanged.
#[must_use = "atomic write options have no effect unless they are used"]
#[non_exhaustive]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LocalAtomicWriteOptions {
    /// Whether missing parent directories should be created before staging.
    create_parent: bool,
    /// Optional limit for retrying a nonblocking destination open.
    open_retry_timeout: Option<Duration>,
    /// Whether a final symlink entry may be replaced without following it.
    replace_target_symlink: bool,
    /// Publication policy enforced during final installation.
    publication_mode: LocalAtomicPublicationMode,
    /// Durability requested for staging and parent synchronization.
    durability: LocalDurabilityRequirement,
}

impl LocalAtomicWriteOptions {
    /// Returns atomic write options without parent creation.
    ///
    /// # Returns
    /// Default atomic write options.
    #[inline(always)]
    pub const fn new() -> Self {
        Self {
            create_parent: false,
            open_retry_timeout: None,
            replace_target_symlink: false,
            publication_mode: LocalAtomicPublicationMode::ReplaceOrCreate,
            durability: LocalDurabilityRequirement::Required,
        }
    }

    /// Returns whether missing parent directories will be created.
    ///
    /// # Returns
    /// `true` when parent creation is enabled.
    #[inline(always)]
    #[must_use]
    pub const fn creates_parent(&self) -> bool {
        self.create_parent
    }

    /// Enables parent directory creation.
    ///
    /// # Returns
    /// Updated options that create missing parent directories before staging.
    #[inline(always)]
    pub const fn with_parent(mut self) -> Self {
        self.create_parent = true;
        self
    }

    /// Returns the configured nonblocking-open retry timeout.
    ///
    /// On Unix, this limits how long commit waits for an existing destination
    /// whose active file lease makes a nonblocking open return
    /// [`std::io::ErrorKind::WouldBlock`]. [`None`] preserves the default
    /// unbounded wait.
    ///
    /// # Returns
    /// The configured timeout, or [`None`] when retries are unbounded.
    #[must_use]
    #[inline(always)]
    pub const fn open_retry_timeout(&self) -> Option<Duration> {
        self.open_retry_timeout
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
    #[inline(always)]
    pub const fn with_open_retry_timeout(mut self, timeout: Duration) -> Self {
        self.open_retry_timeout = Some(timeout);
        self
    }

    /// Returns the requested durability for atomic publication.
    #[must_use]
    #[inline(always)]
    pub const fn durability(&self) -> LocalDurabilityRequirement {
        self.durability
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
    #[inline(always)]
    pub const fn with_durability(
        mut self,
        durability: LocalDurabilityRequirement,
    ) -> Self {
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
    #[inline(always)]
    pub const fn with_create_new(mut self) -> Self {
        self.publication_mode = LocalAtomicPublicationMode::CreateNew;
        self
    }

    /// Enables replacement of a final symbolic-link entry.
    #[inline(always)]
    pub(crate) const fn with_target_symlink_replacement(mut self) -> Self {
        self.replace_target_symlink = true;
        self
    }

    /// Reports whether final symbolic-link replacement is enabled.
    #[must_use]
    #[inline(always)]
    pub(crate) const fn replaces_target_symlink(&self) -> bool {
        self.replace_target_symlink
    }

    /// Returns the final installation policy.
    #[must_use]
    #[inline(always)]
    pub(crate) const fn publication_mode(&self) -> LocalAtomicPublicationMode {
        self.publication_mode
    }
}

impl Default for LocalAtomicWriteOptions {
    /// Returns the same policy as [`Self::new`].
    #[inline(always)]
    fn default() -> Self {
        Self::new()
    }
}
