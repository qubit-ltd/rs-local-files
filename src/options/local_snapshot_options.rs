// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
use super::LocalReadOptions;
use crate::policy::LocalSymlinkPolicy;

/// Options for reading bytes bracketed by same-handle observations.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[must_use = "snapshot options have no effect unless they are used"]
pub struct LocalSnapshotOptions {
    max_bytes: u64,
    read_options: LocalReadOptions,
    symlink_policy: Option<LocalSymlinkPolicy>,
}

impl LocalSnapshotOptions {
    /// Starts building snapshot options with an explicit byte limit.
    pub const fn builder(max_bytes: u64) -> LocalSnapshotOptionsBuilder {
        LocalSnapshotOptionsBuilder {
            max_bytes,
            read_options: LocalReadOptions::new(),
            symlink_policy: None,
        }
    }

    pub(crate) const fn max_bytes(&self) -> u64 {
        self.max_bytes
    }

    pub(crate) const fn read_options(&self) -> &LocalReadOptions {
        &self.read_options
    }

    pub(crate) const fn symlink_policy(&self) -> Option<LocalSymlinkPolicy> {
        self.symlink_policy
    }
}

/// Builder for a bounded consistent-read snapshot.
#[derive(Clone, Copy, Debug)]
#[must_use = "snapshot builder options have no effect unless build is called"]
pub struct LocalSnapshotOptionsBuilder {
    max_bytes: u64,
    read_options: LocalReadOptions,
    symlink_policy: Option<LocalSymlinkPolicy>,
}

impl LocalSnapshotOptionsBuilder {
    /// Replaces reader options used to open the source.
    pub const fn read_options(mut self, options: LocalReadOptions) -> Self {
        self.read_options = options;
        self
    }

    /// Overrides the source symbolic-link policy.
    pub const fn symlink_policy(mut self, policy: LocalSymlinkPolicy) -> Self {
        self.symlink_policy = Some(policy);
        self
    }

    /// Completes snapshot configuration.
    pub const fn build(self) -> LocalSnapshotOptions {
        LocalSnapshotOptions {
            max_bytes: self.max_bytes,
            read_options: self.read_options,
            symlink_policy: self.symlink_policy,
        }
    }
}
