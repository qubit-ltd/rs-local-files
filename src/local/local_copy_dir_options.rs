// =============================================================================
//    Copyright (c) 2026 Haixing Hu.
//
//    SPDX-License-Identifier: Apache-2.0
//
//    Licensed under the Apache License, Version 2.0.
// =============================================================================
//! Recursive directory copy options.

use crate::{
    LocalCopyConflictPolicy,
    LocalCopyTypeConflictPolicy,
};

/// Options controlling recursive directory copy behavior.
///
/// The default is conservative: existing destination entries are not
/// overwritten, symbolic links are not followed, and source permissions are not
/// copied to destination entries. On Unix, newly created files therefore keep
/// the private staging mode `0o600` and newly created directories use `0o700`,
/// subject to a more restrictive process umask.
///
/// Construct this non-exhaustive type through [`Self::new`] and its builders.
/// Builder results must be used:
///
/// ```compile_fail
/// #![deny(unused_must_use)]
/// use qubit_local_files::LocalCopyDirOptions;
///
/// LocalCopyDirOptions::new().follow_symlinks();
/// ```
///
/// Configuration fields are private:
///
/// ```compile_fail
/// use qubit_local_files::LocalCopyDirOptions;
///
/// let mut options = LocalCopyDirOptions::default();
/// options.follow_symlinks = true;
/// ```
#[must_use = "directory copy options have no effect unless they are used"]
#[non_exhaustive]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LocalCopyDirOptions {
    /// Policy for existing destination file entries.
    conflict: LocalCopyConflictPolicy,

    /// Policy for source and destination entry type mismatches.
    type_conflict: LocalCopyTypeConflictPolicy,

    /// Whether symbolic links in the source tree should be followed.
    ///
    /// When this is `false`, encountering a symbolic link returns
    /// [`std::io::ErrorKind::Unsupported`]. This avoids accidentally copying
    /// data outside the requested source tree.
    ///
    /// Source inspection, source opening, destination reinspection, and
    /// destructive replacement are separate path-based operations. The
    /// symbolic link policy prevents ordinary accidental traversal, but it is
    /// not a sandbox boundary when an untrusted actor can mutate either tree
    /// concurrently. Use descriptor- or capability-relative filesystem APIs
    /// when containment must resist concurrent path replacement.
    follow_symlinks: bool,

    /// Whether to copy source permissions to destination entries after
    /// copying.
    ///
    /// This uses `std::fs::set_permissions` and therefore only preserves the
    /// portable permission bits exposed by the Rust standard library. When
    /// this is `false`, new or replaced files retain the copy staging mode and
    /// new directories retain the private directory mode; on Unix these are
    /// `0o600` and `0o700`, respectively, subject to the process umask.
    preserve_permissions: bool,
}

impl LocalCopyDirOptions {
    /// Returns conservative directory copy options.
    ///
    /// # Returns
    /// Options that fail on destination conflicts, do not follow symbolic
    /// links, and do not preserve source permissions.
    #[inline]
    pub const fn new() -> Self {
        Self {
            conflict: LocalCopyConflictPolicy::Fail,
            type_conflict: LocalCopyTypeConflictPolicy::Fail,
            follow_symlinks: false,
            preserve_permissions: false,
        }
    }

    /// Returns the destination file conflict policy.
    ///
    /// # Returns
    /// Policy applied to existing destination file entries.
    #[inline(always)]
    pub const fn conflict_policy(&self) -> LocalCopyConflictPolicy {
        self.conflict
    }

    /// Returns the entry type-conflict policy.
    ///
    /// # Returns
    /// Policy applied to source and destination type mismatches.
    #[inline(always)]
    pub const fn type_conflict_policy(&self) -> LocalCopyTypeConflictPolicy {
        self.type_conflict
    }

    /// Returns whether symbolic links will be followed.
    ///
    /// # Returns
    /// `true` when source symbolic links are followed.
    #[inline(always)]
    pub const fn follows_symlinks(&self) -> bool {
        self.follow_symlinks
    }

    /// Returns whether source permissions will be preserved.
    ///
    /// # Returns
    /// `true` when destination permissions are copied from the source.
    #[inline(always)]
    pub const fn preserves_permissions(&self) -> bool {
        self.preserve_permissions
    }

    /// Sets the policy for existing destination file entries.
    ///
    /// # Parameters
    /// - `conflict`: Conflict policy to use.
    ///
    /// # Returns
    /// Updated directory copy options.
    #[inline(always)]
    pub const fn with_conflict(
        mut self,
        conflict: LocalCopyConflictPolicy,
    ) -> Self {
        self.conflict = conflict;
        self
    }

    /// Sets the policy for source and destination type mismatches.
    ///
    /// # Parameters
    /// - `type_conflict`: Type-conflict policy to use.
    ///
    /// # Returns
    /// Updated directory copy options.
    #[inline(always)]
    pub const fn with_type_conflict(
        mut self,
        type_conflict: LocalCopyTypeConflictPolicy,
    ) -> Self {
        self.type_conflict = type_conflict;
        self
    }

    /// Enables following symbolic links in the source tree.
    ///
    /// # Returns
    /// Updated directory copy options.
    #[inline(always)]
    pub const fn follow_symlinks(mut self) -> Self {
        self.follow_symlinks = true;
        self
    }

    /// Enables preservation of source permissions.
    ///
    /// # Returns
    /// Updated directory copy options.
    #[inline(always)]
    pub const fn preserve_permissions(mut self) -> Self {
        self.preserve_permissions = true;
        self
    }
}

impl Default for LocalCopyDirOptions {
    /// Returns conservative directory copy options.
    ///
    /// # Returns
    /// Options that do not overwrite existing destination entries, do not
    /// follow symbolic links, and do not preserve source permissions.
    #[inline(always)]
    fn default() -> Self {
        Self::new()
    }
}
