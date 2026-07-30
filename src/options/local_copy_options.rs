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
    /// Symbolic-link traversal policy.
    symlink: LocalSymlinkPolicy,
    /// Whether copying a directory tree is authorized.
    recursive: bool,
    /// Whether missing target parent directories are created.
    create_parent: bool,
    /// Required publication atomicity.
    atomicity: LocalAtomicityRequirement,
    /// Required durability.
    durability: LocalDurabilityRequirement,
}

impl LocalCopyOptions {
    /// Creates conservative copy options.
    #[inline]
    pub const fn new() -> Self {
        Self {
            conflict: LocalCopyConflictPolicy::Fail,
            type_conflict: LocalCopyTypeConflictPolicy::Fail,
            preserve_metadata: LocalMetadataPreservePolicy::None,
            symlink: LocalSymlinkPolicy::Reject,
            recursive: false,
            create_parent: false,
            atomicity: LocalAtomicityRequirement::Preferred,
            durability: LocalDurabilityRequirement::NotRequired,
        }
    }

    /// Returns the destination file conflict policy.
    #[inline(always)]
    pub const fn conflict(&self) -> LocalCopyConflictPolicy {
        self.conflict
    }

    /// Returns the file/directory type conflict policy.
    #[inline(always)]
    pub const fn type_conflict(&self) -> LocalCopyTypeConflictPolicy {
        self.type_conflict
    }

    /// Returns the metadata preservation policy.
    #[inline(always)]
    pub const fn preserve_metadata(&self) -> LocalMetadataPreservePolicy {
        self.preserve_metadata
    }

    /// Returns the symbolic-link policy.
    #[inline(always)]
    pub const fn symlink_policy(&self) -> LocalSymlinkPolicy {
        self.symlink
    }

    /// Reports whether directory-tree copying is authorized.
    #[must_use]
    #[inline(always)]
    pub const fn recursive(&self) -> bool {
        self.recursive
    }

    /// Reports whether missing target parent directories are created.
    #[must_use]
    #[inline(always)]
    pub const fn creates_parent(&self) -> bool {
        self.create_parent
    }

    /// Returns the required atomicity.
    #[inline(always)]
    pub const fn atomicity(&self) -> LocalAtomicityRequirement {
        self.atomicity
    }

    /// Returns the required durability.
    #[inline(always)]
    pub const fn durability(&self) -> LocalDurabilityRequirement {
        self.durability
    }

    /// Sets the destination file conflict policy.
    #[inline(always)]
    pub const fn with_conflict(
        mut self,
        conflict: LocalCopyConflictPolicy,
    ) -> Self {
        self.conflict = conflict;
        self
    }

    /// Sets the file/directory type conflict policy.
    #[inline(always)]
    pub const fn with_type_conflict(
        mut self,
        type_conflict: LocalCopyTypeConflictPolicy,
    ) -> Self {
        self.type_conflict = type_conflict;
        self
    }

    /// Sets metadata preservation policy.
    #[inline(always)]
    pub const fn with_metadata_preservation(
        mut self,
        preserve_metadata: LocalMetadataPreservePolicy,
    ) -> Self {
        self.preserve_metadata = preserve_metadata;
        self
    }

    /// Sets symbolic-link policy.
    #[inline(always)]
    pub const fn with_symlink_policy(
        mut self,
        symlink: LocalSymlinkPolicy,
    ) -> Self {
        self.symlink = symlink;
        self
    }

    /// Authorizes copying a directory tree.
    #[inline(always)]
    pub const fn with_recursive(mut self) -> Self {
        self.recursive = true;
        self
    }

    /// Creates missing target parent directories before copying.
    #[inline(always)]
    pub const fn with_parent(mut self) -> Self {
        self.create_parent = true;
        self
    }

    /// Sets required publication atomicity.
    #[inline(always)]
    pub const fn with_atomicity(
        mut self,
        atomicity: LocalAtomicityRequirement,
    ) -> Self {
        self.atomicity = atomicity;
        self
    }

    /// Sets required durability.
    #[inline(always)]
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
    #[inline]
    fn default() -> Self {
        Self::new()
    }
}
