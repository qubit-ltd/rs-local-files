// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
// Covered by temporary resource integration tests.

use std::path::Path;
use std::path::PathBuf;

use super::LocalTempCleanupLimits;
use crate::policy::LocalOwnerAccessPolicy;
use crate::policy::LocalPrivacyRequirement;

/// Configures a temporary directory's location, naming, privacy, owner access,
/// and retained cleanup limits.
///
/// # Examples
///
/// ```
/// use qubit_local_files::options::LocalTempDirectoryOptions;
///
/// let options = LocalTempDirectoryOptions::new().with_prefix("cache-");
/// assert_eq!(options.prefix(), Some("cache-"));
/// ```
#[derive(Clone, Debug, Eq, PartialEq)]
#[must_use = "temporary directory options have no effect unless they are used"]
pub struct LocalTempDirectoryOptions {
    /// Optional parent directory; the owning filesystem's PWD is the default.
    parent: Option<PathBuf>,
    /// Optional directory-name prefix.
    prefix: Option<String>,
    /// Optional directory-name suffix.
    suffix: Option<String>,
    /// Optional maximum random-name creation attempts.
    max_attempts: Option<usize>,
    /// Whether a missing parent directory is created before allocation.
    create_parent: bool,
    /// Limits retained for explicit cleanup and the final Drop attempt.
    cleanup_limits: LocalTempCleanupLimits,
    /// Requested privacy for the newly created directory.
    privacy: LocalPrivacyRequirement,
    /// Exact access policy for the stage and entries created beneath it.
    owner_access_policy: Option<LocalOwnerAccessPolicy>,
}

impl LocalTempDirectoryOptions {
    /// Creates default temporary-directory options.
    ///
    /// # Returns
    ///
    /// Default options with platform-default privacy, unbounded cleanup, and
    /// no explicit parent, name affixes, or owner access policy.
    #[inline]
    pub const fn new() -> Self {
        Self {
            parent: None,
            prefix: None,
            suffix: None,
            max_attempts: None,
            create_parent: false,
            cleanup_limits: LocalTempCleanupLimits::new(),
            privacy: LocalPrivacyRequirement::PlatformDefault,
            owner_access_policy: None,
        }
    }

    /// Returns the limits that each created directory retains for cleanup.
    ///
    /// # Returns
    ///
    /// The cleanup limits used for explicit cleanup and the final Drop attempt.
    #[must_use = "inspect the limits retained for explicit and automatic cleanup"]
    pub const fn cleanup_limits(&self) -> LocalTempCleanupLimits {
        self.cleanup_limits
    }

    /// Returns the requested creation privacy.
    ///
    /// # Returns
    ///
    /// The privacy requirement requested for the new directory.
    #[must_use]
    #[inline]
    pub const fn privacy(&self) -> LocalPrivacyRequirement {
        self.privacy
    }

    /// Returns the exact owner access policy requested for the stage.
    ///
    /// # Returns
    ///
    /// `Some(policy)` when configured, or `None` when creation uses platform
    /// defaults.
    pub const fn owner_access_policy(&self) -> Option<&LocalOwnerAccessPolicy> {
        self.owner_access_policy.as_ref()
    }

    /// Returns the configured parent, or `None` for the owning filesystem's
    /// PWD.
    ///
    /// # Returns
    ///
    /// `Some(parent)` for the configured path, or `None` to use the owning
    /// filesystem's PWD.
    #[inline]
    pub fn parent(&self) -> Option<&Path> {
        self.parent.as_deref()
    }

    /// Returns the optional directory-name prefix.
    ///
    /// # Returns
    ///
    /// `Some(prefix)` when configured, or `None` when names have no prefix.
    #[inline]
    pub fn prefix(&self) -> Option<&str> {
        self.prefix.as_deref()
    }

    /// Returns the optional directory-name suffix.
    ///
    /// # Returns
    ///
    /// `Some(suffix)` when configured, or `None` when names have no suffix.
    #[inline]
    pub fn suffix(&self) -> Option<&str> {
        self.suffix.as_deref()
    }

    /// Returns the optional maximum number of random-name creation attempts.
    ///
    /// `None` leaves collision retries unbounded. A positive value includes
    /// the first attempt; only name collisions are retried. `Some(0)` is
    /// invalid and is rejected by the consuming creation operation before
    /// side effects.
    ///
    /// # Returns
    ///
    /// `Some(attempts)` for the configured budget, or `None` when collision
    /// retries are unbounded. `Some(0)` is invalid and rejected on consumption.
    #[inline]
    pub const fn max_attempts(&self) -> Option<usize> {
        self.max_attempts
    }

    /// Returns whether missing parent directories are created.
    ///
    /// # Returns
    ///
    /// `true` when missing parent directories are created before allocation.
    #[must_use]
    #[inline]
    pub const fn creates_parent(&self) -> bool {
        self.create_parent
    }

    /// Sets the requested creation privacy.
    ///
    /// # Parameters
    ///
    /// - `requirement`: Privacy requirement to use when creating the directory.
    ///
    /// # Returns
    ///
    /// These options with the requested privacy requirement.
    #[inline]
    pub const fn with_privacy(mut self, requirement: LocalPrivacyRequirement) -> Self {
        self.privacy = requirement;
        self
    }

    /// Sets the exact owner access policy used when creating the stage and
    /// every entry written beneath it.
    ///
    /// # Parameters
    ///
    /// - `policy`: Exact owner access policy for the stage and its entries.
    ///
    /// # Returns
    ///
    /// These options configured with `policy`.
    #[inline]
    pub const fn with_owner_access_policy(mut self, policy: LocalOwnerAccessPolicy) -> Self {
        self.owner_access_policy = Some(policy);
        self
    }

    /// Retains `limits` for explicit cleanup and Drop without performing I/O.
    ///
    /// # Parameters
    ///
    /// - `limits`: Cleanup limits retained by each created directory.
    ///
    /// # Returns
    ///
    /// These options configured with `limits`.
    pub const fn with_cleanup_limits(mut self, limits: LocalTempCleanupLimits) -> Self {
        self.cleanup_limits = limits;
        self
    }

    /// Enables recursive creation of a missing parent directory.
    ///
    /// # Returns
    ///
    /// These options configured to create missing parent directories.
    #[inline]
    pub const fn with_create_parent(mut self) -> Self {
        self.create_parent = true;
        self
    }

    /// Sets the native parent directory.
    ///
    /// # Parameters
    ///
    /// - `parent`: Absolute or relative parent directory.
    ///
    /// # Returns
    ///
    /// These options configured to create the directory beneath `parent`.
    pub fn with_parent(mut self, parent: &Path) -> Self {
        self.parent = Some(parent.to_path_buf());
        self
    }

    /// Sets the portable directory-name prefix.
    ///
    /// # Parameters
    ///
    /// - `prefix`: Prefix validated before entry creation.
    ///
    /// # Returns
    ///
    /// These options configured with `prefix`.
    pub fn with_prefix(mut self, prefix: &str) -> Self {
        self.prefix = Some(prefix.to_owned());
        self
    }

    /// Sets the portable directory-name suffix.
    ///
    /// # Parameters
    ///
    /// - `suffix`: Suffix validated before entry creation.
    ///
    /// # Returns
    ///
    /// These options configured with `suffix`.
    pub fn with_suffix(mut self, suffix: &str) -> Self {
        self.suffix = Some(suffix.to_owned());
        self
    }

    /// Sets the maximum number of random-name creation attempts.
    ///
    /// # Parameters
    ///
    /// - `max_attempts`: Positive attempt count, including the first attempt.
    ///   Only name collisions are retried. Zero is invalid and is rejected when
    ///   the options are consumed by a creation operation, before side effects.
    ///
    /// # Returns
    ///
    /// These options configured with the requested attempt budget.
    #[inline]
    pub const fn with_max_attempts(mut self, max_attempts: usize) -> Self {
        self.max_attempts = Some(max_attempts);
        self
    }

    /// Removes the random-name attempt budget, leaving name-collision retries
    /// unbounded.
    ///
    /// # Returns
    ///
    /// These options configured without an attempt limit.
    #[inline]
    pub const fn without_max_attempts(mut self) -> Self {
        self.max_attempts = None;
        self
    }
}

impl Default for LocalTempDirectoryOptions {
    /// Returns default temporary-directory options.
    ///
    /// # Returns
    ///
    /// Default options as returned by [`LocalTempDirectoryOptions::new`].
    fn default() -> Self {
        Self::new()
    }
}
