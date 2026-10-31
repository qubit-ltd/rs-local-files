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
///
/// The builder starts with [`LocalReadOptions::new`], not the filesystem's
/// configured defaults. Copy defaults explicitly when they are desired.
/// The before/after observations detect changes to the opened object; they do
/// not make the bytes an atomic snapshot against concurrent writes.
///
/// ```
/// use qubit_local_files::LocalFileSystem;
/// use qubit_local_files::options::LocalSnapshotOptions;
///
/// # fn main() -> Result<(), Box<dyn std::error::Error>> {
/// let fixture = tempfile::tempdir()?;
/// let source = fixture.path().join("input.bin");
/// std::fs::write(&source, b"snapshot")?;
/// let filesystem = LocalFileSystem::host()?;
/// let options = LocalSnapshotOptions::builder(4096)
///     .read_options(*filesystem.default_read_options())
///     .build();
/// let snapshot = filesystem.read_consistent_with_options(&source, &options)?;
/// assert_eq!(snapshot.bytes(), b"snapshot");
/// # Ok(())
/// # }
/// ```
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[must_use = "snapshot options have no effect unless they are used"]
pub struct LocalSnapshotOptions {
    max_bytes: u64,
    read_options: LocalReadOptions,
    symlink_policy: Option<LocalSymlinkPolicy>,
}

impl LocalSnapshotOptions {
    /// Starts building snapshot options with an explicit byte limit.
    ///
    /// Read options start at [`LocalReadOptions::new`]; filesystem defaults
    /// are not inherited unless passed to
    /// [`LocalSnapshotOptionsBuilder::read_options`].
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
    ///
    /// This is a complete replacement, not a merge with filesystem defaults.
    pub const fn read_options(mut self, options: LocalReadOptions) -> Self {
        self.read_options = options;
        self
    }

    /// Overrides the source symbolic-link policy.
    pub const fn symlink_policy(mut self, policy: LocalSymlinkPolicy) -> Self {
        self.symlink_policy = Some(policy);
        self
    }

    /// Completes snapshot configuration. Reads exceeding the configured content
    /// limit fail with a resource-limit error. At the limit, one additional
    /// byte may be read to distinguish EOF from excess input. Before/after
    /// observations do not provide atomic content.
    pub const fn build(self) -> LocalSnapshotOptions {
        LocalSnapshotOptions {
            max_bytes: self.max_bytes,
            read_options: self.read_options,
            symlink_policy: self.symlink_policy,
        }
    }
}
