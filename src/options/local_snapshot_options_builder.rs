// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
use super::LocalReadOptions;
use super::LocalSnapshotOptions;
use crate::policy::LocalSymlinkPolicy;

/// Builder for a bounded consistent-read snapshot.
///
/// # Examples
///
/// ```
/// use qubit_local_files::options::LocalSnapshotOptions;
/// use qubit_local_files::options::LocalSnapshotOptionsBuilder;
///
/// let builder: LocalSnapshotOptionsBuilder = LocalSnapshotOptions::builder(4096);
/// let options = builder.build();
/// assert_eq!(options, LocalSnapshotOptions::builder(4096).build());
/// ```
#[derive(Clone, Copy, Debug)]
#[must_use = "snapshot builder options have no effect unless build is called"]
pub struct LocalSnapshotOptionsBuilder {
    /// Maximum content length accepted by the completed snapshot options.
    pub(super) max_bytes: u64,
    /// Complete reader-opening configuration for the source.
    pub(super) read_options: LocalReadOptions,
    /// Optional source symbolic-link policy override.
    pub(super) symlink_policy: Option<LocalSymlinkPolicy>,
}

impl LocalSnapshotOptionsBuilder {
    /// Replaces reader options used to open the source.
    ///
    /// This is a complete replacement, not a merge with filesystem defaults.
    ///
    /// # Parameters
    ///
    /// - `options`: Reader-opening options to use for this snapshot read.
    ///
    /// # Returns
    ///
    /// The builder with the supplied reader options.
    #[inline]
    pub const fn read_options(mut self, options: LocalReadOptions) -> Self {
        self.read_options = options;
        self
    }

    /// Overrides the source symbolic-link policy.
    ///
    /// # Parameters
    ///
    /// - `policy`: Symbolic-link policy to apply when opening the source.
    ///
    /// # Returns
    ///
    /// The builder with the supplied policy override.
    #[inline]
    pub const fn symlink_policy(mut self, policy: LocalSymlinkPolicy) -> Self {
        self.symlink_policy = Some(policy);
        self
    }

    /// Completes snapshot configuration. Reads exceeding the configured content
    /// limit fail with a resource-limit error. At the limit, one additional
    /// byte may be read to distinguish EOF from excess input. Before/after
    /// observations do not provide atomic content.
    ///
    /// # Returns
    ///
    /// The completed snapshot options, ready to pass to a consistent-read
    /// operation.
    #[inline]
    pub const fn build(self) -> LocalSnapshotOptions {
        LocalSnapshotOptions {
            max_bytes: self.max_bytes,
            read_options: self.read_options,
            symlink_policy: self.symlink_policy,
        }
    }
}
