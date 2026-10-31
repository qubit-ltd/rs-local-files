// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
use super::LocalReadOptions;
use crate::policy::LocalSymlinkPolicy;

/// Options for bounded complete-file reads.
///
/// By default, the read options and symlink policy come from the filesystem,
/// and no additional byte limit is applied. An explicit read-options value
/// replaces the filesystem's complete reader configuration; a symlink policy
/// set here takes precedence over that value's policy and the filesystem
/// default. The byte limit applies to the complete file, and exceeding it
/// returns [`LocalFileErrorKind::ResourceLimit`](crate::error::LocalFileErrorKind).
///
/// # Examples
///
/// ```
/// use qubit_local_files::LocalFileSystem;
/// use qubit_local_files::options::LocalReadAllOptions;
/// use qubit_local_files::error::LocalFileErrorKind;
///
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let scratch = tempfile::tempdir()?;
/// let path = scratch.path().join("payload");
/// std::fs::write(&path, b"data")?;
/// let filesystem = LocalFileSystem::host()?;
/// let options = LocalReadAllOptions::new().with_max_bytes(4);
/// assert_eq!(filesystem.read_all_with_options(&path, &options)?, b"data");
/// let failure = filesystem.read_all_with_options(
///     &path,
///     &LocalReadAllOptions::new().with_max_bytes(3),
/// ).expect_err("the complete file exceeds the byte limit");
/// assert_eq!(failure.kind(), LocalFileErrorKind::ResourceLimit);
/// # Ok(())
/// # }
/// ```
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
#[must_use = "read-all options have no effect unless they are used"]
pub struct LocalReadAllOptions {
    /// Optional complete-file byte limit; `None` imposes no additional limit.
    max_bytes: Option<u64>,
    /// Optional complete replacement for the filesystem's reader defaults.
    read_options: Option<LocalReadOptions>,
    /// Optional symlink override, applied after the selected reader options.
    symlink_policy: Option<LocalSymlinkPolicy>,
}

impl LocalReadAllOptions {
    /// Creates options that inherit filesystem reader defaults and have no
    /// additional byte limit.
    pub const fn new() -> Self {
        Self {
            max_bytes: None,
            read_options: None,
            symlink_policy: None,
        }
    }

    /// Sets the maximum number of bytes returned by a complete read.
    pub const fn with_max_bytes(mut self, maximum: u64) -> Self {
        self.max_bytes = Some(maximum);
        self
    }

    /// Replaces the filesystem's complete reader options.
    pub const fn with_read_options(mut self, options: LocalReadOptions) -> Self {
        self.read_options = Some(options);
        self
    }

    /// Overrides the symbolic-link policy after selecting reader options.
    pub const fn with_symlink_policy(mut self, policy: LocalSymlinkPolicy) -> Self {
        self.symlink_policy = Some(policy);
        self
    }

    /// Returns the additional complete-file byte limit, if configured.
    pub(crate) const fn max_bytes(&self) -> Option<u64> {
        self.max_bytes
    }

    /// Returns the explicit reader configuration; `None` means inherit defaults.
    pub(crate) const fn read_options(&self) -> Option<&LocalReadOptions> {
        self.read_options.as_ref()
    }

    /// Returns the policy override, if present, which takes precedence over
    /// the selected reader options and filesystem default.
    pub(crate) const fn symlink_policy(&self) -> Option<LocalSymlinkPolicy> {
        self.symlink_policy
    }
}
