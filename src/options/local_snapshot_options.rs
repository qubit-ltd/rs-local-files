// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
use super::LocalReadOptions;
use super::LocalSnapshotOptionsBuilder;
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
    /// Maximum content length accepted by a consistent read.
    pub(super) max_bytes: u64,
    /// Reader-opening options; these replace rather than inherit filesystem
    /// defaults.
    pub(super) read_options: LocalReadOptions,
    /// Optional symbolic-link policy override for the source path.
    pub(super) symlink_policy: Option<LocalSymlinkPolicy>,
}

impl LocalSnapshotOptions {
    /// Starts building snapshot options with an explicit byte limit.
    ///
    /// Read options start at [`LocalReadOptions::new`]; filesystem defaults
    /// are not inherited unless passed to
    /// [`LocalSnapshotOptionsBuilder::read_options`].
    ///
    /// # Parameters
    ///
    /// - `max_bytes`: Maximum number of content bytes accepted before the read
    ///   reports a resource-limit error.
    ///
    /// # Returns
    ///
    /// A builder initialized with the byte limit, default reader options, and
    /// no symbolic-link policy override.
    #[inline]
    pub const fn builder(max_bytes: u64) -> LocalSnapshotOptionsBuilder {
        LocalSnapshotOptionsBuilder {
            max_bytes,
            read_options: LocalReadOptions::new(),
            symlink_policy: None,
        }
    }

    /// Returns the maximum content length accepted by the snapshot read.
    ///
    /// # Returns
    ///
    /// The configured content byte limit.
    #[must_use]
    #[inline]
    pub(crate) const fn max_bytes(&self) -> u64 {
        self.max_bytes
    }

    /// Borrows the reader-opening options used for the source.
    ///
    /// These options are the complete configuration and do not merge with
    /// filesystem defaults.
    ///
    /// # Returns
    ///
    /// A borrow of the reader-opening options.
    #[inline]
    pub(crate) const fn read_options(&self) -> &LocalReadOptions {
        &self.read_options
    }

    /// Returns the source symbolic-link policy override, if one was set.
    ///
    /// `None` leaves policy selection to the surrounding filesystem operation.
    ///
    /// # Returns
    ///
    /// `Some(policy)` when an override is configured, or `None` when the
    /// surrounding filesystem operation selects the policy.
    #[inline]
    pub(crate) const fn symlink_policy(&self) -> Option<LocalSymlinkPolicy> {
        self.symlink_policy
    }
}
