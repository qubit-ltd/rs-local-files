// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
// qubit-style: allow source-test-pair
// Covered by copy integration tests.

use std::time::Duration;

use super::LocalAtomicityRequirement;
use super::LocalCopySourceMode;
use super::LocalDurabilityRequirement;
use super::LocalMetadataPreservePolicy;
use super::LocalSymlinkPolicy;
use crate::LocalCopyConflictPolicy;
use crate::LocalCopyTypeConflictPolicy;

/// Unified options for copying a native file or directory tree.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[must_use = "copy options have no effect unless they are used"]
pub struct LocalCopyOptions {
    /// Destination file conflict policy.
    conflict: LocalCopyConflictPolicy,
    /// File/directory type conflict policy.
    type_conflict: LocalCopyTypeConflictPolicy,
    /// Source metadata preservation policy.
    preserve_metadata: LocalMetadataPreservePolicy,
    /// Optional symbolic-link policy overriding the owning filesystem.
    symlink: Option<LocalSymlinkPolicy>,
    /// Whether copying a directory tree is authorized.
    source_mode: LocalCopySourceMode,
    /// Whether missing target parent directories are created.
    create_parent: bool,
    /// Required publication atomicity.
    atomicity: LocalAtomicityRequirement,
    /// Required durability.
    durability: LocalDurabilityRequirement,
    /// Optional maximum descendant depth for tree copies.
    max_depth: Option<usize>,
    /// Optional maximum number of source entries processed.
    max_entries: Option<usize>,
    /// Optional maximum source bytes copied.
    max_bytes: Option<u64>,
    /// Optional maximum concurrently open source directories.
    max_open_directories: Option<usize>,
    /// Optional wall-clock deadline for the complete copy.
    deadline: Option<Duration>,
}

impl LocalCopyOptions {
    /// Creates copy options that inherit the owning filesystem's
    /// symbolic-link policy.
    pub const fn new() -> Self {
        Self {
            conflict: LocalCopyConflictPolicy::Fail,
            type_conflict: LocalCopyTypeConflictPolicy::Fail,
            preserve_metadata: LocalMetadataPreservePolicy::None,
            symlink: None,
            source_mode: LocalCopySourceMode::Auto,
            create_parent: false,
            atomicity: LocalAtomicityRequirement::Preferred,
            durability: LocalDurabilityRequirement::NotRequired,
            max_depth: None,
            max_entries: None,
            max_bytes: None,
            max_open_directories: None,
            deadline: None,
        }
    }

    /// Returns the destination file conflict policy.
    pub const fn conflict(&self) -> LocalCopyConflictPolicy {
        self.conflict
    }

    /// Returns the file/directory type conflict policy.
    pub const fn type_conflict(&self) -> LocalCopyTypeConflictPolicy {
        self.type_conflict
    }

    /// Returns the metadata preservation policy.
    pub const fn preserve_metadata(&self) -> LocalMetadataPreservePolicy {
        self.preserve_metadata
    }

    /// Returns the optional symbolic-link policy override.
    #[must_use]
    pub const fn symlink_policy_override(&self) -> Option<LocalSymlinkPolicy> {
        self.symlink
    }

    /// Returns the source kind accepted by this copy.
    pub const fn source_mode(&self) -> LocalCopySourceMode {
        self.source_mode
    }

    /// Reports whether missing target parent directories are created.
    #[must_use]
    pub const fn creates_parent(&self) -> bool {
        self.create_parent
    }

    /// Returns the required atomicity.
    pub const fn atomicity(&self) -> LocalAtomicityRequirement {
        self.atomicity
    }

    /// Returns the required durability.
    pub const fn durability(&self) -> LocalDurabilityRequirement {
        self.durability
    }

    /// Returns the optional maximum tree depth.
    pub const fn max_depth(&self) -> Option<usize> {
        self.max_depth
    }
    /// Returns the optional maximum source-entry count.
    pub const fn max_entries(&self) -> Option<usize> {
        self.max_entries
    }
    /// Returns the optional maximum source-byte count.
    pub const fn max_bytes(&self) -> Option<u64> {
        self.max_bytes
    }
    /// Returns the optional maximum open-directory count.
    pub const fn max_open_directories(&self) -> Option<usize> {
        self.max_open_directories
    }
    /// Returns the optional wall-clock deadline.
    pub const fn deadline(&self) -> Option<Duration> {
        self.deadline
    }

    /// Sets the destination file conflict policy.
    pub const fn with_conflict(
        mut self,
        conflict: LocalCopyConflictPolicy,
    ) -> Self {
        self.conflict = conflict;
        self
    }

    /// Sets the file/directory type conflict policy.
    pub const fn with_type_conflict(
        mut self,
        type_conflict: LocalCopyTypeConflictPolicy,
    ) -> Self {
        self.type_conflict = type_conflict;
        self
    }

    /// Sets metadata preservation policy.
    pub const fn with_metadata_preservation(
        mut self,
        preserve_metadata: LocalMetadataPreservePolicy,
    ) -> Self {
        self.preserve_metadata = preserve_metadata;
        self
    }

    /// Sets symbolic-link policy.
    pub const fn with_symlink_policy(
        mut self,
        symlink: LocalSymlinkPolicy,
    ) -> Self {
        self.symlink = Some(symlink);
        self
    }

    /// Requires a regular file source.
    pub const fn with_file_source(mut self) -> Self {
        self.source_mode = LocalCopySourceMode::File;
        self
    }

    /// Requires a directory-tree source.
    pub const fn with_tree_source(mut self) -> Self {
        self.source_mode = LocalCopySourceMode::Tree;
        self
    }

    /// Creates missing target parent directories before copying.
    pub const fn with_parent(mut self) -> Self {
        self.create_parent = true;
        self
    }

    /// Sets required publication atomicity.
    pub const fn with_atomicity(
        mut self,
        atomicity: LocalAtomicityRequirement,
    ) -> Self {
        self.atomicity = atomicity;
        self
    }

    /// Sets required durability.
    pub const fn with_durability(
        mut self,
        durability: LocalDurabilityRequirement,
    ) -> Self {
        self.durability = durability;
        self
    }

    /// Limits recursive tree depth.
    pub const fn with_max_depth(mut self, max_depth: usize) -> Self {
        self.max_depth = Some(max_depth);
        self
    }
    /// Limits the number of source entries processed.
    pub const fn with_max_entries(mut self, max_entries: usize) -> Self {
        self.max_entries = Some(max_entries);
        self
    }
    /// Limits source bytes copied.
    pub const fn with_max_bytes(mut self, max_bytes: u64) -> Self {
        self.max_bytes = Some(max_bytes);
        self
    }
    /// Limits concurrently open source directories.
    pub const fn with_max_open_directories(
        mut self,
        max_open_directories: usize,
    ) -> Self {
        self.max_open_directories = Some(max_open_directories);
        self
    }
    /// Sets a wall-clock deadline for the complete copy.
    pub const fn with_deadline(mut self, deadline: Duration) -> Self {
        self.deadline = Some(deadline);
        self
    }
}

impl Default for LocalCopyOptions {
    /// Returns conservative copy defaults.
    fn default() -> Self {
        Self::new()
    }
}
