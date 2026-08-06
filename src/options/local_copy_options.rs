// =============================================================================
//    Copyright (c) 2025 - 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
// qubit-style: allow source-test-pair
// Covered by copy integration tests.

use super::{
    LocalAtomicityRequirement,
    LocalCopySourceMode,
    LocalDurabilityRequirement,
    LocalMetadataPreservePolicy,
    LocalSymlinkPolicy,
};
use crate::{
    LocalCopyConflictPolicy,
    LocalCopyTypeConflictPolicy,
};

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
}

impl LocalCopyOptions {
    /// Creates copy options that inherit the owning filesystem's
    /// symbolic-link policy.
    #[cfg_attr(coverage, inline(never))]
    #[cfg_attr(not(coverage), inline)]
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
        }
    }

    /// Returns the destination file conflict policy.
    #[cfg_attr(coverage, inline(never))]
    #[cfg_attr(not(coverage), inline(always))]
    pub const fn conflict(&self) -> LocalCopyConflictPolicy {
        self.conflict
    }

    /// Returns the file/directory type conflict policy.
    #[cfg_attr(coverage, inline(never))]
    #[cfg_attr(not(coverage), inline(always))]
    pub const fn type_conflict(&self) -> LocalCopyTypeConflictPolicy {
        self.type_conflict
    }

    /// Returns the metadata preservation policy.
    #[cfg_attr(coverage, inline(never))]
    #[cfg_attr(not(coverage), inline(always))]
    pub const fn preserve_metadata(&self) -> LocalMetadataPreservePolicy {
        self.preserve_metadata
    }

    /// Returns the optional symbolic-link policy override.
    #[must_use]
    #[cfg_attr(coverage, inline(never))]
    #[cfg_attr(not(coverage), inline(always))]
    pub const fn symlink_policy_override(&self) -> Option<LocalSymlinkPolicy> {
        self.symlink
    }

    /// Returns the source kind accepted by this copy.
    #[cfg_attr(coverage, inline(never))]
    #[cfg_attr(not(coverage), inline(always))]
    pub const fn source_mode(&self) -> LocalCopySourceMode {
        self.source_mode
    }

    /// Reports whether missing target parent directories are created.
    #[must_use]
    #[cfg_attr(coverage, inline(never))]
    #[cfg_attr(not(coverage), inline(always))]
    pub const fn creates_parent(&self) -> bool {
        self.create_parent
    }

    /// Returns the required atomicity.
    #[cfg_attr(coverage, inline(never))]
    #[cfg_attr(not(coverage), inline(always))]
    pub const fn atomicity(&self) -> LocalAtomicityRequirement {
        self.atomicity
    }

    /// Returns the required durability.
    #[cfg_attr(coverage, inline(never))]
    #[cfg_attr(not(coverage), inline(always))]
    pub const fn durability(&self) -> LocalDurabilityRequirement {
        self.durability
    }

    /// Sets the destination file conflict policy.
    #[cfg_attr(coverage, inline(never))]
    #[cfg_attr(not(coverage), inline(always))]
    pub const fn with_conflict(
        mut self,
        conflict: LocalCopyConflictPolicy,
    ) -> Self {
        self.conflict = conflict;
        self
    }

    /// Sets the file/directory type conflict policy.
    #[cfg_attr(coverage, inline(never))]
    #[cfg_attr(not(coverage), inline(always))]
    pub const fn with_type_conflict(
        mut self,
        type_conflict: LocalCopyTypeConflictPolicy,
    ) -> Self {
        self.type_conflict = type_conflict;
        self
    }

    /// Sets metadata preservation policy.
    #[cfg_attr(coverage, inline(never))]
    #[cfg_attr(not(coverage), inline(always))]
    pub const fn with_metadata_preservation(
        mut self,
        preserve_metadata: LocalMetadataPreservePolicy,
    ) -> Self {
        self.preserve_metadata = preserve_metadata;
        self
    }

    /// Sets symbolic-link policy.
    #[cfg_attr(coverage, inline(never))]
    #[cfg_attr(not(coverage), inline(always))]
    pub const fn with_symlink_policy(
        mut self,
        symlink: LocalSymlinkPolicy,
    ) -> Self {
        self.symlink = Some(symlink);
        self
    }

    /// Requires a regular file source.
    #[cfg_attr(coverage, inline(never))]
    #[cfg_attr(not(coverage), inline(always))]
    pub const fn with_file_source(mut self) -> Self {
        self.source_mode = LocalCopySourceMode::File;
        self
    }

    /// Requires a directory-tree source.
    #[cfg_attr(coverage, inline(never))]
    #[cfg_attr(not(coverage), inline(always))]
    pub const fn with_tree_source(mut self) -> Self {
        self.source_mode = LocalCopySourceMode::Tree;
        self
    }

    /// Creates missing target parent directories before copying.
    #[cfg_attr(coverage, inline(never))]
    #[cfg_attr(not(coverage), inline(always))]
    pub const fn with_parent(mut self) -> Self {
        self.create_parent = true;
        self
    }

    /// Sets required publication atomicity.
    #[cfg_attr(coverage, inline(never))]
    #[cfg_attr(not(coverage), inline(always))]
    pub const fn with_atomicity(
        mut self,
        atomicity: LocalAtomicityRequirement,
    ) -> Self {
        self.atomicity = atomicity;
        self
    }

    /// Sets required durability.
    #[cfg_attr(coverage, inline(never))]
    #[cfg_attr(not(coverage), inline(always))]
    pub const fn with_durability(
        mut self,
        durability: LocalDurabilityRequirement,
    ) -> Self {
        self.durability = durability;
        self
    }
}

impl Default for LocalCopyOptions {
    /// Returns conservative copy defaults.
    #[cfg_attr(coverage, inline(never))]
    #[cfg_attr(not(coverage), inline)]
    fn default() -> Self {
        Self::new()
    }
}
